use std::collections::HashMap;
use std::process::Command;
use std::sync::Mutex;

use serde::Deserialize;

use crate::audio::backend::AudioBackend;
use crate::audio::types::{
    checked_channel_outputs, is_own_sink, is_virtual_sink, own_output_refusal, AppStream,
    OutputDevice,
};
use crate::error::SinkError;

/// `owner_module` value pactl uses when a sink has no owning module.
const PA_INVALID_INDEX: u32 = u32::MAX;

/// Backend that drives the audio system through the `pactl` CLI (works on both
/// PulseAudio and PipeWire); `--format=json` avoids scraping text.
pub struct PactlBackend {
    /// sink name -> index of the `module-null-sink` module that owns it,
    /// tracked here since `create_virtual_sink` returns `()` per the trait.
    modules: Mutex<HashMap<String, u32>>,
    /// channel sink name -> indices of its `module-loopback`s, one per
    /// output device (output routing fallback; the native backend uses
    /// passive links instead).
    loopbacks: Mutex<HashMap<String, Vec<u32>>>,
}

// ---- JSON shapes for `pactl --format=json` output ----

#[derive(Deserialize)]
struct PactlVolume {
    value_percent: String,
}

// Channel volume/mute state is owned by MixerState, so sinks only need
// identity fields here. serde ignores the unparsed JSON keys.
#[derive(Deserialize)]
struct PactlSink {
    index: u32,
    name: String,
    description: String,
    #[serde(default)]
    owner_module: Option<u32>,
}

#[derive(Deserialize)]
struct PactlSinkInput {
    index: u32,
    /// Index of the sink this stream is currently connected to.
    sink: u32,
    mute: bool,
    #[serde(default)]
    corked: bool,
    volume: HashMap<String, PactlVolume>,
    #[serde(default)]
    properties: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct PactlModule {
    index: u32,
    name: String,
    #[serde(default)]
    argument: Option<String>,
}

impl PactlBackend {
    pub fn new() -> Self {
        Self {
            modules: Mutex::new(HashMap::new()),
            loopbacks: Mutex::new(HashMap::new()),
        }
    }

    /// Where a follow-default channel's loopback plays. A user can make one
    /// of our channels the system default, and following it would loop every
    /// follow-default channel into it, so fall back to a real device then
    /// (the native backend's link planner does the same).
    fn follow_default_target(&self, sink_name: &str) -> Result<String, SinkError> {
        let default = self.get_default_devices().ok().and_then(|(sink, _)| sink);
        if !default.as_deref().is_some_and(is_own_sink) {
            return Ok("@DEFAULT_SINK@".to_string());
        }
        self.list_output_devices()?
            .into_iter()
            .map(|d| d.name)
            .find(|name| !is_own_sink(name))
            .ok_or_else(|| SinkError::Config(own_output_refusal(sink_name)))
    }

    /// Play a channel's monitor on one device; returns the module index.
    fn load_loopback(sink_name: &str, target: &str) -> Result<u32, SinkError> {
        let stdout = Self::run(&[
            "load-module",
            "module-loopback",
            &format!("source={sink_name}.monitor"),
            &format!("sink={target}"),
            "source_dont_move=true",
        ])?;
        stdout
            .trim()
            .parse()
            .map_err(|_| SinkError::Parse(format!("load-module returned {stdout:?}")))
    }

    /// Run pactl with the given args, returning stdout on success.
    fn run(args: &[&str]) -> Result<String, SinkError> {
        let output = Command::new("pactl").args(args).output().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                SinkError::PactlNotFound
            } else {
                SinkError::Io(e)
            }
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr = stderr.trim();
            if stderr.contains("Connection refused") || stderr.contains("Connection failure") {
                return Err(SinkError::ServerUnreachable);
            }
            return Err(SinkError::CommandFailed(format!(
                "pactl {}: {}",
                args.join(" "),
                stderr
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Run a `pactl --format=json list <kind>` query and deserialize it.
    fn query<T: serde::de::DeserializeOwned>(kind: &str) -> Result<T, SinkError> {
        let stdout = Self::run(&["--format=json", "list", kind])?;
        serde_json::from_str(&stdout)
            .map_err(|e| SinkError::Parse(format!("`pactl list {kind}`: {e}")))
    }

    fn list_sinks() -> Result<Vec<PactlSink>, SinkError> {
        Self::query("sinks")
    }

    fn lock_modules(&self) -> Result<std::sync::MutexGuard<'_, HashMap<String, u32>>, SinkError> {
        self.modules
            .lock()
            .map_err(|_| SinkError::Parse("module index table lock poisoned".into()))
    }

    /// Find the module index of a `module-null-sink` owning `sink_name` by
    /// scanning live modules - covers a sink left over from a crashed run.
    fn find_null_sink_module(sink_name: &str) -> Result<Option<u32>, SinkError> {
        let modules: Vec<PactlModule> = Self::query("modules")?;
        let needle = format!("sink_name={sink_name}");
        Ok(modules
            .iter()
            .find(|m| {
                m.name == "module-null-sink"
                    && m.argument
                        .as_deref()
                        .map(|a| a.split_whitespace().any(|tok| tok == needle))
                        .unwrap_or(false)
            })
            .map(|m| m.index))
    }
}

/// Parse a pactl `value_percent` string like "87%" into a percentage.
/// Multi-channel volumes are collapsed to the loudest channel.
fn volume_percent(volume: &HashMap<String, PactlVolume>) -> u8 {
    volume
        .values()
        .filter_map(|v| v.value_percent.trim_end_matches('%').parse::<u32>().ok())
        .max()
        .unwrap_or(100)
        .min(u8::MAX as u32) as u8
}

/// Extract a string property from a pactl properties map.
fn prop<'a>(props: &'a HashMap<String, serde_json::Value>, key: &str) -> Option<&'a str> {
    props.get(key).and_then(|v| v.as_str())
}

impl AudioBackend for PactlBackend {
    fn create_virtual_sink(&self, name: &str, label: &str) -> Result<(), SinkError> {
        // Idempotency: if the sink already exists (e.g. previous run crashed
        // before teardown), adopt its module instead of loading a duplicate.
        if let Some(existing) = Self::list_sinks()?.iter().find(|s| s.name == name) {
            match existing.owner_module {
                Some(idx) if idx != PA_INVALID_INDEX => {
                    self.lock_modules()?.insert(name.to_string(), idx);
                    return Ok(());
                }
                _ => {
                    if let Some(idx) = Self::find_null_sink_module(name)? {
                        self.lock_modules()?.insert(name.to_string(), idx);
                        return Ok(());
                    }
                }
            }
        }

        // Escape the description so a label with whitespace/quotes can't split
        // pactl's space-delimited sink_properties into extra properties.
        let desc: String = label
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect::<String>()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        let stdout = Self::run(&[
            "load-module",
            "module-null-sink",
            &format!("sink_name={name}"),
            &format!("sink_properties=device.description=\"{desc}\""),
        ])?;
        let module_index: u32 = stdout
            .trim()
            .parse()
            .map_err(|_| SinkError::Parse(format!("load-module returned {stdout:?}")))?;

        self.lock_modules()?.insert(name.to_string(), module_index);
        Ok(())
    }

    fn destroy_virtual_sink(&self, name: &str) -> Result<(), SinkError> {
        let tracked = self.lock_modules()?.remove(name);
        let module_index = match tracked {
            Some(idx) => Some(idx),
            None => Self::find_null_sink_module(name)?,
        };

        match module_index {
            Some(idx) => {
                Self::run(&["unload-module", &idx.to_string()])?;
                Ok(())
            }
            // Sink does not exist - nothing to destroy. Treat as success so
            // teardown is idempotent.
            None => Ok(()),
        }
    }

    fn list_app_streams(&self) -> Result<Vec<AppStream>, SinkError> {
        // Map sink index -> sink name so we can resolve each stream's
        // current channel assignment.
        let sink_names: HashMap<u32, String> = Self::list_sinks()?
            .into_iter()
            .map(|s| (s.index, s.name))
            .collect();

        let inputs: Vec<PactlSinkInput> = Self::query("sink-inputs")?;
        Ok(inputs
            .into_iter()
            .map(|input| {
                // Skips generic/wrapper names so the winning property+value
                // becomes the stream's persistent identity.
                let (app_name, match_prop, match_value) =
                    crate::audio::types::resolve_identity(|key| {
                        prop(&input.properties, key).map(str::to_string)
                    });
                let icon_name =
                    prop(&input.properties, "application.icon_name").map(str::to_string);
                let assigned_sink = sink_names
                    .get(&input.sink)
                    .filter(|name| is_virtual_sink(name))
                    .cloned();
                // pactl shows the client's own claims; the daemon-owned
                // keys are dropped like the native backend does.
                let props: HashMap<String, String> = input
                    .properties
                    .iter()
                    .filter(|(k, _)| !crate::audio::types::daemon_owned(k))
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect();

                AppStream {
                    props,
                    settled: true,
                    index: input.index,
                    // PulseAudio's sink-input index is itself never reused.
                    serial: u64::from(input.index),
                    app_name,
                    match_prop,
                    match_value,
                    // Filled in by the command layer from the saved aliases.
                    alias: None,
                    icon_name,
                    icon_path: None,
                    pid: prop(&input.properties, "application.process.id")
                        .and_then(|v| v.parse().ok()),
                    assigned_sink,
                    volume_percent: volume_percent(&input.volume),
                    muted: input.mute,
                    active: !input.corked,
                }
            })
            .collect())
    }

    fn list_output_devices(&self) -> Result<Vec<OutputDevice>, SinkError> {
        Ok(Self::list_sinks()?
            .into_iter()
            .filter(|s| !is_own_sink(&s.name))
            .map(|s| OutputDevice {
                index: s.index,
                name: s.name,
                description: s.description,
            })
            .collect())
    }

    fn set_sink_volume(&self, sink_name: &str, volume_percent: u8) -> Result<(), SinkError> {
        Self::run(&["set-sink-volume", sink_name, &format!("{volume_percent}%")])?;
        Ok(())
    }

    fn set_sink_mute(&self, sink_name: &str, muted: bool) -> Result<(), SinkError> {
        Self::run(&["set-sink-mute", sink_name, if muted { "1" } else { "0" }])?;
        Ok(())
    }

    fn move_stream_to_sink(&self, stream_index: u32, sink_name: &str) -> Result<(), SinkError> {
        // Empty sink name = unassign: hand the stream back to the default sink.
        let target = if sink_name.is_empty() {
            "@DEFAULT_SINK@"
        } else {
            sink_name
        };
        Self::run(&["move-sink-input", &stream_index.to_string(), target])?;
        Ok(())
    }

    fn set_app_volume(&self, stream_index: u32, volume_percent: u8) -> Result<(), SinkError> {
        Self::run(&[
            "set-sink-input-volume",
            &stream_index.to_string(),
            &format!("{volume_percent}%"),
        ])?;
        Ok(())
    }

    fn list_input_devices(&self) -> Result<Vec<OutputDevice>, SinkError> {
        #[derive(Deserialize)]
        struct PactlSource {
            index: u32,
            name: String,
            description: String,
        }
        let sources: Vec<PactlSource> = Self::query("sources")?;
        Ok(sources
            .into_iter()
            .filter(|s| !s.name.ends_with(".monitor"))
            .map(|s| OutputDevice {
                index: s.index,
                name: s.name,
                description: s.description,
            })
            .collect())
    }

    fn set_mic_config(&self, _config: &crate::audio::types::MicConfig) -> Result<(), SinkError> {
        Err(SinkError::Config(
            "the mic DSP chain requires the native PipeWire backend".into(),
        ))
    }

    fn set_channel_eq(
        &self,
        _sink_name: &str,
        _config: &crate::audio::types::EqConfig,
    ) -> Result<(), SinkError> {
        Err(SinkError::Config(
            "parametric EQ requires the native PipeWire backend".into(),
        ))
    }

    fn create_bus(
        &self,
        _name: &str,
        _label: &str,
        _role: crate::persistence::buses::MixRole,
    ) -> Result<(), SinkError> {
        Err(SinkError::Config(
            "mix buses require the native PipeWire backend".into(),
        ))
    }

    fn destroy_bus(&self, _name: &str) -> Result<(), SinkError> {
        Ok(()) // nothing to destroy in the fallback
    }

    fn set_bus_members(&self, _name: &str, _channels: &[String]) -> Result<(), SinkError> {
        Err(SinkError::Config(
            "mix buses require the native PipeWire backend".into(),
        ))
    }

    fn set_bus_mic(&self, _name: &str, _mic: bool) -> Result<(), SinkError> {
        Err(SinkError::Config(
            "mic-in-mix requires the native PipeWire backend".into(),
        ))
    }

    fn set_bus_member_gain(
        &self,
        _bus_name: &str,
        _member: &str,
        _percent: u8,
    ) -> Result<(), SinkError> {
        Err(SinkError::Config(
            "per-mix send levels require the native PipeWire backend".into(),
        ))
    }

    fn set_monitor(&self, _name: &str, _enabled: bool) -> Result<(), SinkError> {
        Err(SinkError::Config(
            "monitoring requires the native PipeWire backend".into(),
        ))
    }

    fn get_default_devices(&self) -> Result<(Option<String>, Option<String>), SinkError> {
        let sink = Self::run(&["get-default-sink"])
            .ok()
            .map(|s| s.trim().to_string());
        let source = Self::run(&["get-default-source"])
            .ok()
            .map(|s| s.trim().to_string());
        Ok((
            sink.filter(|s| !s.is_empty()),
            source.filter(|s| !s.is_empty()),
        ))
    }

    fn set_default_output(&self, name: &str) -> Result<(), SinkError> {
        Self::run(&["set-default-sink", name])?;
        Ok(())
    }

    fn set_default_input(&self, name: &str) -> Result<(), SinkError> {
        Self::run(&["set-default-source", name])?;
        Ok(())
    }

    fn set_channel_outputs(&self, sink_name: &str, outputs: &[String]) -> Result<(), SinkError> {
        // Same guards as the command layer and the native backend, before
        // any teardown: profile/startup restore calls the backend directly,
        // and a stale saved target must never loop a channel into our own
        // nodes - here as a real module-loopback, not just a link.
        if !is_virtual_sink(sink_name) {
            return Err(SinkError::UnknownSink(sink_name.to_string()));
        }
        let outputs = checked_channel_outputs(sink_name, outputs)?;
        if let Some(name) = outputs.iter().find(|name| !is_plain_device_name(name)) {
            return Err(SinkError::Config(format!(
                "{name:?} is not a device name pactl can address"
            )));
        }
        let targets = if outputs.is_empty() {
            vec![self.follow_default_target(sink_name)?]
        } else {
            outputs
        };
        // Ask the server for existing loopbacks, not just our table - a crashed
        // prior run leaves modules loaded, and stacking more would double-play.
        let needle = format!("source={sink_name}.monitor");
        let previous: Vec<String> = Self::run(&["list", "modules", "short"])
            .map(|stdout| {
                stdout
                    .lines()
                    .filter(|line| line.contains("module-loopback") && line.contains(&needle))
                    .filter_map(|line| line.split_whitespace().next().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        // One loopback per device, loaded before the previous ones go: if
        // none of the new devices is there, the channel keeps playing where
        // it was instead of going silent on an error.
        let loaded = loaded_or_first_error(
            targets
                .iter()
                .map(|target| {
                    Self::load_loopback(sink_name, target).inspect_err(|e| {
                        eprintln!("sink: {sink_name} can't play on {target}: {e}");
                    })
                })
                .collect(),
        )?;
        for index in &previous {
            // Best effort: the module may already be gone.
            let _ = Self::run(&["unload-module", index]);
        }
        self.loopbacks
            .lock()
            .map_err(|_| SinkError::Parse("loopback table lock poisoned".into()))?
            .insert(sink_name.to_string(), loaded);
        Ok(())
    }
}

/// pactl joins a module's arguments into one space-separated string, so a
/// name with a space or a quote in it would be read as extra module
/// arguments. Real device node names have neither.
fn is_plain_device_name(name: &str) -> bool {
    !name
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '"' || c == '\'')
}

/// A channel routed to several devices plays on the ones that loaded - one
/// of them being unplugged is not a failure. Only none loading is.
fn loaded_or_first_error(results: Vec<Result<u32, SinkError>>) -> Result<Vec<u32>, SinkError> {
    let mut loaded = Vec::new();
    let mut first_error = None;
    for result in results {
        match result {
            Ok(module_index) => loaded.push(module_index),
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
    }
    match first_error {
        Some(e) if loaded.is_empty() => Err(e),
        _ => Ok(loaded),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The own-node guard fires before any pactl call, so this runs with
    /// no audio server: a refused target must not reach the loopback code.
    #[test]
    fn channel_output_rejects_own_nodes_and_non_channels() {
        let backend = PactlBackend::new();
        // One own node refuses the whole list, real device or not.
        let err = backend
            .set_channel_outputs(
                "sink_game",
                &["alsa_output.pci".to_string(), "sink_chat".to_string()],
            )
            .expect_err("own node must be refused");
        assert!(err.to_string().contains("loops the channel back"), "{err}");
        assert!(matches!(
            backend.set_channel_outputs("alsa_output.usb", &["alsa_output.pci".to_string()]),
            Err(SinkError::UnknownSink(_))
        ));
    }

    #[test]
    fn a_channel_plays_on_the_devices_that_loaded() {
        let gone = || Err(SinkError::Parse("no such sink".into()));
        assert_eq!(
            loaded_or_first_error(vec![Ok(3), gone(), Ok(9)]).expect("two loaded"),
            [3, 9]
        );
        assert!(loaded_or_first_error(vec![gone(), gone()]).is_err());
        // Follow-default with nothing to load is not an error either.
        assert_eq!(
            loaded_or_first_error(vec![]).expect("empty"),
            Vec::<u32>::new()
        );
    }

    /// A space would end `sink=<name>` and start another module argument.
    #[test]
    fn a_device_name_cannot_smuggle_module_arguments() {
        let backend = PactlBackend::new();
        for name in [
            "alsa_output.pci source_dont_move=false",
            " sink_chat",
            "alsa\"quoted",
            "tab\tseparated",
        ] {
            let err = backend
                .set_channel_outputs("sink_game", &[name.to_string()])
                .expect_err("refused before any pactl call");
            assert!(
                err.to_string().contains("not a device name"),
                "{name}: {err}"
            );
        }
        assert!(is_plain_device_name(
            "alsa_output.usb-Generic_USB_Audio-00.HiFi__Speaker__sink"
        ));
        assert!(is_plain_device_name("bluez_output.AA_BB_CC_DD_EE_FF.1"));
    }

    #[test]
    fn parses_volume_percent() {
        let mut vol = HashMap::new();
        vol.insert(
            "front-left".to_string(),
            PactlVolume {
                value_percent: "87%".to_string(),
            },
        );
        vol.insert(
            "front-right".to_string(),
            PactlVolume {
                value_percent: "92%".to_string(),
            },
        );
        assert_eq!(volume_percent(&vol), 92);
    }

    #[test]
    fn volume_percent_defaults_to_100_on_garbage() {
        let mut vol = HashMap::new();
        vol.insert(
            "mono".to_string(),
            PactlVolume {
                value_percent: "not-a-number".to_string(),
            },
        );
        assert_eq!(volume_percent(&vol), 100);
    }

    #[test]
    fn parses_real_sink_json() {
        let json = r#"[{"index":66,"state":"SUSPENDED","name":"alsa_output.usb-Arctis-00.analog-stereo","description":"Arctis Analog Stereo","mute":false,"owner_module":4294967295,"volume":{"front-left":{"value":57016,"value_percent":"87%","db":"-3.63 dB"}}}]"#;
        let sinks: Vec<PactlSink> = serde_json::from_str(json).expect("sink json should parse");
        assert_eq!(sinks[0].index, 66);
        assert_eq!(sinks[0].name, "alsa_output.usb-Arctis-00.analog-stereo");
        assert_eq!(sinks[0].owner_module, Some(PA_INVALID_INDEX));
    }

    #[test]
    fn parses_sink_input_json() {
        let json = r#"[{"index":12,"sink":66,"mute":false,"volume":{"front-left":{"value":65536,"value_percent":"100%","db":"0.00 dB"}},"properties":{"application.name":"Firefox","application.icon_name":"firefox"}}]"#;
        let inputs: Vec<PactlSinkInput> =
            serde_json::from_str(json).expect("sink-input json should parse");
        assert_eq!(inputs[0].index, 12);
        assert_eq!(
            prop(&inputs[0].properties, "application.name"),
            Some("Firefox")
        );
    }
}
