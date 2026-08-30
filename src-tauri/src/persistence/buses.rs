use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::SinkError;

/// Node-name prefix for user-created mixes (the seeded default keeps the
/// historical "sink_stream" name so existing OBS setups keep working).
pub const BUS_PREFIX: &str = "sink_bus_";
pub const DEFAULT_BUS_NODE: &str = "sink_stream";
pub const MAX_BUSES: usize = 4;

/// True if `name` is a mix bus node (not a channel, not a service node).
pub fn is_bus_name(name: &str) -> bool {
    name == DEFAULT_BUS_NODE || name.starts_with(BUS_PREFIX)
}

/// True if `name` is the always-on master mix: it carries every channel,
/// can't be deleted, and its membership is managed automatically.
pub fn is_master(name: &str) -> bool {
    name == DEFAULT_BUS_NODE
}

/// One user-defined mix: a capturable virtual source carrying the chosen
/// channels. The label is what recorders (OBS etc.) display.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BusDef {
    /// Stable node name, e.g. "sink_stream" or "sink_bus_voice_only".
    pub name: String,
    /// Display label - also the device description recorders see.
    pub label: String,
    /// Exclude mode (false): channels carried by this mix.
    /// Exclude mode (true): channels kept OUT of this mix.
    pub channels: Vec<String>,
    /// True = the mix carries every channel except `channels`, so new
    /// channels join automatically ("everything but music"). False = the
    /// mix carries exactly `channels` (manual selection).
    #[serde(default)]
    pub exclude: bool,
    /// Playback level recorders hear (0-150%). Persisted so a mix keeps its
    /// level across UI remounts, profile switches, and restarts.
    #[serde(default = "default_volume")]
    pub volume_percent: u8,
    /// Muted for recorders (they hear silence). Persisted like the volume.
    #[serde(default)]
    pub muted: bool,
    /// The processed virtual mic feeds this mix alongside its channels.
    #[serde(default)]
    pub mic: bool,
    /// Per-member send level (0-150%; absent = 100%), independent of the
    /// member's own volume. Keyed by sink name, or "sink_mic".
    #[serde(default)]
    pub member_gains: HashMap<String, u8>,
}

fn default_volume() -> u8 {
    100
}

impl BusDef {
    /// The channels this mix actually carries, given the full channel set.
    pub fn effective_members(&self, all_channels: &[String]) -> Vec<String> {
        if self.exclude {
            all_channels
                .iter()
                .filter(|c| !self.channels.contains(c))
                .cloned()
                .collect()
        } else {
            self.channels.clone()
        }
    }
}

/// The user's mixes, stored at `$XDG_CONFIG_HOME/sink/buses.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Buses {
    pub buses: Vec<BusDef>,
}

impl Default for Buses {
    fn default() -> Self {
        Self {
            buses: vec![BusDef {
                name: DEFAULT_BUS_NODE.to_string(),
                label: "Master Mix".to_string(),
                channels: Vec::new(),
                exclude: false,
                volume_percent: 100,
                muted: false,
                mic: false,
                member_gains: HashMap::new(),
            }],
        }
    }
}

fn slugify(label: &str) -> String {
    let slug: String = label
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if slug.is_empty() {
        "mix".to_string()
    } else {
        slug
    }
}

impl Buses {
    pub fn config_path() -> Result<PathBuf, SinkError> {
        let dir = crate::persistence::config_root()
            .ok_or_else(|| SinkError::Config("cannot resolve the user config directory".into()))?;
        Ok(dir.join("sink").join("buses.json"))
    }

    /// Load from disk. On first run (no file), the default Stream Mix bus
    /// inherits membership from the legacy per-channel `stream_mix` flags.
    pub fn load(legacy_channels: &crate::persistence::channels::Channels) -> Self {
        let path = match Self::config_path() {
            Ok(p) => p,
            Err(_) => return Self::default(),
        };
        match fs::read_to_string(&path) {
            Ok(raw) => {
                // A present-but-broken file is a torn or hand-edited write:
                // log it rather than resetting the user's mixes in silence.
                let mut buses = match serde_json::from_str::<Self>(&raw) {
                    Ok(buses) => buses,
                    Err(e) => {
                        eprintln!("sink: buses.json is unreadable ({e}); using defaults");
                        Self::default()
                    }
                };
                buses.clamp_loaded();
                buses
            }
            Err(_) => {
                let mut buses = Self::default();
                buses.buses[0].channels = legacy_channels
                    .channels
                    .iter()
                    .filter(|c| c.stream_mix)
                    .map(|c| c.name.clone())
                    .collect();
                buses
            }
        }
    }

    /// A hand-edited buses.json degrades to the documented ranges instead
    /// of riding through to the UI (the `EqConfig::clamp_ranges` rule).
    fn clamp_loaded(&mut self) {
        for bus in &mut self.buses {
            bus.volume_percent = bus.volume_percent.min(150);
            bus.member_gains.retain(|_, percent| {
                *percent = (*percent).min(150);
                *percent != 100
            });
        }
    }

    pub fn save(&self) -> Result<(), SinkError> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            crate::persistence::ensure_private_dir(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| SinkError::Config(format!("serialize buses: {e}")))?;
        super::write_atomic(&path, &json)?;
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&BusDef> {
        self.buses.iter().find(|b| b.name == name)
    }

    /// Ensure the master mix exists, sits first, and carries every channel.
    /// Called wherever the channel set changes (init, add, profile load).
    pub fn sync_master(&mut self, channels: &[String]) {
        let mut def = match self.buses.iter().position(|b| is_master(&b.name)) {
            Some(i) => self.buses.remove(i),
            None => BusDef {
                name: DEFAULT_BUS_NODE.to_string(),
                label: "Master Mix".to_string(),
                channels: Vec::new(),
                exclude: false,
                volume_percent: 100,
                muted: false,
                mic: false,
                member_gains: HashMap::new(),
            },
        };
        def.channels = channels.to_vec();
        def.exclude = false;
        self.buses.insert(0, def);
    }

    /// Switch a mix between manual and auto-include mode, preserving its
    /// current effective membership (the stored list flips meaning).
    pub fn set_exclude(
        &mut self,
        name: &str,
        exclude: bool,
        all_channels: &[String],
    ) -> Result<(), SinkError> {
        if is_master(name) {
            return Err(SinkError::Config(
                "the master mix always carries every channel".into(),
            ));
        }
        let def = self
            .buses
            .iter_mut()
            .find(|b| b.name == name)
            .ok_or_else(|| SinkError::UnknownSink(name.to_string()))?;
        if def.exclude == exclude {
            return Ok(());
        }
        // Preserve what the mix carries: exclude mode stores the
        // complement, manual mode stores the carried set itself.
        let effective = def.effective_members(all_channels);
        def.channels = if exclude {
            all_channels
                .iter()
                .filter(|c| !effective.contains(c))
                .cloned()
                .collect()
        } else {
            effective
        };
        def.exclude = exclude;
        Ok(())
    }

    pub fn add(&mut self, label: &str) -> Result<BusDef, SinkError> {
        let label = label.trim();
        if label.is_empty() || label.len() > 24 {
            return Err(SinkError::Config("mix label must be 1-24 characters".into()));
        }
        // The master mix doesn't count against the user's mixes.
        if self.buses.iter().filter(|b| !is_master(&b.name)).count() >= MAX_BUSES {
            return Err(SinkError::Config(format!(
                "at most {MAX_BUSES} mixes are supported"
            )));
        }
        let base = format!("{BUS_PREFIX}{}", slugify(label));
        let mut name = base.clone();
        let mut counter = 2;
        while self.get(&name).is_some() || name == DEFAULT_BUS_NODE {
            name = format!("{base}_{counter}");
            counter += 1;
        }
        // New mixes start in auto-include mode carrying everything -
        // uncheck what you don't want ("everything but music") and future
        // channels keep joining automatically.
        let def = BusDef {
            name,
            label: label.to_string(),
            channels: Vec::new(),
            exclude: true,
            volume_percent: 100,
            muted: false,
            mic: false,
            member_gains: HashMap::new(),
        };
        self.buses.push(def.clone());
        Ok(def)
    }

    pub fn rename(&mut self, name: &str, label: &str) -> Result<(), SinkError> {
        let label = label.trim();
        if label.is_empty() || label.len() > 24 {
            return Err(SinkError::Config("mix label must be 1-24 characters".into()));
        }
        let def = self
            .buses
            .iter_mut()
            .find(|b| b.name == name)
            .ok_or_else(|| SinkError::UnknownSink(name.to_string()))?;
        def.label = label.to_string();
        Ok(())
    }

    /// Whether `remove` would succeed, so callers can reject a bad name
    /// before tearing the node down.
    pub fn removable(&self, name: &str) -> Result<(), SinkError> {
        if is_master(name) {
            return Err(SinkError::Config("the master mix can't be deleted".into()));
        }
        if !self.buses.iter().any(|b| b.name == name) {
            return Err(SinkError::UnknownSink(name.to_string()));
        }
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<(), SinkError> {
        self.removable(name)?;
        self.buses.retain(|b| b.name != name);
        Ok(())
    }

    pub fn set_members(&mut self, name: &str, channels: Vec<String>) -> Result<(), SinkError> {
        if is_master(name) {
            return Err(SinkError::Config(
                "the master mix always carries every channel".into(),
            ));
        }
        let def = self
            .buses
            .iter_mut()
            .find(|b| b.name == name)
            .ok_or_else(|| SinkError::UnknownSink(name.to_string()))?;
        def.channels = channels;
        Ok(())
    }

    pub fn set_volume(&mut self, name: &str, volume: u8) -> Result<(), SinkError> {
        let def = self
            .buses
            .iter_mut()
            .find(|b| b.name == name)
            .ok_or_else(|| SinkError::UnknownSink(name.to_string()))?;
        def.volume_percent = volume;
        Ok(())
    }

    pub fn set_muted(&mut self, name: &str, muted: bool) -> Result<(), SinkError> {
        let def = self
            .buses
            .iter_mut()
            .find(|b| b.name == name)
            .ok_or_else(|| SinkError::UnknownSink(name.to_string()))?;
        def.muted = muted;
        Ok(())
    }

    /// Allowed on the master mix too - unlike channel membership, mic
    /// inclusion isn't auto-managed.
    pub fn set_mic(&mut self, name: &str, mic: bool) -> Result<(), SinkError> {
        let def = self
            .buses
            .iter_mut()
            .find(|b| b.name == name)
            .ok_or_else(|| SinkError::UnknownSink(name.to_string()))?;
        def.mic = mic;
        Ok(())
    }

    /// Unity (100) drops the entry, so a fader back at rest doesn't bloat
    /// the persisted file.
    pub fn set_member_gain(&mut self, name: &str, member: &str, percent: u8) -> Result<(), SinkError> {
        let def = self
            .buses
            .iter_mut()
            .find(|b| b.name == name)
            .ok_or_else(|| SinkError::UnknownSink(name.to_string()))?;
        if percent == 100 {
            def.member_gains.remove(member);
        } else {
            def.member_gains.insert(member.to_string(), percent);
        }
        Ok(())
    }

    /// Drop a deleted channel from every bus's membership and any per-mix
    /// send level it had.
    pub fn remove_channel(&mut self, channel: &str) {
        for bus in &mut self.buses {
            bus.channels.retain(|c| c != channel);
            bus.member_gains.remove(channel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_master_mix() {
        let b = Buses::default();
        assert_eq!(b.buses.len(), 1);
        assert_eq!(b.buses[0].name, "sink_stream");
        assert_eq!(b.buses[0].label, "Master Mix");
        assert!(is_master(&b.buses[0].name));
    }

    #[test]
    fn add_generates_prefixed_unique_names() {
        let mut b = Buses::default();
        let d = b.add("Voice Only").expect("adds");
        assert_eq!(d.name, "sink_bus_voice_only");
        let d2 = b.add("Voice Only").expect("adds dup label");
        assert_eq!(d2.name, "sink_bus_voice_only_2");
        assert!(is_bus_name(&d.name));
        assert!(is_bus_name("sink_stream"));
        assert!(!is_bus_name("sink_game"));
    }

    #[test]
    fn membership_and_channel_removal() {
        let mut b = Buses::default();
        let mix = b.add("Voice Only").expect("adds");
        b.set_members(&mix.name, vec!["sink_game".into(), "sink_chat".into()])
            .expect("sets");
        b.remove_channel("sink_chat");
        assert_eq!(b.get(&mix.name).expect("bus").channels, vec!["sink_game"]);
    }

    #[test]
    fn master_is_protected_and_auto_synced() {
        let mut b = Buses::default();
        assert!(b.remove("sink_stream").is_err());
        assert!(b.set_members("sink_stream", vec!["sink_game".into()]).is_err());
        // Renaming is allowed - recorders see the label.
        b.rename("sink_stream", "Everything").expect("renames");

        b.sync_master(&["sink_game".into(), "sink_chat".into()]);
        let master = b.get("sink_stream").expect("master");
        assert_eq!(master.label, "Everything");
        assert_eq!(master.channels, vec!["sink_game", "sink_chat"]);
        assert_eq!(b.buses[0].name, "sink_stream");

        // Recreated (with the default label) if it ever goes missing.
        b.buses.clear();
        b.sync_master(&["sink_game".into()]);
        assert_eq!(b.buses[0].label, "Master Mix");
        assert_eq!(b.buses[0].channels, vec!["sink_game"]);
    }

    #[test]
    fn removable_matches_remove_without_mutating() {
        let mut b = Buses::default();
        let name = b.add("Game Capture").expect("adds").name;

        // The master mix and unknown names are rejected by the probe, and
        // the probe leaves the set untouched either way.
        assert!(b.removable("sink_stream").is_err());
        assert!(b.removable("sink_bus_nope").is_err());
        assert!(b.removable(&name).is_ok());
        assert!(b.get(&name).is_some());

        b.remove(&name).expect("removes");
        assert!(b.removable(&name).is_err());
    }

    #[test]
    fn exclude_mode_carries_everything_but_the_unchecked() {
        let all = vec![
            "sink_game".to_string(),
            "sink_chat".to_string(),
            "sink_music".to_string(),
        ];
        let mut b = Buses::default();
        let mix = b.add("No Music").expect("adds");
        // New mixes auto-include: carry everything out of the box…
        assert!(mix.exclude);
        assert_eq!(b.get(&mix.name).expect("mix").effective_members(&all), all);
        // …and a new channel joins without touching the definition.
        let mut grown = all.clone();
        grown.push("sink_voice".to_string());
        assert_eq!(b.get(&mix.name).expect("mix").effective_members(&grown), grown);

        // Keep music out: only music is stored; everything else flows.
        b.set_members(&mix.name, vec!["sink_music".into()]).expect("sets");
        assert_eq!(
            b.get(&mix.name).expect("mix").effective_members(&grown),
            vec!["sink_game", "sink_chat", "sink_voice"]
        );
    }

    #[test]
    fn mode_switch_preserves_effective_membership() {
        let all = vec!["sink_game".to_string(), "sink_music".to_string()];
        let mut b = Buses::default();
        let mix = b.add("Mix").expect("adds"); // exclude, carries all
        b.set_members(&mix.name, vec!["sink_music".into()]).expect("excludes music");

        b.set_exclude(&mix.name, false, &all).expect("to manual");
        let def = b.get(&mix.name).expect("mix");
        assert!(!def.exclude);
        assert_eq!(def.channels, vec!["sink_game"]); // stored = carried now
        assert_eq!(def.effective_members(&all), vec!["sink_game"]);

        b.set_exclude(&mix.name, true, &all).expect("back to auto");
        let def = b.get(&mix.name).expect("mix");
        assert_eq!(def.effective_members(&all), vec!["sink_game"]);
        // Master can't leave auto-everything.
        assert!(b.set_exclude("sink_stream", true, &all).is_err());
    }

    #[test]
    fn sync_master_replaces_stale_membership() {
        let mut b = Buses::default();
        b.sync_master(&["sink_game".into(), "sink_chat".into()]);
        // A channel disappeared: sync must drop it, not merge.
        b.sync_master(&["sink_game".into()]);
        assert_eq!(b.get("sink_stream").expect("master").channels, vec!["sink_game"]);
        // No channels at all: the master mirrors that too.
        b.sync_master(&[]);
        assert!(b.get("sink_stream").expect("master").channels.is_empty());
    }

    #[test]
    fn volume_and_mute_persist_and_default() {
        let mut b = Buses::default();
        // Fresh mixes start at unity, unmuted.
        assert_eq!(b.buses[0].volume_percent, 100);
        assert!(!b.buses[0].muted);

        b.set_volume("sink_stream", 60).expect("sets volume");
        b.set_muted("sink_stream", true).expect("sets mute");
        assert_eq!(b.get("sink_stream").expect("master").volume_percent, 60);
        assert!(b.get("sink_stream").expect("master").muted);
        assert!(b.set_volume("sink_missing", 50).is_err());

        // Legacy buses.json written before these fields loads at the defaults.
        let legacy = r#"{"buses":[{"name":"sink_stream","label":"Master Mix","channels":[],"exclude":false}]}"#;
        let loaded: Buses = serde_json::from_str(legacy).expect("legacy loads");
        assert_eq!(loaded.buses[0].volume_percent, 100);
        assert!(!loaded.buses[0].muted);
    }

    #[test]
    fn mic_membership_persists_and_defaults_off() {
        let mut b = Buses::default();
        assert!(!b.buses[0].mic);
        b.set_mic("sink_stream", true).expect("sets mic on master");
        assert!(b.get("sink_stream").expect("master").mic);

        let mix = b.add("Voice Only").expect("adds");
        assert!(!b.get(&mix.name).expect("mix").mic);
        b.set_mic(&mix.name, true).expect("sets mic");
        assert!(b.get(&mix.name).expect("mix").mic);
        assert!(b.set_mic("sink_missing", true).is_err());

        // sync_master preserves the flag across membership resyncs.
        b.sync_master(&["sink_game".into()]);
        assert!(b.get("sink_stream").expect("master").mic);

        // Legacy buses.json written before this field loads at the default.
        let legacy = r#"{"buses":[{"name":"sink_stream","label":"Master Mix","channels":[],"exclude":false}]}"#;
        let loaded: Buses = serde_json::from_str(legacy).expect("legacy loads");
        assert!(!loaded.buses[0].mic);
    }

    #[test]
    fn member_gain_round_trips_and_unity_drops_the_entry() {
        let mut b = Buses::default();
        b.set_member_gain("sink_stream", "sink_game", 60).expect("sets gain");
        assert_eq!(b.get("sink_stream").expect("master").member_gains.get("sink_game"), Some(&60));

        // A mic send level, keyed the same way as a channel.
        b.set_member_gain("sink_stream", "sink_mic", 130).expect("sets mic gain");
        assert_eq!(b.get("sink_stream").expect("master").member_gains.get("sink_mic"), Some(&130));

        // Returning to unity (100) drops the entry rather than storing it.
        b.set_member_gain("sink_stream", "sink_game", 100).expect("resets gain");
        assert!(!b.get("sink_stream").expect("master").member_gains.contains_key("sink_game"));

        assert!(b.set_member_gain("sink_missing", "sink_game", 50).is_err());

        // Deleting the channel drops its per-mix send level too.
        b.remove_channel("sink_mic"); // exercises the mic key through the same path
        assert!(!b.get("sink_stream").expect("master").member_gains.contains_key("sink_mic"));

        // Legacy buses.json written before this field loads at the default.
        let legacy = r#"{"buses":[{"name":"sink_stream","label":"Master Mix","channels":[],"exclude":false}]}"#;
        let loaded: Buses = serde_json::from_str(legacy).expect("legacy loads");
        assert!(loaded.buses[0].member_gains.is_empty());
    }

    #[test]
    fn clamp_loaded_bounds_hand_edited_values() {
        // A hand-edited file with out-of-range numbers degrades to the
        // documented bounds instead of riding through to the UI.
        let raw = r#"{"buses":[{"name":"sink_stream","label":"Master Mix","channels":[],
            "exclude":false,"volume_percent":250,
            "member_gains":{"sink_game":255,"sink_chat":100,"sink_music":60}}]}"#;
        let mut b: Buses = serde_json::from_str(raw).expect("parses");
        b.clamp_loaded();
        assert_eq!(b.buses[0].volume_percent, 150);
        assert_eq!(b.buses[0].member_gains.get("sink_game"), Some(&150));
        // A stored unity entry is dropped, same as set_member_gain does.
        assert!(!b.buses[0].member_gains.contains_key("sink_chat"));
        assert_eq!(b.buses[0].member_gains.get("sink_music"), Some(&60));
    }

    #[test]
    fn master_does_not_count_toward_limit() {
        let mut b = Buses::default();
        for i in 0..MAX_BUSES {
            b.add(&format!("Mix {i}")).expect("adds user mix");
        }
        assert!(b.add("One Too Many").is_err());
        assert_eq!(b.buses.len(), MAX_BUSES + 1); // master + user mixes
    }
}
