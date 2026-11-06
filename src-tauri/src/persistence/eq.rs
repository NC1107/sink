use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::audio::types::EqConfig;
use crate::error::SinkError;

/// Per-channel parametric EQ configs, stored as JSON at
/// `$XDG_CONFIG_HOME/sink/eq.json`. Missing = "never touched" (disabled, flat).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ChannelEq {
    /// `serde(default)` keeps pre-EQ profile files loading cleanly.
    #[serde(default)]
    pub configs: HashMap<String, EqConfig>,
}

impl ChannelEq {
    pub fn config_path() -> Result<PathBuf, SinkError> {
        let dir = crate::persistence::config_root()
            .ok_or_else(|| SinkError::Config("cannot resolve the user config directory".into()))?;
        Ok(dir.join("sink").join("eq.json"))
    }

    pub fn load() -> Self {
        let Ok(path) = Self::config_path() else {
            return Self::default();
        };
        let mut eq: Self = match fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
                eprintln!("sink: ignoring malformed {}: {e}", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        };
        eq.clamp_ranges();
        eq
    }

    /// Sanitize every channel's config, as the editor does before it saves.
    pub fn clamp_ranges(&mut self) {
        for config in self.configs.values_mut() {
            config.clamp_ranges();
        }
    }

    pub fn save(&self) -> Result<(), SinkError> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            crate::persistence::ensure_private_dir(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| SinkError::Config(format!("serialize eq: {e}")))?;
        super::write_atomic(&path, &json)?;
        Ok(())
    }

    /// A channel's EQ, defaulting to disabled/flat when never configured.
    pub fn get(&self, sink_name: &str) -> EqConfig {
        self.configs.get(sink_name).cloned().unwrap_or_default()
    }

    pub fn set(&mut self, sink_name: &str, config: EqConfig) {
        self.configs.insert(sink_name.to_string(), config);
    }

    /// Drop all state for a removed channel.
    pub fn remove(&mut self, sink_name: &str) {
        self.configs.remove(sink_name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::types::{default_eq_bands, EqBandKind};

    /// A hand-edited or downgraded file reaches the RT biquads straight from
    /// here at startup; -500 dB silenced the whole channel, not just a band.
    #[test]
    fn load_clamps_out_of_range_values() {
        let _root = crate::persistence::testing::TempConfig::new("eq-load-clamp");
        let path = ChannelEq::config_path().unwrap();
        crate::persistence::ensure_private_dir(path.parent().unwrap()).unwrap();
        let band = r#"{"kind":"peaking","freq_hz":99999,"gain_db":-500,"q":1}"#;
        let bands = vec![band; 30].join(",");
        let raw = format!(
            r#"{{"configs":{{"sink_game":{{"enabled":true,"preamp_db":900,"bands":[{bands}]}}}}}}"#
        );
        std::fs::write(&path, raw).unwrap();

        let config = ChannelEq::load().get("sink_game");
        assert_eq!(config.preamp_db, 24.0);
        assert_eq!(config.bands.len(), crate::audio::types::MAX_EQ_BANDS);
        assert!(config
            .bands
            .iter()
            .all(|b| b.gain_db == -24.0 && b.freq_hz == 20000.0));
    }

    #[test]
    fn roundtrips_configured_channels() {
        let mut eq = ChannelEq::default();
        let mut config = EqConfig {
            enabled: true,
            preamp_db: -3.0,
            ..EqConfig::default()
        };
        config.bands[0].gain_db = 4.5;
        eq.set("sink_game", config.clone());
        let json = serde_json::to_string(&eq).expect("serializes");
        let back: ChannelEq = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back, eq);
        assert_eq!(back.get("sink_game"), config);
    }

    #[test]
    fn unconfigured_channel_gets_default() {
        let eq = ChannelEq::default();
        let config = eq.get("sink_chat");
        assert!(!config.enabled);
        assert_eq!(config.bands, default_eq_bands());
        assert_eq!(config.bands[0].kind, EqBandKind::LowShelf);
    }

    #[test]
    fn legacy_file_without_configs_field_loads() {
        // A pre-EQ profile (or an empty file body) has no `configs` key.
        let eq: ChannelEq = serde_json::from_str("{}").expect("legacy loads");
        assert_eq!(eq, ChannelEq::default());
    }

    #[test]
    fn remove_drops_config() {
        let mut eq = ChannelEq::default();
        eq.set("sink_game", EqConfig::default());
        eq.remove("sink_game");
        assert!(eq.configs.is_empty());
    }
}
