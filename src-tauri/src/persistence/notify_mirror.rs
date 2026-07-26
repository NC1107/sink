//! On/off marker for mirroring desktop notifications to the headset OLED.
//! Presence of the marker file is the persisted state; the actual dbus-monitor
//! thread lives in the headset manager.

use std::fs;
use std::path::PathBuf;

use crate::error::SinkError;

fn marker_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("sink").join("notify_mirror"))
}

pub fn is_enabled() -> bool {
    marker_path().map(|p| p.exists()).unwrap_or(false)
}

pub fn set(enabled: bool) -> Result<(), SinkError> {
    let Some(path) = marker_path() else {
        return Ok(());
    };
    if enabled {
        if let Some(parent) = path.parent() {
            super::ensure_private_dir(parent)?;
        }
        super::write_atomic(&path, b"1")?;
    } else if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}
