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
            }),
        }
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

    #[test]
    fn serde_roundtrip() {
        let mut a = Assignments::default();
        a.set("node.name", "audio-src", "sink_system");
        let json = serde_json::to_string(&a).expect("serializes");
        let back: Assignments = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back.assignments, a.assignments);
    }
}
