//! App id -> game name from Steam's appmanifest files: the store name,
//! with no version in it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::audio::identity::SteamDb;

/// A miss rescans, throttled so a non-Steam stream can't walk the library
/// every refresh.
const RESCAN_AFTER: Duration = Duration::from_secs(60);

struct Library {
    names: HashMap<String, String>,
    scanned_at: Instant,
}

static LIBRARY: OnceLock<Mutex<Library>> = OnceLock::new();

fn steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".local/share/Steam"));
        roots.push(home.join(".steam/steam"));
        roots.push(home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"));
    }
    roots
}

/// The quoted value following a quoted `key` on a VDF line.
fn vdf_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let line = line.trim();
    let rest = line
        .strip_prefix('"')?
        .strip_prefix(key)?
        .strip_prefix('"')?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('"')?;
    rest.split('"').next()
}

fn library_paths(root: &Path) -> Vec<PathBuf> {
    let mut paths = vec![root.to_path_buf()];
    if let Ok(vdf) = std::fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) {
        for line in vdf.lines() {
            if let Some(p) = vdf_value(line, "path") {
                let p = PathBuf::from(p.replace("\\\\", "\\"));
                if !paths.contains(&p) {
                    paths.push(p);
                }
            }
        }
    }
    paths
}

fn parse_manifest(text: &str) -> Option<(String, String)> {
    let mut appid = None;
    let mut name = None;
    for line in text.lines() {
        if appid.is_none() {
            appid = vdf_value(line, "appid").map(str::to_string);
        }
        if name.is_none() {
            name = vdf_value(line, "name").map(str::to_string);
        }
        if appid.is_some() && name.is_some() {
            break;
        }
    }
    Some((appid?, name?))
}

fn scan() -> HashMap<String, String> {
    let mut names = HashMap::new();
    for root in steam_roots() {
        for lib in library_paths(&root) {
            let Ok(read) = std::fs::read_dir(lib.join("steamapps")) else {
                continue;
            };
            for entry in read.flatten() {
                let file = entry.file_name();
                let file = file.to_string_lossy();
                if !(file.starts_with("appmanifest_") && file.ends_with(".acf")) {
                    continue;
                }
                if let Some((id, name)) = std::fs::read_to_string(entry.path())
                    .ok()
                    .and_then(|t| parse_manifest(&t))
                {
                    names.insert(id, name);
                }
            }
        }
    }
    names
}

/// The installed Steam library, scanned lazily.
pub struct SteamLibrary;

impl SteamDb for SteamLibrary {
    fn name(&self, app_id: &str) -> Option<String> {
        let lib = LIBRARY.get_or_init(|| {
            Mutex::new(Library {
                names: scan(),
                scanned_at: Instant::now(),
            })
        });
        let mut lib = lib
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(name) = lib.names.get(app_id) {
            return Some(name.clone());
        }
        if lib.scanned_at.elapsed() >= RESCAN_AFTER {
            lib.names = scan();
            lib.scanned_at = Instant::now();
        }
        lib.names.get(app_id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_yields_id_and_versionless_name() {
        let text = "\"AppState\"\n{\n\t\"appid\"\t\t\"730\"\n\t\"name\"\t\t\"Counter-Strike 2\"\n\t\"buildid\"\t\"123\"\n}\n";
        assert_eq!(
            parse_manifest(text),
            Some(("730".to_string(), "Counter-Strike 2".to_string()))
        );
    }

    #[test]
    fn vdf_value_ignores_other_keys_and_spacing() {
        assert_eq!(
            vdf_value("\t\"path\"\t\t\"/mnt/games/SteamLibrary\"", "path"),
            Some("/mnt/games/SteamLibrary")
        );
        assert_eq!(vdf_value("\"name\" \"Factorio\"", "path"), None);
        assert_eq!(vdf_value("not vdf", "path"), None);
    }
}
