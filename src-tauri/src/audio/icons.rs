//! Desktop-entry based icon and name resolution - the same mechanism app
//! launchers use. Parses .desktop files once (Name/Icon/Exec/
//! StartupWMClass), matches streams against them, and resolves icon names
//! to actual files across the freedesktop icon dirs (user, system,
//! Flatpak exports). Results are cached per identity.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::audio::identity::DesktopDb;

#[derive(Debug, Clone)]
struct DesktopEntry {
    /// Desktop-file id: the file stem, lowercased (e.g. "org.kde.dolphin",
    /// "spotify"). What systemd scopes and flatpak ids point at.
    id: String,
    /// Display name, e.g. "Spotify".
    name: String,
    name_lower: String,
    icon: Option<String>,
    /// Basename of the Exec command, lowercased.
    exec_base: Option<String>,
    wm_class_lower: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Resolved {
    /// Absolute path to an icon file, ready for the asset protocol.
    pub icon_path: Option<String>,
    /// Polished display name from the desktop entry, when matched.
    pub display_name: Option<String>,
}

struct Resolver {
    desktops: Vec<DesktopEntry>,
    cache: HashMap<String, Resolved>,
}

static RESOLVER: OnceLock<Mutex<Resolver>> = OnceLock::new();

fn desktop_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = dirs::data_dir() {
        dirs.push(home.join("applications"));
        dirs.push(home.join("flatpak/exports/share/applications"));
    }
    dirs.push(PathBuf::from("/usr/share/applications"));
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
    dirs
}

/// Every installed icon theme directory (hicolor first, then whatever
/// themes the distro/user installed - Papirus, Adwaita, breeze, …).
/// Many apps only ship icons inside a theme, so hicolor alone misses them.
fn icon_theme_dirs() -> &'static [PathBuf] {
    // The theme set is stable for the process lifetime; scanning the icon
    // roots once avoids re-walking them for every resolve cache miss
    // (icon_name_to_path tries several candidate names per stream).
    static THEMES: OnceLock<Vec<PathBuf>> = OnceLock::new();
    THEMES.get_or_init(|| {
        let mut roots = Vec::new();
        if let Some(data) = dirs::data_dir() {
            roots.push(data.join("icons"));
            roots.push(data.join("flatpak/exports/share/icons"));
        }
        roots.push(PathBuf::from("/usr/share/icons"));
        roots.push(PathBuf::from("/var/lib/flatpak/exports/share/icons"));

        let mut themes = Vec::new();
        for root in roots {
            // hicolor is the freedesktop fallback theme - search it first.
            let hicolor = root.join("hicolor");
            if hicolor.is_dir() {
                themes.push(hicolor);
            }
            if let Ok(read) = fs::read_dir(&root) {
                for entry in read.flatten() {
                    let path = entry.path();
                    if path.is_dir() && path.file_name().is_some_and(|n| n != "hicolor") {
                        themes.push(path);
                    }
                }
            }
        }
        themes
    })
}

fn parse_desktop_file(path: &Path) -> Option<DesktopEntry> {
    let raw = fs::read_to_string(path).ok()?;
    let mut in_entry = false;
    let (mut name, mut icon, mut exec, mut wm_class, mut no_display) =
        (None::<String>, None, None, None, false);
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            match key {
                "Name" if name.is_none() => name = Some(value.to_string()),
                "Icon" => icon = Some(value.to_string()),
                "Exec" => exec = Some(value.to_string()),
                "StartupWMClass" => wm_class = Some(value.to_string()),
                "NoDisplay" => no_display = value.eq_ignore_ascii_case("true"),
                _ => {}
            }
        }
    }
    if no_display {
        return None;
    }
    let name = name?;
    // The program is the first token that isn't a launcher prefix
    // (`env FOO=1 app`, `sh -c app`, `flatpak-spawn --host app`).
    let exec_base = exec.and_then(|e| {
        let first = e.split_whitespace().find(|t| {
            let base = Path::new(t).file_name().map(|f| f.to_string_lossy().into_owned());
            !(t.contains('=')
                || t.starts_with('-')
                || matches!(base.as_deref(), Some("env" | "sh" | "bash" | "flatpak-spawn")))
        })?;
        Path::new(first)
            .file_name()
            .map(|f| f.to_string_lossy().to_lowercase())
    });
    Some(DesktopEntry {
        id: path
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default(),
        name_lower: name.to_lowercase(),
        name,
        icon,
        exec_base,
        wm_class_lower: wm_class.map(|w| w.to_lowercase()),
    })
}

/// Desktop-id candidates for a live process, most reliable first. Linux
/// binaries don't embed icons - the icon belongs to the app's .desktop
/// entry, so identifying a stream's icon means mapping PID → desktop id
/// through the fingerprints the system leaves on the process.
pub fn desktop_id_candidates(pid: u32) -> Vec<String> {
    let mut out = Vec::new();

    // 1. systemd app units: desktop launchers run apps in cgroups named
    //    app[-<launcher>]-<DesktopID>-<rand>.scope or
    //    app-<DesktopID>@<uuid>.service (e.g. app-discord@1a2b….service).
    if let Ok(cgroup) = fs::read_to_string(format!("/proc/{pid}/cgroup")) {
        if let Some(unit) = cgroup
            .lines()
            .filter_map(|l| l.rsplit('/').next())
            .find(|seg| {
                seg.starts_with("app-") && (seg.ends_with(".scope") || seg.ends_with(".service"))
            })
        {
            let token = unit
                .trim_start_matches("app-")
                .trim_end_matches(".scope")
                .trim_end_matches(".service");
            // Drop the instance suffix: @uuid, or a trailing -random part.
            let token = match token.split_once('@') {
                Some((before, _)) => before,
                None => match token.rfind('-') {
                    Some(i) if token[i + 1..].chars().all(|c| c.is_ascii_alphanumeric()) => {
                        &token[..i]
                    }
                    _ => token,
                },
            };
            // systemd escapes '-' inside unit names as \x2d.
            let token = token.replace("\\x2d", "-").to_lowercase();
            if !token.is_empty() {
                out.push(token.clone());
                // And without a launcher prefix (app-gnome-spotify-…).
                if let Some((_, rest)) = token.split_once('-') {
                    out.push(rest.to_string());
                }
            }
        }
    }

    // 2. Flatpak sandbox: the app id sits at the sandbox root.
    if let Ok(info) = fs::read_to_string(format!("/proc/{pid}/root/.flatpak-info")) {
        if let Some(name) = info.lines().find_map(|l| l.strip_prefix("name=")) {
            out.push(name.trim().to_lowercase());
        }
    }

    // 3. GIO stamps processes launched from a menu/dock with the exact
    //    .desktop file (inherited by children - which is what we want for
    //    audio helper processes).
    if let Ok(environ) = fs::read(format!("/proc/{pid}/environ")) {
        for var in environ.split(|b| *b == 0) {
            if let Some(value) = var.strip_prefix(b"GIO_LAUNCHED_DESKTOP_FILE=".as_slice()) {
                let path = String::from_utf8_lossy(value);
                if let Some(stem) = Path::new(path.as_ref()).file_stem() {
                    out.push(stem.to_string_lossy().to_lowercase());
                }
            }
        }
    }

    out
}

/// The real executable basename - resolves wrapper scripts and symlinks
/// (an "electron" stream whose exe is /opt/Slack/slack, say).
fn exe_basename(pid: u32) -> Option<String> {
    fs::read_link(format!("/proc/{pid}/exe"))
        .ok()?
        .file_name()
        .map(|f| f.to_string_lossy().to_lowercase())
}

fn load_desktops() -> Vec<DesktopEntry> {
    let mut entries = Vec::new();
    for dir in desktop_dirs() {
        let Ok(read) = fs::read_dir(&dir) else { continue };
        for file in read.flatten() {
            let path = file.path();
            if path.extension().is_some_and(|e| e == "desktop") {
                if let Some(entry) = parse_desktop_file(&path) {
                    entries.push(entry);
                }
            }
        }
    }
    entries
}

/// Resolve an icon name to a file path across the freedesktop dirs.
fn icon_name_to_path(name: &str) -> Option<String> {
    if name.starts_with('/') && Path::new(name).exists() {
        return Some(name.to_string());
    }
    const SIZES: [&str; 9] = [
        "64x64", "128x128", "256x256", "96x96", "72x72", "48x48", "512x512", "32x32", "24x24",
    ];
    for theme in icon_theme_dirs() {
        for size in SIZES {
            let p = theme.join(size).join("apps").join(format!("{name}.png"));
            if p.exists() {
                return Some(p.to_string_lossy().into_owned());
            }
            // Some themes nest the size the other way around (apps/<size>).
            let p = theme.join("apps").join(size).join(format!("{name}.svg"));
            if p.exists() {
                return Some(p.to_string_lossy().into_owned());
            }
        }
        let svg = theme.join("scalable/apps").join(format!("{name}.svg"));
        if svg.exists() {
            return Some(svg.to_string_lossy().into_owned());
        }
    }
    for ext in ["png", "svg", "xpm"] {
        let p = PathBuf::from("/usr/share/pixmaps").join(format!("{name}.{ext}"));
        if p.exists() {
            return Some(p.to_string_lossy().into_owned());
        }
    }
    None
}

fn resolver() -> &'static Mutex<Resolver> {
    RESOLVER.get_or_init(|| {
        Mutex::new(Resolver {
            desktops: load_desktops(),
            cache: HashMap::new(),
        })
    })
}

/// The installed desktop entries, as the identity ladder sees them.
pub struct Desktops;

impl DesktopDb for Desktops {
    fn name_by_id(&self, id: &str) -> Option<String> {
        let resolver = resolver().lock().ok()?;
        resolver
            .desktops
            .iter()
            .find(|d| d.id == id)
            .map(|d| d.name.clone())
    }

    fn entry_for_exec(&self, ids: &[String], exe: &str) -> Option<(String, String)> {
        let resolver = resolver().lock().ok()?;
        resolver
            .desktops
            .iter()
            .find(|d| ids.iter().any(|i| i == &d.id) && d.exec_base.as_deref() == Some(exe))
            .map(|d| (d.id.clone(), d.name.clone()))
    }
}

/// Resolve the best icon path + display name for a stream.
///
/// `binary` is the process binary when the identity came from it;
/// `icon_hint` is the stream's application.icon-name property.
pub fn resolve(
    app_name: &str,
    binary: Option<&str>,
    icon_hint: Option<&str>,
    pid: Option<u32>,
) -> Resolved {
    let Ok(mut resolver) = resolver().lock() else {
        return Resolved::default();
    };

    // PID presence is part of the key (not the PID itself - it changes per
    // run): a name-only resolution from history must not shadow the more
    // accurate /proc-based one for a live stream, or vice versa.
    let key = format!("{app_name}\0{binary:?}\0{icon_hint:?}\0{}", pid.is_some());
    if let Some(hit) = resolver.cache.get(&key) {
        return hit.clone();
    }

    let app_lower = app_name.to_lowercase();
    let binary_lower = binary.map(str::to_lowercase);

    // The PID beats name-matching: the process's cgroup scope, flatpak id,
    // or launch environment names its desktop entry, and the real exe path
    // sees through wrapper binaries. A scope is inherited from whatever
    // launched the process (a terminal, Steam), so a candidate only counts
    // when its Exec runs this very executable.
    let pid_desktop = pid.and_then(|p| {
        let candidates = desktop_id_candidates(p);
        let exe = exe_basename(p);
        resolver
            .desktops
            .iter()
            .find(|d| {
                !d.id.is_empty()
                    && candidates.iter().any(|c| c == &d.id)
                    && exe.as_deref().map_or(true, |e| d.exec_base.as_deref() == Some(e))
            })
            .or_else(|| {
                // A runtime's entry (python3, java) would claim every app on it.
                let exe = exe.as_deref().filter(|e| !crate::audio::identity::is_wrapper_exe(e))?;
                resolver
                    .desktops
                    .iter()
                    .find(|d| d.exec_base.as_deref() == Some(exe))
            })
    });

    let desktop = pid_desktop.or_else(|| {
        resolver.desktops.iter().find(|d| {
            d.wm_class_lower.as_deref() == Some(app_lower.as_str())
                || (binary_lower.is_some() && d.exec_base == binary_lower)
                || d.name_lower == app_lower
                || d.exec_base.as_deref() == Some(app_lower.as_str())
        })
    });

    // Icon candidates in priority order: explicit stream hint, the desktop
    // entry's icon, the binary name, a slug of the display name.
    let slug = app_lower.replace(' ', "-");
    let mut candidates: Vec<&str> = Vec::new();
    if let Some(hint) = icon_hint {
        candidates.push(hint);
    }
    if let Some(d) = desktop {
        if let Some(icon) = d.icon.as_deref() {
            candidates.push(icon);
        }
    }
    if let Some(b) = binary_lower.as_deref() {
        candidates.push(b);
    }
    candidates.push(&slug);

    let resolved = Resolved {
        icon_path: candidates.iter().find_map(|c| icon_name_to_path(c)),
        display_name: desktop.map(|d| d.name.clone()),
    };
    resolver.cache.insert(key, resolved.clone());
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_id_candidates_handles_real_processes() {
        // Our own PID won't be in an app unit, but the call must not
        // panic or error on a live /proc entry.
        let _ = desktop_id_candidates(std::process::id());
        // Nonexistent PID degrades to no candidates.
        assert!(desktop_id_candidates(u32::MAX - 7).is_empty());
    }

    #[test]
    fn parses_minimal_desktop_entry() {
        let dir = std::env::temp_dir().join("sink-test-desktop");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.desktop");
        fs::write(
            &path,
            "[Desktop Entry]\nName=Cool App\nExec=/usr/bin/coolapp --flag\nIcon=coolapp\nStartupWMClass=CoolApp\n",
        )
        .expect("writes");
        let entry = parse_desktop_file(&path).expect("parses");
        assert_eq!(entry.name, "Cool App");
        assert_eq!(entry.exec_base.as_deref(), Some("coolapp"));
        assert_eq!(entry.wm_class_lower.as_deref(), Some("coolapp"));
        assert_eq!(entry.icon.as_deref(), Some("coolapp"));
    }

    #[test]
    fn nodisplay_entries_are_skipped() {
        let dir = std::env::temp_dir().join("sink-test-desktop");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("hidden.desktop");
        fs::write(&path, "[Desktop Entry]\nName=Hidden\nNoDisplay=true\n").expect("writes");
        assert!(parse_desktop_file(&path).is_none());
    }
}
