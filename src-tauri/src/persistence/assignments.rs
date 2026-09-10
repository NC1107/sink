use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::SinkError;

/// One persistent routing assignment: streams whose PipeWire property
/// `match_prop` equals `match_value` belong on `sink_name`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Assignment {
    /// Property to match, e.g. "application.name".
    pub match_prop: String,
    /// Property value, e.g. "spotify".
    pub match_value: String,
    /// Target virtual sink, e.g. "sink_music".
    pub sink_name: String,
    /// Identities this rule was adopted into, once each, so unassigning
    /// the adopted rule isn't undone by the next refresh.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adopted_by: Vec<String>,
}

/// Unambiguous because prop names never contain ':'.
pub fn identity_key(prop: &str, value: &str) -> String {
    format!("{prop}:{value}")
}

/// The set of saved app→channel assignments, stored as JSON at
/// `$XDG_CONFIG_HOME/sink/assignments.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Assignments {
    pub assignments: Vec<Assignment>,
}

impl Assignments {
    pub fn config_path() -> Result<PathBuf, SinkError> {
        let dir = crate::persistence::config_root()
            .ok_or_else(|| SinkError::Config("cannot resolve the user config directory".into()))?;
        Ok(dir.join("sink").join("assignments.json"))
    }

    /// Load from disk; a missing or unreadable file yields the empty set
    /// (first run, or the user deleted their config).
    pub fn load() -> Self {
        let Ok(path) = Self::config_path() else {
            return Self::default();
        };
        match fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
                eprintln!("sink: ignoring malformed {}: {e}", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<(), SinkError> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            crate::persistence::ensure_private_dir(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| SinkError::Config(format!("serialize assignments: {e}")))?;
        super::write_atomic(&path, &json)?;
        Ok(())
    }

    /// Insert or update the assignment for (`match_prop`, `match_value`).
    pub fn set(&mut self, match_prop: &str, match_value: &str, sink_name: &str) {
        match self
            .assignments
            .iter_mut()
            .find(|a| a.match_prop == match_prop && a.match_value == match_value)
        {
            Some(existing) => existing.sink_name = sink_name.to_string(),
            None => self.assignments.push(Assignment {
                match_prop: match_prop.to_string(),
                match_value: match_value.to_string(),
                sink_name: sink_name.to_string(),
                adopted_by: Vec::new(),
            }),
        }
    }

    /// Adopt the legacy rules an app's stream matches (`matchers`, most
    /// specific first) into its process identity; `Some(sink)` when a rule
    /// was created. Every matching rule is marked in that same pass, or the
    /// second one would bring a removed rule back. Nothing is marked while
    /// the identity carries a rule of its own, so a legacy rule still
    /// applies once the user clears that one.
    pub fn adopt_all(
        &mut self,
        matchers: &[(String, String)],
        into_prop: &str,
        into_value: &str,
    ) -> Option<String> {
        let key = identity_key(into_prop, into_value);
        let matching: Vec<usize> = matchers
            .iter()
            .filter_map(|(prop, value)| {
                self.assignments
                    .iter()
                    .position(|a| a.match_prop == *prop && a.match_value == *value)
            })
            .collect();
        if matching
            .iter()
            .all(|&i| self.assignments[i].adopted_by.contains(&key))
        {
            return None;
        }
        if self.sink_for(into_prop, into_value).is_some() {
            return None;
        }
        let sink = self.assignments[matching[0]].sink_name.clone();
        for &i in &matching {
            if !self.assignments[i].adopted_by.contains(&key) {
                self.assignments[i].adopted_by.push(key.clone());
            }
        }
        self.set(into_prop, into_value, &sink);
        Some(sink)
    }

    pub fn remove(&mut self, match_prop: &str, match_value: &str) {
        self.assignments
            .retain(|a| !(a.match_prop == match_prop && a.match_value == match_value));
    }

    pub fn sink_for(&self, match_prop: &str, match_value: &str) -> Option<&str> {
        self.assignments
            .iter()
            .find(|a| a.match_prop == match_prop && a.match_value == match_value)
            .map(|a| a.sink_name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_upserts_and_remove_deletes() {
        let mut a = Assignments::default();
        a.set("application.name", "spotify", "sink_music");
        a.set("application.name", "spotify", "sink_game");
        assert_eq!(a.assignments.len(), 1);
        assert_eq!(a.sink_for("application.name", "spotify"), Some("sink_game"));

        a.remove("application.name", "spotify");
        assert!(a.sink_for("application.name", "spotify").is_none());
        assert!(a.assignments.is_empty());
    }

    fn matchers(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(p, v)| (p.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn adoption_happens_once_per_identity() {
        let mut a = Assignments::default();
        a.set("application.name", "SDL Application", "sink_game");
        let sdl = matchers(&[("application.name", "SDL Application")]);
        assert_eq!(
            a.adopt_all(&sdl, "steam.app_id", "1"),
            Some("sink_game".to_string())
        );
        // A second game on the same engine adopts too; the legacy rule stays.
        assert!(a.adopt_all(&sdl, "steam.app_id", "2").is_some());
        assert_eq!(
            a.sink_for("application.name", "SDL Application"),
            Some("sink_game")
        );
        // The user unassigns game 1: it must not come back.
        a.remove("steam.app_id", "1");
        assert!(a.adopt_all(&sdl, "steam.app_id", "1").is_none());
        assert!(a.sink_for("steam.app_id", "1").is_none());
    }

    #[test]
    fn every_matching_legacy_rule_is_marked_in_one_pass() {
        // The most specific matcher wins; the other is marked without
        // overriding the sink, so it can't resurrect the rule either.
        let mut a = Assignments::default();
        a.set("application.name", "Rocket League", "sink_game");
        a.set(
            "application.process.binary",
            "RocketLeague.exe",
            "sink_music",
        );
        let both = matchers(&[
            ("application.name", "Rocket League"),
            ("application.process.binary", "RocketLeague.exe"),
        ]);
        assert_eq!(
            a.adopt_all(&both, "steam.app_id", "9"),
            Some("sink_game".into())
        );
        assert_eq!(a.sink_for("steam.app_id", "9"), Some("sink_game"));
        a.remove("steam.app_id", "9");
        assert!(a.adopt_all(&both, "steam.app_id", "9").is_none());
        assert!(a.sink_for("steam.app_id", "9").is_none());
    }

    #[test]
    fn a_rule_blocked_by_the_identitys_own_rule_still_applies_later() {
        let mut a = Assignments::default();
        a.set("application.name", "Firefox", "sink_game");
        a.set("process.exe", "firefox", "sink_music");
        let legacy = matchers(&[("application.name", "Firefox")]);
        // The user's own rule for the identity wins and nothing is marked.
        assert!(a.adopt_all(&legacy, "process.exe", "firefox").is_none());
        assert_eq!(a.sink_for("process.exe", "firefox"), Some("sink_music"));
        // Once that rule is gone the legacy one carries the app again.
        a.remove("process.exe", "firefox");
        assert_eq!(
            a.adopt_all(&legacy, "process.exe", "firefox"),
            Some("sink_game".into())
        );
    }

    #[test]
    fn nothing_to_adopt_is_a_no_op() {
        let mut a = Assignments::default();
        assert!(a
            .adopt_all(&matchers(&[("application.name", "x")]), "process.exe", "x")
            .is_none());
        assert!(a.assignments.is_empty());
    }

    #[test]
    fn serde_roundtrip() {
        let mut a = Assignments::default();
        a.set("node.name", "audio-src", "sink_system");
        let json = serde_json::to_string(&a).expect("serializes");
        let back: Assignments = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back.assignments, a.assignments);
    }
}
