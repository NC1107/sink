use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::SinkError;

/// Sink node names reserved by Sink itself (not user channels).
pub const RESERVED_SINK_NAMES: [&str; 2] = ["sink_mic", "sink_stream"];
/// Upper bound on user channels (level-meter slots are budgeted for this).
pub const MAX_CHANNELS: usize = 10;

/// One user-defined mixer channel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChannelDef {
    /// PipeWire sink node name, e.g. "sink_game". Stable once created.
    pub name: String,
    /// Display label, e.g. "Game". Renameable.
    pub label: String,
    /// Material Symbol name for the strip icon (None = legacy default).
    #[serde(default)]
    pub icon: Option<String>,
    /// Whether the channel feeds the Stream Mix source (default: yes).
    #[serde(default = "default_true")]
    pub stream_mix: bool,
    /// Fader position (0-150%). Persisted so a channel keeps its level
    /// across restarts even when no profile is active.
    #[serde(default = "default_volume")]
    pub volume_percent: u8,
    /// Muted state, persisted alongside the volume.
    #[serde(default)]
    pub muted: bool,
}

fn default_true() -> bool {
    true
}

fn default_volume() -> u8 {
    100
}

/// The user's channel set, stored as JSON at
/// `$XDG_CONFIG_HOME/sink/channels.json`. Defaults to the classic four.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Channels {
    pub channels: Vec<ChannelDef>,
}

impl Default for Channels {
    fn default() -> Self {
        let def = |name: &str, label: &str, icon: &str| ChannelDef {
            name: name.to_string(),
            label: label.to_string(),
            icon: Some(icon.to_string()),
            stream_mix: true,
            volume_percent: default_volume(),
            muted: false,
        };
        Self {
            channels: vec![
                def("sink_game", "Game", "sports_esports"),
                def("sink_chat", "Chat", "forum"),
                def("sink_music", "Music", "music_note"),
                def("sink_system", "System", "desktop_windows"),
            ],
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
        "channel".to_string()
    } else {
        slug
    }
}

impl Channels {
    pub fn config_path() -> Result<PathBuf, SinkError> {
        let dir = crate::persistence::config_root()
            .ok_or_else(|| SinkError::Config("cannot resolve the user config directory".into()))?;
        Ok(dir.join("sink").join("channels.json"))
    }

    pub fn load() -> Self {
        let Ok(path) = Self::config_path() else {
            return Self::default();
        };
        // A missing file is first run (silent default); a present-but-broken
        // one is a torn or hand-edited write we log rather than honour.
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(_) => return Self::default(),
        };
        match Self::parse(&raw) {
            Some(c) if !c.channels.is_empty() => c,
            Some(_) => {
                eprintln!("sink: channels.json held no valid channels; using defaults");
                Self::default()
            }
            None => {
                eprintln!("sink: channels.json is unreadable (corrupt?); using defaults");
                Self::default()
            }
        }
    }

    /// Parse and sanitize the channel set from JSON. Entries that break the
    /// invariants `add()` guarantees - a reserved name, a missing `sink_`
    /// prefix, or a duplicate - are dropped: a hand-edited or foreign-tool
    /// file would otherwise collide with the mic/stream service nodes, or make
    /// `is_virtual_sink` reject a channel and abort `init_virtual_devices`.
    /// The set is capped at `MAX_CHANNELS` so it can't exhaust the level-meter
    /// slots. Returns `None` only when the text isn't valid JSON, so `load`
    /// can tell a corrupt file from a merely empty one.
    fn parse(raw: &str) -> Option<Self> {
        let parsed: Self = serde_json::from_str(raw).ok()?;
        let mut seen = std::collections::HashSet::new();
        let mut channels = Vec::new();
        for def in parsed.channels {
            let name = def.name.as_str();
            let valid = name.starts_with("sink_")
                && !RESERVED_SINK_NAMES.contains(&name)
                && !crate::persistence::buses::is_bus_name(name)
                && seen.insert(def.name.clone());
            if valid && channels.len() < MAX_CHANNELS {
                channels.push(def);
            } else {
                eprintln!(
                    "sink: dropping invalid channel '{}' from channels.json",
                    def.name
                );
            }
        }
        Some(Self { channels })
    }

    pub fn save(&self) -> Result<(), SinkError> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            crate::persistence::ensure_private_dir(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| SinkError::Config(format!("serialize channels: {e}")))?;
        super::write_atomic(&path, &json)?;
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&ChannelDef> {
        self.channels.iter().find(|c| c.name == name)
    }

    pub fn set_icon(&mut self, name: &str, icon: Option<String>) -> Result<(), SinkError> {
        let def = self
            .channels
            .iter_mut()
            .find(|c| c.name == name)
            .ok_or_else(|| SinkError::UnknownSink(name.to_string()))?;
        def.icon = icon;
        Ok(())
    }

    /// Record a channel's fader position so it survives a restart. Unknown
    /// channels are ignored rather than erroring: this runs on every fader
    /// tick, and a channel deleted mid-drag shouldn't surface an error.
    pub fn set_volume(&mut self, name: &str, volume_percent: u8) {
        if let Some(def) = self.channels.iter_mut().find(|c| c.name == name) {
            def.volume_percent = volume_percent;
        }
    }

    /// Record a channel's mute state (see `set_volume` for the lenient
    /// unknown-channel handling).
    pub fn set_muted(&mut self, name: &str, muted: bool) {
        if let Some(def) = self.channels.iter_mut().find(|c| c.name == name) {
            def.muted = muted;
        }
    }

    /// Add a channel for `label`, generating a unique reserved-safe sink
    /// name. Returns the new definition.
    pub fn add(&mut self, label: &str, icon: Option<String>) -> Result<ChannelDef, SinkError> {
        let label = label.trim();
        if label.is_empty() || label.len() > 24 {
            return Err(SinkError::Config("channel label must be 1-24 characters".into()));
        }
        if self.channels.len() >= MAX_CHANNELS {
            return Err(SinkError::Config(format!(
                "at most {MAX_CHANNELS} channels are supported"
            )));
        }
        let mut base = format!("sink_{}", slugify(label));
        if crate::persistence::buses::is_bus_name(&base) {
            // A label like "Bus Foo" would slug straight into the mix-bus
            // namespace, where every suffixed variant still matches the
            // prefix; step out of it instead of trying to disambiguate.
            base = base.replacen("sink_bus_", "sink_ch_bus_", 1);
        }
        let mut name = base.clone();
        let mut counter = 2;
        while self.get(&name).is_some() || RESERVED_SINK_NAMES.contains(&name.as_str()) {
            name = format!("{base}_{counter}");
            counter += 1;
        }
        let def = ChannelDef {
            name,
            label: label.to_string(),
            icon,
            stream_mix: true,
            volume_percent: default_volume(),
            muted: false,
        };
        self.channels.push(def.clone());
        Ok(def)
    }

    pub fn rename(&mut self, name: &str, label: &str) -> Result<(), SinkError> {
        let label = label.trim();
        if label.is_empty() || label.len() > 24 {
            return Err(SinkError::Config("channel label must be 1-24 characters".into()));
        }
        let def = self
            .channels
            .iter_mut()
            .find(|c| c.name == name)
            .ok_or_else(|| SinkError::UnknownSink(name.to_string()))?;
        def.label = label.to_string();
        Ok(())
    }

    /// Reorder the channel set. `order` must contain exactly the current
    /// sink names (it's a permutation, not an edit).
    pub fn reorder(&mut self, order: &[String]) -> Result<(), SinkError> {
        if order.len() != self.channels.len()
            || !order.iter().all(|n| self.get(n).is_some())
        {
            return Err(SinkError::Config(
                "reorder must list every existing channel exactly once".into(),
            ));
        }
        self.channels.sort_by_key(|c| {
            order.iter().position(|n| n == &c.name).unwrap_or(usize::MAX)
        });
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<(), SinkError> {
        if self.channels.len() <= 1 {
            return Err(SinkError::Config("at least one channel is required".into()));
        }
        let before = self.channels.len();
        self.channels.retain(|c| c.name != name);
        if self.channels.len() == before {
            return Err(SinkError::UnknownSink(name.to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_classic_four() {
        let c = Channels::default();
        assert_eq!(c.channels.len(), 4);
        assert_eq!(c.channels[0].name, "sink_game");
    }

    #[test]
    fn add_generates_unique_safe_names() {
        let mut c = Channels::default();
        let d = c.add("Voice Chat!", Some("mic".into())).expect("adds");
        assert_eq!(d.name, "sink_voice_chat");
        assert_eq!(d.icon.as_deref(), Some("mic"));
        let d2 = c.add("Voice Chat", None).expect("adds duplicate label");
        assert_eq!(d2.name, "sink_voice_chat_2");
        // Reserved collision: label "mic" must not produce sink_mic.
        let d3 = c.add("Mic", None).expect("adds");
        assert_eq!(d3.name, "sink_mic_2");
    }

    #[test]
    fn pathological_labels_hit_the_slug_fallback() {
        let mut c = Channels::default();
        // All-special-char labels slugify to empty → "channel" fallback.
        let d = c.add("!!!", None).expect("adds");
        assert_eq!(d.name, "sink_channel");
        let d2 = c.add("___", None).expect("adds second pathological label");
        assert_eq!(d2.name, "sink_channel_2");
        // Whitespace-only labels are rejected outright.
        assert!(c.add("   ", None).is_err());
    }

    #[test]
    fn bus_namespace_labels_stay_out_of_the_bus_prefix() {
        let mut c = Channels::default();
        let d = c.add("Bus Foo", None).expect("adds");
        assert_eq!(d.name, "sink_ch_bus_foo");
        assert!(!crate::persistence::buses::is_bus_name(&d.name));
        assert!(crate::audio::types::is_virtual_sink(&d.name));

        // A hand-edited channels.json can't smuggle one in either.
        let raw = r#"{"channels":[
            {"name":"sink_bus_evil","label":"Evil"},
            {"name":"sink_game","label":"Game"}
        ]}"#;
        let parsed = Channels::parse(raw).expect("parses");
        assert_eq!(parsed.channels.len(), 1);
        assert_eq!(parsed.channels[0].name, "sink_game");
    }

    #[test]
    fn reorder_is_a_strict_permutation() {
        let mut c = Channels::default();
        c.reorder(&[
            "sink_music".into(),
            "sink_game".into(),
            "sink_system".into(),
            "sink_chat".into(),
        ])
        .expect("reorders");
        let names: Vec<&str> = c.channels.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["sink_music", "sink_game", "sink_system", "sink_chat"]);
        // Wrong length and unknown names are rejected.
        assert!(c.reorder(&["sink_game".into()]).is_err());
        assert!(c
            .reorder(&[
                "sink_music".into(),
                "sink_game".into(),
                "sink_system".into(),
                "sink_nope".into(),
            ])
            .is_err());
    }

    #[test]
    fn remove_keeps_at_least_one() {
        let mut c = Channels::default();
        c.remove("sink_game").expect("removes");
        c.remove("sink_chat").expect("removes");
        c.remove("sink_music").expect("removes");
        assert!(c.remove("sink_system").is_err(), "last channel must stay");
    }

    #[test]
    fn rename_updates_label_only() {
        let mut c = Channels::default();
        c.rename("sink_game", "Gaems").expect("renames");
        assert_eq!(c.get("sink_game").expect("exists").label, "Gaems");
        assert!(c.rename("sink_nope", "X").is_err());
    }

    #[test]
    fn parse_keeps_valid_and_fills_serde_defaults() {
        // Old-shape entries (pre-Phase-4: no icon / stream_mix) must still
        // load, with the serde defaults applied - an upgrade keeps user data.
        let raw = r#"{"channels":[
            {"name":"sink_game","label":"Game"},
            {"name":"sink_music","label":"Music","icon":"music_note","stream_mix":false}
        ]}"#;
        let c = Channels::parse(raw).expect("valid json");
        assert_eq!(c.channels.len(), 2);
        assert_eq!(c.channels[0].icon, None);
        assert!(c.channels[0].stream_mix, "missing stream_mix defaults true");
        assert!(!c.channels[1].stream_mix);
    }

    #[test]
    fn volume_and_mute_persist_and_default_to_unity() {
        let mut c = Channels::default();
        assert_eq!(c.channels[0].volume_percent, 100);
        assert!(!c.channels[0].muted);

        c.set_volume("sink_game", 42);
        c.set_muted("sink_game", true);
        assert_eq!(c.get("sink_game").expect("channel").volume_percent, 42);
        assert!(c.get("sink_game").expect("channel").muted);
        // Other channels are untouched.
        assert_eq!(c.get("sink_music").expect("channel").volume_percent, 100);
        // A vanished channel is a no-op, not a panic (fader tick mid-delete).
        c.set_volume("sink_gone", 10);
        c.set_muted("sink_gone", true);

        // A round trip through JSON keeps the levels…
        let raw = serde_json::to_string(&c).expect("serializes");
        let back = Channels::parse(&raw).expect("reparses");
        assert_eq!(back.get("sink_game").expect("channel").volume_percent, 42);
        assert!(back.get("sink_game").expect("channel").muted);

        // …and channels.json written before these fields loads at unity.
        let legacy = r#"{"channels":[{"name":"sink_game","label":"Game"}]}"#;
        let old = Channels::parse(legacy).expect("legacy loads");
        assert_eq!(old.channels[0].volume_percent, 100);
        assert!(!old.channels[0].muted);
    }

    #[test]
    fn parse_drops_reserved_unprefixed_and_duplicate_names() {
        let raw = r#"{"channels":[
            {"name":"sink_game","label":"Game"},
            {"name":"sink_game","label":"Dup"},
            {"name":"sink_mic","label":"Reserved"},
            {"name":"nope","label":"NoPrefix"},
            {"name":"sink_ok","label":"Fine"}
        ]}"#;
        let names: Vec<String> = Channels::parse(raw)
            .expect("valid json")
            .channels
            .into_iter()
            .map(|d| d.name)
            .collect();
        assert_eq!(names, ["sink_game", "sink_ok"]);
    }

    #[test]
    fn parse_caps_at_max_channels() {
        let mut items = Vec::new();
        for i in 0..(MAX_CHANNELS + 5) {
            items.push(format!(r#"{{"name":"sink_c{i}","label":"C{i}"}}"#));
        }
        let raw = format!(r#"{{"channels":[{}]}}"#, items.join(","));
        assert_eq!(
            Channels::parse(&raw).expect("valid json").channels.len(),
            MAX_CHANNELS
        );
    }

    #[test]
    fn parse_rejects_corrupt_or_empty_text() {
        assert!(Channels::parse("{ truncated").is_none());
        assert!(Channels::parse("").is_none());
    }

    #[test]
    fn parse_all_invalid_yields_empty_set() {
        // load() turns this into defaults; parse itself reports the empty set
        // so load can distinguish it from a corrupt (None) file.
        let raw = r#"{"channels":[{"name":"sink_mic","label":"x"},{"name":"bad","label":"y"}]}"#;
        assert!(Channels::parse(raw).expect("valid json").channels.is_empty());
    }
}
