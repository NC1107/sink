use std::fs;
use std::path::PathBuf;

use crate::audio::types::MicConfig;
use crate::error::SinkError;

/// Mic chain configuration, stored as JSON at
/// `$XDG_CONFIG_HOME/sink/mic.json`.
pub fn config_path() -> Result<PathBuf, SinkError> {
    let dir = crate::persistence::config_root()
        .ok_or_else(|| SinkError::Config("cannot resolve the user config directory".into()))?;
    Ok(dir.join("sink").join("mic.json"))
}

pub fn load() -> MicConfig {
    let Ok(path) = config_path() else {
        return MicConfig::default();
    };
    let mut config: MicConfig = match fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
            eprintln!("sink: ignoring malformed {}: {e}", path.display());
            MicConfig::default()
        }),
        Err(_) => MicConfig::default(),
    };
    config.clamp_ranges();
    config.drop_own_input();
    config
}

pub fn save(config: &MicConfig) -> Result<(), SinkError> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        crate::persistence::ensure_private_dir(parent)?;
    }
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| SinkError::Config(format!("serialize mic config: {e}")))?;
    super::write_atomic(&path, &json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_clamps_out_of_range_values() {
        let _root = crate::persistence::testing::TempConfig::new("mic-load-clamp");
        let path = config_path().unwrap();
        crate::persistence::ensure_private_dir(path.parent().unwrap()).unwrap();
        let mut raw = serde_json::to_value(MicConfig::default()).unwrap();
        raw["gain_percent"] = 255.into();
        raw["limiter_ceiling_db"] = 40.0.into();
        raw["eq"]["bands"][0]["gain_db"] = 500.0.into();
        std::fs::write(&path, raw.to_string()).unwrap();

        let config = load();
        assert_eq!(config.gain_percent, 200);
        assert_eq!(config.limiter_ceiling_db, 0.0);
        assert_eq!(config.eq.bands[0].gain_db, 24.0);
    }

    /// Every layer refuses an own-node pin, so loading one back would show
    /// the mic on "sink_stream" while it captures nothing.
    #[test]
    fn load_drops_a_pin_on_our_own_nodes() {
        let _root = crate::persistence::testing::TempConfig::new("mic-load-own");
        for (pin, kept) in [
            ("sink_stream", None),
            ("sink_game.monitor", None),
            ("alsa_input.usb-mic", Some("alsa_input.usb-mic")),
        ] {
            let config = MicConfig {
                input_device: Some(pin.to_string()),
                ..MicConfig::default()
            };
            save(&config).unwrap();
            assert_eq!(load().input_device.as_deref(), kept, "pin {pin}");
        }
    }
}
