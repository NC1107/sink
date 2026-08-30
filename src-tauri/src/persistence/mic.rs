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
    match fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
            eprintln!("sink: ignoring malformed {}: {e}", path.display());
            MicConfig::default()
        }),
        Err(_) => MicConfig::default(),
    }
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
