pub mod active;
pub mod aliases;
pub mod assignments;
pub mod autostart;
pub mod buses;
pub mod channels;
pub mod eq;
pub mod mic;
pub mod outputs;
pub mod prefs;
pub mod seen;
pub mod profiles;
pub mod wireplumber;

/// Create Sink's config directory (and parents) with owner-only access —
/// routing rules and app history are nobody else's business. Used by every
/// save path that writes under `$XDG_CONFIG_HOME/sink`.
pub fn ensure_private_dir(path: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Write `contents` to `path` atomically: write a sibling temp file, fsync it,
/// then rename it over the target. A crash or power loss mid-write then leaves
/// either the old file or the complete new one — never a truncated file that
/// load paths silently discard (resetting the user's config). The parent
/// directory is created if missing; callers needing 0700 call
/// [`ensure_private_dir`] first, which this preserves.
pub fn write_atomic(path: &std::path::Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Temp file in the same directory so the rename stays on one filesystem
    // (a cross-device rename is not atomic).
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    let result = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(contents.as_ref())?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Factory reset: delete everything Sink ever saved — the whole config
/// directory (channels, mixes, profiles, assignments, history, prefs)
/// and the WirePlumber routing rules.
pub fn wipe_all() -> Result<(), crate::error::SinkError> {
    if let Some(dir) = dirs::config_dir() {
        let sink_dir = dir.join("sink");
        if sink_dir.exists() {
            std::fs::remove_dir_all(&sink_dir)?;
        }
    }
    if let Ok(conf) = wireplumber::conf_path() {
        if conf.exists() {
            std::fs::remove_file(&conf)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_overwrites_and_leaves_no_temp() {
        let dir = std::env::temp_dir().join(format!(
            "sink-write-atomic-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let path = dir.join("cfg.json");

        write_atomic(&path, b"first").expect("first write");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first");

        // A shorter follow-up must fully replace, not overlay, the old bytes.
        write_atomic(&path, b"second, longer contents").expect("overwrite");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second, longer contents");

        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        assert!(!std::path::Path::new(&tmp).exists(), "temp file must not linger");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
