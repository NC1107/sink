
//! Which program a stream belongs to. Streams describe their audio stack as
//! often as their app ("FMOD Audio", a versioned name, "Chromium"), so the
//! process is asked first and the stream's own claims are the fallback.

use std::collections::HashMap;

use crate::audio::types;

/// `prop`/`value` is what rules key on; `pid` is set only when trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub prop: String,
    pub value: String,
    pub display: String,
    pub pid: Option<u32>,
}

pub const PROP_FLATPAK: &str = "flatpak.app_id";
pub const PROP_STEAM: &str = "steam.app_id";
pub const PROP_APPIMAGE: &str = "appimage.name";
pub const PROP_DESKTOP: &str = "desktop.id";
pub const PROP_EXE: &str = "process.exe";

/// Props derived from the process; only these adopt legacy stream-keyed rules.
pub fn is_process_prop(prop: &str) -> bool {
    matches!(
        prop,
        PROP_FLATPAK | PROP_STEAM | PROP_APPIMAGE | PROP_DESKTOP | PROP_EXE
    )
}

pub trait ProcReader {
    fn exe_basename(&self, pid: u32) -> Option<String>;
    fn env_var(&self, pid: u32, key: &str) -> Option<String>;
    /// Desktop-entry ids from the process's launch (cgroup scope, GIO stamp).
    fn desktop_ids(&self, pid: u32) -> Vec<String>;
}

pub trait DesktopDb {
    fn name_by_id(&self, id: &str) -> Option<String>;
    /// `(id, name)` of an entry among `ids` whose Exec runs `exe`.
    fn entry_for_exec(&self, ids: &[String], exe: &str) -> Option<(String, String)>;
}

pub trait SteamDb {
    fn name(&self, app_id: &str) -> Option<String>;
}

/// Reads the live `/proc`.
pub struct Proc;

impl ProcReader for Proc {
    fn exe_basename(&self, pid: u32) -> Option<String> {
        std::fs::read_link(format!("/proc/{pid}/exe"))
            .ok()?
            .file_name()
            .map(|f| f.to_string_lossy().to_lowercase())
    }

    fn env_var(&self, pid: u32, key: &str) -> Option<String> {
        let environ = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
        let prefix = format!("{key}=");
        environ
            .split(|b| *b == 0)
            .find_map(|var| {
                let s = String::from_utf8_lossy(var);
                s.strip_prefix(prefix.as_str()).map(str::to_string)
            })
            .filter(|v| !v.trim().is_empty())
    }

    fn desktop_ids(&self, pid: u32) -> Vec<String> {
        crate::audio::icons::desktop_id_candidates(pid)
    }
}

/// Runtimes, not programs: matching one merges every app on it.
pub(crate) fn is_wrapper_exe(exe: &str) -> bool {
    let e = exe.to_ascii_lowercase();
    types::is_wrapper_name(&e)
        || e.starts_with("python")
        || e.starts_with("wine")
        || e.ends_with("-preloader")
        || matches!(
            e.as_str(),
            "apprun" | "sh" | "bash" | "env" | "bwrap" | "electron" | "ld-linux-x86-64.so.2"
        )
}

fn windows_binary(props: &HashMap<String, String>) -> bool {
    props
        .get("application.process.binary")
        .is_some_and(|b| b.to_ascii_lowercase().ends_with(".exe"))
}

fn parse_pid(v: Option<&String>) -> Option<u32> {
    v.and_then(|s| s.trim().parse().ok()).filter(|p| *p > 1)
}

/// Sandboxed clients report their in-sandbox pid, someone else on the host.
fn trusted_pid(props: &HashMap<String, String>, proc: &dyn ProcReader) -> Option<u32> {
    if props.contains_key("pipewire.access.portal.app_id")
        || props.get("pipewire.access").map(String::as_str) == Some("flatpak")
    {
        return None;
    }
    let reported = parse_pid(props.get("application.process.id"))?;
    // A native client's peer pid is kernel-verified; agreement settles it.
    if parse_pid(props.get("pipewire.sec.pid")) == Some(reported) {
        return Some(reported);
    }
    // Without a binary to agree with there is nothing to verify.
    let binary = props.get("application.process.binary")?;
    let exe = proc.exe_basename(reported)?;
    (exe.eq_ignore_ascii_case(binary.trim()) || is_wrapper_exe(&exe)).then_some(reported)
}

fn identity(prop: &str, value: &str, display: String, pid: Option<u32>) -> Identity {
    Identity {
        prop: prop.to_string(),
        value: value.to_string(),
        display,
        pid,
    }
}

/// First row that yields a real (non-runtime) value wins.
pub fn resolve(
    props: &HashMap<String, String>,
    proc: &dyn ProcReader,
    desktops: &dyn DesktopDb,
    steam: &dyn SteamDb,
) -> Identity {
    let (fallback_display, fallback_prop, fallback_value) =
        types::resolve_identity(|key| props.get(key).cloned());

    if let Some(app) = props
        .get("pipewire.access.portal.app_id")
        .filter(|a| !a.trim().is_empty())
    {
        let display = desktops
            .name_by_id(&app.to_lowercase())
            .unwrap_or_else(|| fallback_display.clone());
        return identity(PROP_FLATPAK, app, display, None);
    }

    let pid = trusted_pid(props, proc);
    if let Some(pid) = pid {
        if let Some(app_id) = proc
            .env_var(pid, "SteamAppId")
            .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
        {
            let display = steam
                .name(&app_id)
                .unwrap_or_else(|| fallback_display.clone());
            return identity(PROP_STEAM, &app_id, display, Some(pid));
        }
        if let Some(stem) = proc.env_var(pid, "APPIMAGE").and_then(|path| {
            std::path::Path::new(&path)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        }) {
            let display = types::prettify(&stem);
            return identity(PROP_APPIMAGE, &stem, display, Some(pid));
        }
        let exe = proc.exe_basename(pid);
        let ids = proc.desktop_ids(pid);
        if let (Some(exe), false) = (&exe, ids.is_empty()) {
            if let Some((id, name)) = desktops.entry_for_exec(&ids, exe) {
                return identity(PROP_DESKTOP, &id, name, Some(pid));
            }
        }
        if let Some(exe) = exe {
            if !is_wrapper_exe(&exe) && !windows_binary(props) {
                return identity(PROP_EXE, &exe, types::prettify(&exe), Some(pid));
            }
        }
    }

    identity(&fallback_prop, &fallback_value, fallback_display, pid)
}

/// The stream's own properties a legacy rule may have been keyed on.
pub fn legacy_matchers(props: &HashMap<String, String>) -> Vec<(String, String)> {
    [
        "application.name",
        "application.process.binary",
        "media.name",
        "node.name",
    ]
    .into_iter()
    .filter_map(|k| props.get(k).map(|v| (k.to_string(), v.clone())))
    .filter(|(_, v)| !v.trim().is_empty())
    .collect()
}

/// A pid-less stream borrows a sibling's identity only when exactly one
/// process-backed stream claims the same real (non-runtime) app name, by
/// its stream name or by what it resolved to (a game's engine stream says
/// "FMOD Audio" while the game's own stream carries its manifest name).
pub fn adopt_from_siblings(identities: &mut [Identity], props: &[&HashMap<String, String>]) {
    let real = |n: &str| {
        let n = n.trim();
        (!n.is_empty() && !types::is_generic_name(n) && !types::is_wrapper_name(n))
            .then(|| n.to_lowercase())
    };
    let name_of = |p: &HashMap<String, String>| p.get("application.name").and_then(|n| real(n));
    let mut by_name: HashMap<String, Vec<Identity>> = HashMap::new();
    for (id, p) in identities.iter().zip(props) {
        if id.pid.is_some() && is_process_prop(&id.prop) {
            for name in name_of(p).into_iter().chain(real(&id.display)) {
                let bucket = by_name.entry(name).or_default();
                if !bucket
                    .iter()
                    .any(|b| b.prop == id.prop && b.value == id.value)
                {
                    bucket.push(id.clone());
                }
            }
        }
    }
    for (id, p) in identities.iter_mut().zip(props) {
        if id.pid.is_some() || is_process_prop(&id.prop) {
            continue;
        }
        if let Some(name) = name_of(p) {
            if let Some([donor]) = by_name.get(&name).map(Vec::as_slice) {
                id.prop = donor.prop.clone();
                id.value = donor.value.clone();
                id.display = donor.display.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeProc {
        exe: HashMap<u32, &'static str>,
        env: HashMap<(u32, &'static str), &'static str>,
        ids: HashMap<u32, Vec<&'static str>>,
    }

    impl FakeProc {
        fn new() -> Self {
            Self {
                exe: HashMap::new(),
                env: HashMap::new(),
                ids: HashMap::new(),
            }
        }
    }

    impl ProcReader for FakeProc {
        fn exe_basename(&self, pid: u32) -> Option<String> {
            self.exe.get(&pid).map(|s| s.to_string())
        }
        fn env_var(&self, pid: u32, key: &str) -> Option<String> {
            self.env
                .iter()
                .find(|((p, k), _)| *p == pid && *k == key)
                .map(|(_, v)| v.to_string())
        }
        fn desktop_ids(&self, pid: u32) -> Vec<String> {
            self.ids
                .get(&pid)
                .map(|v| v.iter().map(|s| s.to_string()).collect())
                .unwrap_or_default()
        }
    }

    struct FakeDesktops(Vec<(&'static str, &'static str, &'static str)>); // id, name, exec

    impl DesktopDb for FakeDesktops {
        fn name_by_id(&self, id: &str) -> Option<String> {
            self.0.iter().find(|d| d.0 == id).map(|d| d.1.to_string())
        }
        fn entry_for_exec(&self, ids: &[String], exe: &str) -> Option<(String, String)> {
            self.0
                .iter()
                .find(|d| ids.iter().any(|i| i == d.0) && d.2 == exe)
                .map(|d| (d.0.to_string(), d.1.to_string()))
        }
    }

    struct FakeSteam;
    impl SteamDb for FakeSteam {
        fn name(&self, app_id: &str) -> Option<String> {
            (app_id == "730").then(|| "Counter-Strike 2".to_string())
        }
    }

    fn props(kv: &[(&str, &str)]) -> HashMap<String, String> {
        kv.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn resolve_with(p: &HashMap<String, String>, proc: &FakeProc) -> Identity {
        resolve(
            p,
            proc,
            &FakeDesktops(vec![("firefox", "Firefox", "firefox")]),
            &FakeSteam,
        )
    }

    #[test]
    fn flatpak_client_is_keyed_on_its_app_id_and_never_touches_proc() {
        let p = props(&[
            ("application.name", "Spotify"),
            ("application.process.id", "2"),
            ("pipewire.access", "flatpak"),
            ("pipewire.access.portal.app_id", "com.spotify.Client"),
        ]);
        let mut proc = FakeProc::new();
        proc.exe.insert(2, "kthreadd");
        let desktops = FakeDesktops(vec![("com.spotify.client", "Spotify", "spotify")]);
        let id = resolve(&p, &proc, &desktops, &FakeSteam);
        assert_eq!(
            (id.prop.as_str(), id.value.as_str()),
            (PROP_FLATPAK, "com.spotify.Client")
        );
        assert_eq!(id.display, "Spotify");
        assert_eq!(id.pid, None);

        // A flatpak client without a portal id still reports a sandbox
        // pid; that pid must never be walked, even when it looks like a game.
        let p = props(&[
            ("application.name", "Spotify"),
            ("application.process.id", "2"),
            ("pipewire.access", "flatpak"),
        ]);
        proc.env.insert((2, "SteamAppId"), "730");
        let id = resolve(&p, &proc, &desktops, &FakeSteam);
        assert_eq!(id.prop, "application.name");
        assert_eq!(id.pid, None);
    }

    #[test]
    fn steam_game_is_keyed_on_its_app_id_whatever_its_streams_say() {
        let mut proc = FakeProc::new();
        proc.env.insert((100, "SteamAppId"), "730");
        proc.exe.insert(100, "cs2");
        for name in ["cs2", "FMOD Audio", "SDL Application"] {
            let p = props(&[
                ("application.name", name),
                ("application.process.id", "100"),
                ("pipewire.sec.pid", "100"),
            ]);
            let id = resolve_with(&p, &proc);
            assert_eq!(
                (id.prop.as_str(), id.value.as_str()),
                (PROP_STEAM, "730"),
                "{name}"
            );
            assert_eq!(id.display, "Counter-Strike 2");
        }
    }

    #[test]
    fn a_reported_pid_that_disagrees_with_the_binary_is_not_trusted() {
        // Sandbox pid 2 collided with a real host process.
        let p = props(&[
            ("application.name", "Some Game"),
            ("application.process.binary", "somegame"),
            ("application.process.id", "2"),
        ]);
        let mut proc = FakeProc::new();
        proc.exe.insert(2, "kthreadd");
        proc.env.insert((2, "SteamAppId"), "730");
        let id = resolve_with(&p, &proc);
        assert_eq!(id.prop, "application.name");
        assert_eq!(id.pid, None, "must not read /proc for a mismatched pid");
    }

    #[test]
    fn a_pid_with_nothing_to_verify_it_against_is_not_trusted() {
        let mut proc = FakeProc::new();
        proc.exe.insert(31, "somegame");
        proc.env.insert((31, "SteamAppId"), "730");
        // No sec.pid, no binary: could be any sandbox reporting its own pid.
        let p = props(&[
            ("application.name", "Some Game"),
            ("application.process.id", "31"),
        ]);
        let id = resolve_with(&p, &proc);
        assert_eq!(id.pid, None);
        assert_eq!(id.prop, "application.name");
    }

    #[test]
    fn exe_beats_a_versioned_application_name() {
        let mut proc = FakeProc::new();
        proc.exe.insert(7, "factorio");
        for name in ["Factorio: Space Age 2.1.8", "Factorio: Space Age 2.1.11"] {
            let p = props(&[
                ("application.name", name),
                ("application.process.binary", "factorio"),
                ("application.process.id", "7"),
                ("pipewire.sec.pid", "7"),
            ]);
            let id = resolve_with(&p, &proc);
            assert_eq!(
                (id.prop.as_str(), id.value.as_str()),
                (PROP_EXE, "factorio")
            );
        }
    }

    #[test]
    fn runtime_executables_never_become_the_identity() {
        let mut proc = FakeProc::new();
        for (pid, exe) in [
            (21, "python3.12"),
            (22, "java"),
            (23, "apprun"),
            (4, "wine64-preloader"),
        ] {
            proc.exe.insert(pid, exe);
        }
        for (pid, binary) in [(21, "python3.12"), (22, "java"), (23, "apprun")] {
            let p = props(&[
                ("application.name", "Cool App"),
                ("application.process.binary", binary),
                ("application.process.id", &pid.to_string()),
                ("pipewire.sec.pid", &pid.to_string()),
            ]);
            let id = resolve_with(&p, &proc);
            assert_ne!(id.prop, PROP_EXE, "{binary} must not key an identity");
            assert_eq!(id.value, "Cool App");
        }

        let wine = props(&[
            ("application.name", "RocketLeague.exe"),
            ("application.process.binary", "RocketLeague.exe"),
            ("application.process.id", "4"),
            ("pipewire.sec.pid", "4"),
        ]);
        let id = resolve_with(&wine, &proc);
        assert_eq!(id.prop, "application.name");
        assert_eq!(id.value, "RocketLeague.exe");
    }

    #[test]
    fn a_windows_binary_skips_the_exe_row_whatever_the_loader_is_called() {
        let mut proc = FakeProc::new();
        proc.exe.insert(9, "proton"); // some future loader with no wine in its name
        let p = props(&[
            ("application.name", "Game.exe"),
            ("application.process.binary", "Game.exe"),
            ("application.process.id", "9"),
            ("pipewire.sec.pid", "9"),
        ]);
        let id = resolve_with(&p, &proc);
        assert_ne!(id.prop, PROP_EXE);
        assert_eq!(id.value, "Game.exe");
    }

    #[test]
    fn desktop_entry_needs_the_exec_to_match_the_process() {
        let mut proc = FakeProc::new();
        proc.exe.insert(5, "firefox");
        proc.ids.insert(5, vec!["wezterm", "firefox"]); // launched from a terminal
        let desktops = FakeDesktops(vec![
            ("wezterm", "WezTerm", "wezterm"),
            ("firefox", "Firefox", "firefox"),
        ]);
        let p = props(&[
            ("application.name", "Firefox"),
            ("application.process.binary", "firefox"),
            ("application.process.id", "5"),
            ("pipewire.sec.pid", "5"),
        ]);
        let id = resolve(&p, &proc, &desktops, &FakeSteam);
        assert_eq!(
            (id.prop.as_str(), id.value.as_str()),
            (PROP_DESKTOP, "firefox")
        );
        assert_eq!(id.display, "Firefox");

        // Only the terminal's scope is visible: its entry runs wezterm, not
        // firefox, so it must be rejected rather than mislabel the app.
        proc.ids.insert(5, vec!["wezterm"]);
        let id = resolve(&p, &proc, &desktops, &FakeSteam);
        assert_eq!((id.prop.as_str(), id.value.as_str()), (PROP_EXE, "firefox"));
    }

    #[test]
    fn appimage_is_keyed_on_the_image_not_the_mounted_apprun() {
        let mut proc = FakeProc::new();
        proc.exe.insert(11, "apprun");
        proc.env
            .insert((11, "APPIMAGE"), "/home/me/Apps/Obsidian-1.6.7.AppImage");
        let p = props(&[
            ("application.name", "Chromium"),
            ("application.process.binary", "apprun"),
            ("application.process.id", "11"),
            ("pipewire.sec.pid", "11"),
        ]);
        let id = resolve_with(&p, &proc);
        assert_eq!(
            (id.prop.as_str(), id.value.as_str()),
            (PROP_APPIMAGE, "Obsidian-1.6.7")
        );
    }

    #[test]
    fn no_pid_falls_back_to_todays_name_ladder() {
        let p = props(&[
            ("application.name", "WEBRTC VoiceEngine"),
            ("application.process.binary", "Discord"),
        ]);
        let id = resolve_with(&p, &FakeProc::new());
        assert_eq!(
            (id.prop.as_str(), id.value.as_str()),
            ("application.process.binary", "Discord")
        );
        assert_eq!(id.display, "Discord");
    }

    #[test]
    fn sibling_adoption_is_unambiguous_only() {
        let mut proc = FakeProc::new();
        proc.env.insert((100, "SteamAppId"), "730");
        proc.exe.insert(100, "cs2");
        let with_pid = props(&[
            ("application.name", "cs2"),
            ("application.process.id", "100"),
            ("pipewire.sec.pid", "100"),
        ]);
        let without = props(&[("application.name", "cs2"), ("node.name", "cs2")]);
        let mut ids = vec![
            resolve_with(&with_pid, &proc),
            resolve_with(&without, &proc),
        ];
        adopt_from_siblings(&mut ids, &[&with_pid, &without]);
        assert_eq!(
            (ids[1].prop.as_str(), ids[1].value.as_str()),
            (PROP_STEAM, "730")
        );

        // The engine's stream says "FMOD Audio"; the pid-less one carries the
        // game's real name, which the donor resolved to from its manifest.
        let engine = props(&[
            ("application.name", "FMOD Audio"),
            ("application.process.id", "100"),
            ("pipewire.sec.pid", "100"),
        ]);
        let named = props(&[("application.name", "Counter-Strike 2")]);
        let mut ids = vec![resolve_with(&engine, &proc), resolve_with(&named, &proc)];
        adopt_from_siblings(&mut ids, &[&engine, &named]);
        assert_eq!(
            (ids[1].prop.as_str(), ids[1].value.as_str()),
            (PROP_STEAM, "730")
        );
        assert_eq!(ids[1].display, "Counter-Strike 2");

        // "Chromium" is a runtime name shared by unrelated apps: never a donor.
        proc.exe.insert(200, "spotify");
        let spotify = props(&[
            ("application.name", "Chromium"),
            ("application.process.id", "200"),
            ("pipewire.sec.pid", "200"),
        ]);
        let chrome = props(&[("application.name", "Chromium")]);
        let mut ids = vec![resolve_with(&spotify, &proc), resolve_with(&chrome, &proc)];
        adopt_from_siblings(&mut ids, &[&spotify, &chrome]);
        assert_eq!(ids[1].prop, "application.name");
        assert_eq!(ids[1].value, "Chromium");
    }
}
