use std::collections::HashMap;
use std::process::Command;
use std::sync::Mutex;

use serde::Deserialize;

use crate::audio::backend::AudioBackend;
use crate::audio::types::{is_virtual_sink, AppStream, OutputDevice};
use crate::error::SinkError;

/// `owner_module` value pactl uses when a sink has no owning module.
const PA_INVALID_INDEX: u32 = u32::MAX;

/// Phase 1 backend: drives the audio system through the `pactl` CLI, which
/// works against both PulseAudio and PipeWire (via pipewire-pulse).
///
/// Uses `pactl --format=json` (available since PulseAudio 16) so parsing is
/// structural rather than scraping human-oriented text.
pub struct PactlBackend {
    /// sink name -> index of the `module-null-sink` module that owns it.
    /// `create_virtual_sink` returns `()` per the trait, so module indices
    /// are tracked here instead of in `MixerState`.
    modules: Mutex<HashMap<String, u32>>,
    /// channel sink name -> index of its `module-loopback` (Phase 4 output
    /// routing fallback; the native backend uses passive links instead).
    loopbacks: Mutex<HashMap<String, u32>>,
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
    /// scanning the live module list. Fallback for when the in-memory table
    /// has no entry (e.g. sink left over from a previous crashed run).
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

        // Quote the description and escape it so a label with whitespace
        // (or quotes/backslashes) can't split into extra module properties -
        // pactl parses `sink_properties` as a space-delimited proplist, and
        // the value is otherwise attacker-influenced (TD-048). Control chars
        // are dropped so a newline can't start a new property line.
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
            // Sink does not exist — nothing to destroy. Treat as success so
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
                // Shared identity resolution: skips generic/wrapper names
                // (e.g. "WEBRTC VoiceEngine" → the Discord binary). The
                // winning property+value is the stream's persistent identity.
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

                AppStream {
                    index: input.index,
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
            .filter(|s| !is_virtual_sink(&s.name))
            .map(|s| OutputDevice {
                index: s.index,
                name: s.name,
                description: s.description,
            })
            .collect())
    }

    fn set_sink_volume(&self, sink_name: &str, volume_percent: u8) -> Result<(), SinkError> {
        Self::run(&[
            "set-sink-volume",
            sink_name,
            &format!("{volume_percent}%"),
        ])?;
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

    fn create_bus(&self, _name: &str, _label: &str) -> Result<(), SinkError> {
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

    fn set_monitor(&self, _name: &str, _enabled: bool) -> Result<(), SinkError> {
        Err(SinkError::Config(
            "monitoring requires the native PipeWire backend".into(),
        ))
    }

    fn get_default_devices(&self) -> Result<(Option<String>, Option<String>), SinkError> {
        let sink = Self::run(&["get-default-sink"]).ok().map(|s| s.trim().to_string());
        let source = Self::run(&["get-default-source"]).ok().map(|s| s.trim().to_string());
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

    fn set_channel_output(
        &self,
        sink_name: &str,
        output_name: Option<&str>,
    ) -> Result<(), SinkError> {
        // Replace any existing loopback for this channel — by asking the
        // server, not just our own table. A previous run that died without
        // teardown leaves its modules loaded, and stacking a fresh set on
        // top plays the channel once per leftover. (This assumes a single
        // Sink instance — sink names are deterministic, so two live
        // instances would already be fighting over the nodes themselves.)
        {
            let mut loopbacks = self
                .loopbacks
                .lock()
                .map_err(|_| SinkError::Parse("loopback table lock poisoned".into()))?;
            loopbacks.remove(sink_name);
        }
        let needle = format!("source={sink_name}.monitor");
        if let Ok(stdout) = Self::run(&["list", "modules", "short"]) {
            for line in stdout.lines() {
                if line.contains("module-loopback") && line.contains(&needle) {
                    if let Some(index) = line.split_whitespace().next() {
                        // Best effort: the module may already be gone.
                        let _ = Self::run(&["unload-module", index]);
                    }
                }
            }
        }

        let target = output_name.unwrap_or("@DEFAULT_SINK@");
        let stdout = Self::run(&[
            "load-module",
            "module-loopback",
            &format!("source={sink_name}.monitor"),
            &format!("sink={target}"),
            "source_dont_move=true",
        ])?;
        let module_index: u32 = stdout
            .trim()
            .parse()
            .map_err(|_| SinkError::Parse(format!("load-module returned {stdout:?}")))?;
        self.loopbacks
            .lock()
            .map_err(|_| SinkError::Parse("loopback table lock poisoned".into()))?
            .insert(sink_name.to_string(), module_index);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(prop(&inputs[0].properties, "application.name"), Some("Firefox"));
    }
}
