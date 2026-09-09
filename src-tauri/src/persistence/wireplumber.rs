
//! Retires the conf fragment earlier versions rendered rules into: it could
//! only match stream props, and only acted before the enforcer's first pass.

use std::path::PathBuf;

use crate::error::SinkError;

pub fn conf_path() -> Result<PathBuf, SinkError> {
    let dir = crate::persistence::config_root()
        .ok_or_else(|| SinkError::Config("cannot resolve the user config directory".into()))?;
    Ok(dir
        .join("wireplumber")
        .join("wireplumber.conf.d")
        .join("90-sink-routing.conf"))
}

/// Remove a fragment left by an earlier version, if any.
pub fn remove_stale() {
    if let Ok(path) = conf_path() {
        if path.exists() {
            if let Err(e) = std::fs::remove_file(&path) {
                eprintln!("sink: removing stale {}: {e}", path.display());
            }
        }
    }
}
