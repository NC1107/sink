//! Hotkey settings: the balance step, and key bindings for the X11 fallback.
//! Under the desktop portal the bindings live with the desktop, not here.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::SinkError;

/// Balance moves per press, in percentage points; the UI offers these.
pub const BALANCE_STEPS: [u8; 4] = [1, 10, 15, 25];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyConfig {
    #[serde(default = "default_step")]
    pub balance_step: u8,
    /// Action id -> accelerator, X11 only.
    #[serde(default)]
    pub bindings: BTreeMap<String, String>,
}

fn default_step() -> u8 {
    10
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            balance_step: default_step(),
            bindings: BTreeMap::new(),
        }
    }
}

impl HotkeyConfig {
    pub fn config_path() -> Result<PathBuf, SinkError> {
        let dir = super::config_root()
            .ok_or_else(|| SinkError::Config("cannot resolve the user config directory".into()))?;
        Ok(dir.join("sink").join("hotkeys.json"))
    }

    pub fn load() -> Self {
        let mut config: Self = Self::config_path()
            .ok()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        if !BALANCE_STEPS.contains(&config.balance_step) {
            config.balance_step = default_step();
        }
        config
    }

    pub fn save(&self) -> Result<(), SinkError> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            super::ensure_private_dir(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| SinkError::Config(format!("serialize hotkeys: {e}")))?;
        super::write_atomic(&path, &json)?;
        Ok(())
    }
}
