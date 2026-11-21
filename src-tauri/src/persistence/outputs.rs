use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::audio::types::{distinct_outputs, is_own_sink, MAX_CHANNEL_OUTPUTS};
use crate::error::SinkError;

/// Per-channel output device choices (JSON in `sink/outputs.json`). A channel
/// plays on every device in its list; an empty list follows the system
/// default, with automatic failover.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ChannelOutputs {
    #[serde(with = "device_lists")]
    pub outputs: HashMap<String, Vec<String>>,
    /// Channels with auto-failover off route only to their chosen device and
    /// stay silent when it's gone; `serde(default)` keeps old configs loading.
    #[serde(default)]
    pub no_failover: HashSet<String>,
}

/// On disk a channel with one device stays a plain string and follow-default
/// stays `null`, exactly as before multi-output; only a channel that really
/// has several devices is written as a list. So a config that never uses the
/// feature still loads in an older Sink after a downgrade.
mod device_lists {
    use std::collections::HashMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    #[serde(untagged)]
    enum Stored {
        One(String),
        Many(Vec<String>),
    }

    pub fn serialize<S: Serializer>(
        outputs: &HashMap<String, Vec<String>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let stored: HashMap<&String, Option<Stored>> = outputs
            .iter()
            .map(|(channel, devices)| {
                let stored = match devices.as_slice() {
                    [] => None,
                    [device] => Some(Stored::One(device.clone())),
                    devices => Some(Stored::Many(devices.to_vec())),
                };
                (channel, stored)
            })
            .collect();
        stored.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<HashMap<String, Vec<String>>, D::Error> {
        // Entry by entry: one channel a hand edit broke (a number, a nested
        // list) follows the default, and every other channel keeps its
        // devices instead of the whole file being thrown away.
        let stored = HashMap::<String, serde_json::Value>::deserialize(deserializer)?;
        Ok(stored
            .into_iter()
            .map(|(channel, value)| {
                let devices = match serde_json::from_value::<Option<Stored>>(value) {
                    Ok(None) => Vec::new(),
                    Ok(Some(Stored::One(device))) => vec![device],
                    Ok(Some(Stored::Many(devices))) => devices,
                    Err(e) => {
                        eprintln!("sink: ignoring the saved outputs of {channel}: {e}");
                        Vec::new()
                    }
                };
                (channel, super::normalize(&devices))
            })
            .collect())
    }
}

/// What a stored list is trusted as: no blanks or repeats, and cut to what
/// a channel may have. A command refuses an over-long list; a file that
/// already holds one keeps its first devices rather than losing them all.
fn normalize(devices: &[String]) -> Vec<String> {
    let mut kept = distinct_outputs(devices);
    kept.truncate(MAX_CHANNEL_OUTPUTS);
    kept
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

    pub fn set(&mut self, sink_name: &str, devices: Vec<String>) {
        self.outputs
            .insert(sink_name.to_string(), normalize(&devices));
    }

    /// The channel's chosen devices; empty means it follows the default.
    pub fn get(&self, sink_name: &str) -> &[String] {
        self.outputs.get(sink_name).map_or(&[], Vec::as_slice)
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
    /// isn't playing on. It is dropped; a channel left with no device
    /// follows the default instead.
    pub fn drop_own_targets(&mut self) {
        for devices in self.outputs.values_mut() {
            devices.retain(|device| !is_own_sink(device));
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

    fn devices(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn roundtrips_one_device_several_and_follow_default() {
        let mut o = ChannelOutputs::default();
        o.set("sink_game", devices(&["alsa_output.usb-Headset"]));
        o.set(
            "sink_music",
            devices(&["alsa_output.usb-Headset", "alsa_output.pci-Speakers"]),
        );
        o.set("sink_chat", Vec::new());
        let json = serde_json::to_string(&o).expect("serializes");
        let back: ChannelOutputs = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back, o);
        assert_eq!(back.get("sink_game"), ["alsa_output.usb-Headset"]);
        assert_eq!(
            back.get("sink_music"),
            ["alsa_output.usb-Headset", "alsa_output.pci-Speakers"]
        );
        assert!(back.get("sink_chat").is_empty());
        assert!(back.get("sink_never_set").is_empty());
    }

    /// An older Sink reads `"device"` or `null` per channel and rejects the
    /// whole file on anything else, so only a real multi-device channel may
    /// be written as a list.
    #[test]
    fn a_config_without_multi_output_keeps_the_old_format() {
        let mut o = ChannelOutputs::default();
        o.set("sink_game", devices(&["dev"]));
        o.set("sink_chat", Vec::new());
        let json = serde_json::to_value(&o).unwrap();
        assert_eq!(json["outputs"]["sink_game"], "dev");
        assert!(json["outputs"]["sink_chat"].is_null());

        o.set("sink_music", devices(&["a", "b"]));
        let json = serde_json::to_value(&o).unwrap();
        assert_eq!(json["outputs"]["sink_music"], serde_json::json!(["a", "b"]));
    }

    #[test]
    fn set_drops_blanks_and_repeats_keeping_order() {
        let mut o = ChannelOutputs::default();
        o.set("sink_game", devices(&["b", "", "a", "b"]));
        assert_eq!(o.get("sink_game"), ["b", "a"]);

        let hand_edited = r#"{"outputs":{"sink_game":["x","x",""]}}"#;
        let o: ChannelOutputs = serde_json::from_str(hand_edited).expect("loads");
        assert_eq!(o.get("sink_game"), ["x"]);
    }

    #[test]
    fn a_hand_edited_list_is_cut_to_what_a_channel_may_have() {
        let many: Vec<String> = (0..12).map(|i| format!("dev{i}")).collect();
        let raw = serde_json::json!({ "outputs": { "sink_game": many } }).to_string();
        let o: ChannelOutputs = serde_json::from_str(&raw).expect("loads");
        assert_eq!(o.get("sink_game").len(), MAX_CHANNEL_OUTPUTS);
        assert_eq!(o.get("sink_game")[0], "dev0");
        assert_eq!(o.get("sink_game")[7], "dev7");
    }

    /// The whole file used to be rejected, and the next save then wiped
    /// every channel's device.
    #[test]
    fn one_broken_entry_does_not_cost_the_other_channels_their_devices() {
        let raw = r#"{"outputs":{
            "sink_game": 42,
            "sink_chat": [["nested"]],
            "sink_system": ["ok", null],
            "sink_music": ["alsa_output.usb-Headset", "alsa_output.pci-Speakers"]
        },"no_failover":["sink_music"]}"#;
        let o: ChannelOutputs = serde_json::from_str(raw).expect("loads");
        assert!(o.get("sink_game").is_empty());
        assert!(o.get("sink_chat").is_empty());
        assert!(o.get("sink_system").is_empty());
        assert_eq!(
            o.get("sink_music"),
            ["alsa_output.usb-Headset", "alsa_output.pci-Speakers"]
        );
        assert!(!o.failover("sink_music"));
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
    fn config_from_before_multi_output_loads() {
        // One device or null per channel, and no `no_failover` key.
        let legacy = r#"{"outputs":{"sink_game":"dev","sink_music":null}}"#;
        let o: ChannelOutputs = serde_json::from_str(legacy).expect("legacy loads");
        assert!(o.failover("sink_game"), "missing field means failover on");
        assert_eq!(o.get("sink_game"), ["dev"]);
        assert!(o.get("sink_music").is_empty());
    }

    #[test]
    fn load_drops_outputs_on_our_own_nodes() {
        let _root = crate::persistence::testing::TempConfig::new("outputs-load-own");
        let mut o = ChannelOutputs::default();
        o.set("sink_game", devices(&["sink_chat"]));
        o.set(
            "sink_chat",
            devices(&["sink_bus_voice", "alsa_output.pci-Speakers"]),
        );
        o.set("sink_music", devices(&["alsa_output.usb-Headset"]));
        o.save().unwrap();

        let back = ChannelOutputs::load();
        assert!(back.get("sink_game").is_empty());
        assert_eq!(back.get("sink_chat"), ["alsa_output.pci-Speakers"]);
        assert_eq!(back.get("sink_music"), ["alsa_output.usb-Headset"]);
    }

    #[test]
    fn remove_drops_output_and_failover() {
        let mut o = ChannelOutputs::default();
        o.set("sink_game", devices(&["dev"]));
        o.set_failover("sink_game", false);
        o.remove("sink_game");
        assert!(o.get("sink_game").is_empty());
        assert!(o.failover("sink_game"));
    }
}
