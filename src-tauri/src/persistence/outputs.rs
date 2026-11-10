use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::audio::types::is_own_sink;
use crate::error::SinkError;

/// Per-channel output device choices (JSON in `sink/outputs.json`).
/// `None` = follow the system default, with automatic failover.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ChannelOutputs {
    pub outputs: HashMap<String, Option<String>>,
    /// Channels with auto-failover off route only to their chosen device and
    /// stay silent when it's gone; `serde(default)` keeps old configs loading.
    #[serde(default)]
    pub no_failover: HashSet<String>,
}

impl ChannelOutputs {
    pub fn config_path() -> Result<PathBuf, SinkError> {
        let dir = crate::persistence::config_root()
            .ok_or_else(|| SinkError::Config("cannot resolve the user config directory".into()))?;
        Ok(dir.join("sink").join("outputs.json"))
    }

    pub fn load() -> Self {
        let Ok(path) = Self::config_path() else {
            return Self::default();
        };
        let mut outputs: Self = match fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
                eprintln!("sink: ignoring malformed {}: {e}", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        };
        outputs.drop_own_targets();
        outputs
    }

    pub fn save(&self) -> Result<(), SinkError> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            crate::persistence::ensure_private_dir(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| SinkError::Config(format!("serialize outputs: {e}")))?;
        super::write_atomic(&path, &json)?;
        Ok(())
    }

    pub fn set(&mut self, sink_name: &str, output: Option<String>) {
        self.outputs.insert(sink_name.to_string(), output);
    }

    pub fn get(&self, sink_name: &str) -> Option<&str> {
        self.outputs.get(sink_name)?.as_deref()
    }

    /// Whether this channel fails over to another device when its chosen
    /// device (or the default) is gone. On unless explicitly turned off.
    pub fn failover(&self, sink_name: &str) -> bool {
        !self.no_failover.contains(sink_name)
    }

    pub fn set_failover(&mut self, sink_name: &str, enabled: bool) {
        if enabled {
            self.no_failover.remove(sink_name);
        } else {
            self.no_failover.insert(sink_name.to_string());
        }
    }

    /// Every layer refuses an output on one of our own nodes, so a saved one
    /// (an old config, a hand edit) would only show a device the channel
    /// isn't playing on. Those channels follow the default instead.
    pub fn drop_own_targets(&mut self) {
        for output in self.outputs.values_mut() {
            if output.as_deref().is_some_and(is_own_sink) {
                *output = None;
            }
        }
    }

    /// Drop all state for a removed channel.
    pub fn remove(&mut self, sink_name: &str) {
        self.outputs.remove(sink_name);
        self.no_failover.remove(sink_name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_with_follow_default_entries() {
        let mut o = ChannelOutputs::default();
        o.set("sink_game", Some("alsa_output.usb-Headset".into()));
        o.set("sink_music", None);
        let json = serde_json::to_string(&o).expect("serializes");
        let back: ChannelOutputs = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back, o);
        assert_eq!(back.get("sink_game"), Some("alsa_output.usb-Headset"));
        assert_eq!(back.get("sink_music"), None);
    }

    #[test]
    fn failover_defaults_on_and_roundtrips() {
        let mut o = ChannelOutputs::default();
        assert!(o.failover("sink_game"), "failover on by default");
        o.set_failover("sink_game", false);
        assert!(!o.failover("sink_game"));
        let back: ChannelOutputs =
            serde_json::from_str(&serde_json::to_string(&o).unwrap()).unwrap();
        assert_eq!(back, o);
        assert!(!back.failover("sink_game"));
        // Turning it back on clears the entry rather than storing `true`.
        o.set_failover("sink_game", true);
        assert!(o.no_failover.is_empty());
    }

    #[test]
    fn old_config_without_no_failover_field_loads() {
        // Configs written before the failover flag have no `no_failover` key.
        let legacy = r#"{"outputs":{"sink_game":"dev","sink_music":null}}"#;
        let o: ChannelOutputs = serde_json::from_str(legacy).expect("legacy loads");
        assert!(o.failover("sink_game"), "missing field means failover on");
        assert_eq!(o.get("sink_game"), Some("dev"));
    }

    #[test]
    fn load_drops_outputs_on_our_own_nodes() {
        let _root = crate::persistence::testing::TempConfig::new("outputs-load-own");
        let mut o = ChannelOutputs::default();
        o.set("sink_game", Some("sink_chat".into()));
        o.set("sink_chat", Some("sink_bus_voice".into()));
        o.set("sink_music", Some("alsa_output.usb-Headset".into()));
        o.save().unwrap();

        let back = ChannelOutputs::load();
        assert_eq!(back.get("sink_game"), None);
        assert_eq!(back.get("sink_chat"), None);
        assert_eq!(back.get("sink_music"), Some("alsa_output.usb-Headset"));
    }

    #[test]
    fn remove_drops_output_and_failover() {
        let mut o = ChannelOutputs::default();
        o.set("sink_game", Some("dev".into()));
        o.set_failover("sink_game", false);
        o.remove("sink_game");
        assert_eq!(o.get("sink_game"), None);
        assert!(o.failover("sink_game"));
    }
}
