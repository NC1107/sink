use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

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
    /// Where each channel should play. Shared with the thread that keeps
    /// the `module-loopback`s in step with the devices that are connected
    /// (the native backend uses passive links and registry events instead).
    routing: Arc<Mutex<Routing>>,
}

/// pactl has no events to react to, so devices coming and going are noticed
/// by looking again this often.
const DEVICE_WATCH_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Default)]
struct Routing {
    /// channel sink name -> its chosen devices; empty = follow the default.
    /// Only channels that were routed at least once are in here.
    chosen: HashMap<String, Vec<String>>,
    /// Channels that go silent, not to the default, with no device left.
    strict: HashSet<String>,
    /// The devices the loopbacks were last matched against.
    matched_devices: Option<Vec<String>>,
}

/// One `module-loopback` a channel should have.
#[derive(Debug, Clone, PartialEq)]
struct Loopback {
    sink: String,
    /// A loopback follows its sink to the default device when the sink is
    /// unplugged. On a chosen device that would play the channel twice on
    /// the default (once per missing device), so those are pinned and die
    /// with the device; only the follow-default one may move.
    pinned: bool,
}

impl Loopback {
    fn arguments(&self, sink_name: &str) -> Vec<String> {
        let mut arguments = vec![
            format!("source={sink_name}.monitor"),
            format!("sink={}", self.sink),
            "source_dont_move=true".to_string(),
        ];
        if self.pinned {
            arguments.push("sink_dont_move=true".to_string());
        }
        arguments
    }

    fn is_loaded_as(&self, argument: &str) -> bool {
        let pinned = argument
            .split_whitespace()
            .any(|token| token == "sink_dont_move=true");
        let sink = format!("sink={}", self.sink);
        pinned == self.pinned && argument.split_whitespace().any(|token| token == sink)
    }
}

/// Where a channel plays, given the devices connected right now.
#[derive(Debug, PartialEq)]
enum Targets {
    Devices(Vec<String>),
    FollowDefault,
    Silent,
}

/// The native link planner's rule: a channel stays on its chosen devices
/// while at least one is connected, and only then follows the default (or
/// goes silent when its failover is off).
fn resolve_targets(chosen: &[String], strict: bool, connected: &[String]) -> Targets {
    let present: Vec<String> = chosen
        .iter()
        .filter(|name| connected.contains(name))
        .cloned()
        .collect();
    if !present.is_empty() {
        Targets::Devices(present)
    } else if chosen.is_empty() || !strict {
        Targets::FollowDefault
    } else {
        Targets::Silent
    }
}

/// What to change so a channel's loaded loopbacks (`(module index,
/// argument)`) become `wanted`: the wanted ones to load, and the module
/// indices to unload (duplicates of a wanted one included).
fn diff_loopbacks(loaded: &[(u32, String)], wanted: &[Loopback]) -> (Vec<Loopback>, Vec<u32>) {
    let mut keep = HashSet::new();
    let mut missing = Vec::new();
    for loopback in wanted {
        match loaded
            .iter()
            .find(|(_, argument)| loopback.is_loaded_as(argument))
        {
            Some((index, _)) => {
                keep.insert(*index);
            }
            None => missing.push(loopback.clone()),
        }
    }
    let stale = loaded
        .iter()
        .map(|(index, _)| *index)
        .filter(|index| !keep.contains(index))
        .collect();
    (missing, stale)
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

/// A loaded module, from the tab-separated `pactl list modules short`.
/// The JSON listing can't be used for these: pactl 17 leaves the index out.
#[derive(Debug, PartialEq)]
struct PactlModule {
    index: u32,
    name: String,
    argument: String,
}

fn parse_modules(listing: &str) -> Vec<PactlModule> {
    listing
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            // A native module's argument can span lines; the continuation
            // lines carry no index and are not modules of ours anyway.
            let index = fields.next()?.parse().ok()?;
            Some(PactlModule {
                index,
                name: fields.next()?.to_string(),
                argument: fields.next().unwrap_or_default().trim().to_string(),
            })
        })
        .collect()
}

impl PactlBackend {
    pub fn new() -> Self {
        let routing = Arc::new(Mutex::new(Routing::default()));
        let watched = Arc::downgrade(&routing);
        if let Err(e) = std::thread::Builder::new()
            .name("sink-pactl-devices".into())
            .spawn(move || Self::watch_devices(&watched))
        {
            eprintln!("sink: no device watcher ({e}); replugged devices need a re-pick");
        }
        Self {
            modules: Mutex::new(HashMap::new()),
            routing,
        }
    }

    /// Re-match every routed channel's loopbacks whenever the set of
    /// connected devices changes, until the backend is dropped.
    fn watch_devices(routing: &Weak<Mutex<Routing>>) {
        loop {
            std::thread::sleep(DEVICE_WATCH_INTERVAL);
            let Some(routing) = routing.upgrade() else {
                return;
            };
            let Ok(mut routing) = routing.lock() else {
                return;
            };
            if routing.chosen.is_empty() {
                continue;
            }
            let Ok(connected) = Self::connected_devices() else {
                continue;
            };
            if routing.matched_devices.as_ref() == Some(&connected) {
                continue;
            }
            for sink_name in routing.chosen.keys() {
                if let Err(e) = Self::match_loopbacks(&routing, sink_name, &connected) {
                    eprintln!("sink: output routing for {sink_name} failed: {e}");
                }
            }
            routing.matched_devices = Some(connected);
        }
    }

    fn lock_routing(&self) -> Result<MutexGuard<'_, Routing>, SinkError> {
        self.routing
            .lock()
            .map_err(|_| SinkError::Parse("output routing lock poisoned".into()))
    }

    fn connected_devices() -> Result<Vec<String>, SinkError> {
        Ok(Self::list_sinks()?
            .into_iter()
            .map(|sink| sink.name)
            .filter(|name| !is_own_sink(name))
            .collect())
    }

    /// Load and unload `module-loopback`s until `sink_name` plays where its
    /// routing says it should with `connected` devices.
    fn match_loopbacks(
        routing: &Routing,
        sink_name: &str,
        connected: &[String],
    ) -> Result<(), SinkError> {
        let chosen = routing.chosen.get(sink_name).map_or(&[][..], Vec::as_slice);
        let strict = routing.strict.contains(sink_name);
        let wanted: Vec<Loopback> = match resolve_targets(chosen, strict, connected) {
            Targets::Devices(devices) => devices
                .into_iter()
                .map(|sink| Loopback { sink, pinned: true })
                .collect(),
            Targets::FollowDefault => vec![Loopback {
                sink: Self::follow_default_target(sink_name, connected)?,
                pinned: false,
            }],
            Targets::Silent => Vec::new(),
        };
        // Ask the server what is loaded instead of remembering it: a pinned
        // loopback unloads itself with its device, and a crashed prior run
        // leaves modules behind that would double-play if stacked on.
        let source = format!("source={sink_name}.monitor");
        let loaded: Vec<(u32, String)> = Self::list_modules()?
            .into_iter()
            .filter(|module| module.name == "module-loopback")
            .map(|module| (module.index, module.argument))
            .filter(|(_, argument)| argument.split_whitespace().any(|token| token == source))
            .collect();
        let (missing, stale) = diff_loopbacks(&loaded, &wanted);

        // Loaded before the stale ones go: if none of the new devices takes
        // a loopback, the channel keeps playing where it was instead of
        // going silent on an error.
        let mut failed = None;
        let mut playing = wanted.len() - missing.len();
        for loopback in &missing {
            let arguments = loopback.arguments(sink_name);
            let mut command = vec!["load-module", "module-loopback"];
            command.extend(arguments.iter().map(String::as_str));
            match Self::run(&command) {
                Ok(_) => playing += 1,
                Err(e) => {
                    eprintln!("sink: {sink_name} can't play on {}: {e}", loopback.sink);
                    failed.get_or_insert(e);
                }
            }
        }
        // One device refusing is not a failure while another plays.
        if let Some(e) = failed.filter(|_| playing == 0) {
            return Err(e);
        }
        for index in stale {
            // Best effort: the module may already be gone.
            let _ = Self::run(&["unload-module", &index.to_string()]);
        }
        Ok(())
    }

    /// Route `sink_name` by `change`d routing, restoring the routing it had
    /// if nothing could be made to play.
    fn reroute(&self, sink_name: &str, change: impl FnOnce(&mut Routing)) -> Result<(), SinkError> {
        let mut routing = self.lock_routing()?;
        let chosen_before = routing.chosen.get(sink_name).cloned();
        let strict_before = routing.strict.contains(sink_name);
        change(&mut routing);
        let matched = Self::connected_devices().and_then(|connected| {
            Self::match_loopbacks(&routing, sink_name, &connected)?;
            Ok(connected)
        });
        match matched {
            Ok(connected) => {
                // Only this channel was matched; the watcher still owes the
                // others a pass if the devices changed.
                if routing.matched_devices.as_ref() != Some(&connected) {
                    routing.matched_devices = None;
                }
                Ok(())
            }
            Err(e) => {
                match chosen_before {
                    Some(chosen) => routing.chosen.insert(sink_name.to_string(), chosen),
                    None => routing.chosen.remove(sink_name),
                };
                if strict_before {
                    routing.strict.insert(sink_name.to_string());
                } else {
                    routing.strict.remove(sink_name);
                }
                Err(e)
            }
        }
    }

    /// Where a follow-default channel's loopback plays. A user can make one
    /// of our channels the system default, and following it would loop every
    /// follow-default channel into it, so fall back to a real device then
    /// (the native backend's link planner does the same).
    fn follow_default_target(sink_name: &str, connected: &[String]) -> Result<String, SinkError> {
        let default = Self::run(&["get-default-sink"]).ok();
        if !default.is_some_and(|sink| is_own_sink(sink.trim())) {
            return Ok("@DEFAULT_SINK@".to_string());
        }
        connected
            .first()
            .cloned()
            .ok_or_else(|| SinkError::Config(own_output_refusal(sink_name)))
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

    fn list_modules() -> Result<Vec<PactlModule>, SinkError> {
        Ok(parse_modules(&Self::run(&["list", "modules", "short"])?))
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
        let needle = format!("sink_name={sink_name}");
        Ok(Self::list_modules()?
            .iter()
            .find(|m| {
                m.name == "module-null-sink"
                    && m.argument.split_whitespace().any(|tok| tok == needle)
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
        // Its loopbacks die with their source; forgetting the routing keeps
        // the device watcher from loading them again.
        {
            let mut routing = self.lock_routing()?;
            routing.chosen.remove(name);
            routing.strict.remove(name);
        }
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
        self.reroute(sink_name, |routing| {
            routing.chosen.insert(sink_name.to_string(), outputs);
        })
    }

    fn set_channel_failover(&self, sink_name: &str, enabled: bool) -> Result<(), SinkError> {
        let mut routing = self.lock_routing()?;
        let routed = routing.chosen.contains_key(sink_name);
        if !routed {
            // Nothing to re-match yet; the first routing picks this up.
            if enabled {
                routing.strict.remove(sink_name);
            } else {
                routing.strict.insert(sink_name.to_string());
            }
            return Ok(());
        }
        drop(routing);
        self.reroute(sink_name, |routing| {
            if enabled {
                routing.strict.remove(sink_name);
            } else {
                routing.strict.insert(sink_name.to_string());
            }
        })
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

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn modules_are_read_from_the_short_listing() {
        let listing =
            "1\tlibpipewire-module-protocol-native\t{\n            #sockets = [ ]\n        }\t\n\
            536870913\tmodule-null-sink\tsink_name=sink_game sink_properties=x\t\n\
            536870914\tmodule-loopback\tsource=sink_game.monitor sink=speakers\t\n\
            536870915\tmodule-always-sink\t\t\n";
        let modules = parse_modules(listing);
        assert_eq!(modules.len(), 4);
        assert_eq!(
            modules[2],
            PactlModule {
                index: 536870914,
                name: "module-loopback".into(),
                argument: "source=sink_game.monitor sink=speakers".into(),
            }
        );
        assert_eq!(modules[3].argument, "");
    }

    #[test]
    fn a_channel_stays_on_its_devices_until_none_is_connected() {
        let chosen = names(&["speakers", "headset"]);
        assert_eq!(
            resolve_targets(&chosen, false, &names(&["hdmi", "headset", "speakers"])),
            Targets::Devices(names(&["speakers", "headset"]))
        );
        // One unplugged: the other keeps the channel, nothing follows the default.
        assert_eq!(
            resolve_targets(&chosen, false, &names(&["hdmi", "speakers"])),
            Targets::Devices(names(&["speakers"]))
        );
        assert_eq!(
            resolve_targets(&chosen, false, &names(&["hdmi"])),
            Targets::FollowDefault
        );
        assert_eq!(
            resolve_targets(&chosen, true, &names(&["hdmi"])),
            Targets::Silent
        );
        // No choice is not a missing device: failover off changes nothing.
        assert_eq!(
            resolve_targets(&[], true, &names(&["hdmi"])),
            Targets::FollowDefault
        );
    }

    #[test]
    fn loopbacks_are_matched_by_device_and_pinning() {
        let pinned = |sink: &str| Loopback {
            sink: sink.to_string(),
            pinned: true,
        };
        let loaded = vec![
            (4, pinned("speakers").arguments("sink_game").join(" ")),
            // A duplicate, as a crashed run leaves behind.
            (5, pinned("speakers").arguments("sink_game").join(" ")),
            // The same device unpinned (an older Sink) would follow it to
            // the default when unplugged, so it is replaced.
            (
                6,
                "source=sink_game.monitor sink=headset source_dont_move=true".to_string(),
            ),
            (7, pinned("hdmi").arguments("sink_game").join(" ")),
        ];
        let (missing, stale) = diff_loopbacks(&loaded, &[pinned("speakers"), pinned("headset")]);
        assert_eq!(missing, [pinned("headset")]);
        assert_eq!(stale, [5, 6, 7]);

        // A device name that only starts like another is a different device.
        assert!(!pinned("speakers").is_loaded_as(
            "source=sink_game.monitor sink=speakers.2 source_dont_move=true sink_dont_move=true"
        ));
        let (missing, stale) = diff_loopbacks(&loaded, &[]);
        assert!(missing.is_empty());
        assert_eq!(stale, [4, 5, 6, 7]);
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
