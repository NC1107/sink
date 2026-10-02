//! Opt-in "Strong" noise suppression: DeepFilterNet's LADSPA plugin, run by
//! PipeWire's own filter-chain in a child `pipewire` process.
//!
//! The plugin is a 52 MB file fetched only when a user asks for it, so Sink
//! itself stays small. It runs out of process because the plugin aborts when
//! the CPU can't keep up; in-process that would take Sink down with it.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::SinkError;

/// The filter-chain's two streams. Both carry the internal prefix, so the app
/// list and device lists never show them.
pub const CAPTURE_NAME: &str = "sink-internal-df-capture";
pub const OUTPUT_NAME: &str = "sink-internal-df-output";

const PLUGIN_FILE: &str = "libdeep_filter_ladspa.so";
pub const VERSION: &str = "0.5.6";
const DOWNLOAD_PREFIX: &str = "libdeep_filter_ladspa-";

/// Project pages the Mic screen links to. Fixed here so the frontend can
/// only ever ask to open these, never an arbitrary URL.
pub const LIGHT_URL: &str = "https://github.com/jneem/nnnoiseless";
pub const STRONG_URL: &str = "https://github.com/Rikorose/DeepFilterNet/releases/tag/v0.5.6";

/// The upstream release asset, pinned by hash: a download that doesn't match
/// byte for byte is never loaded.
struct Asset {
    url: &'static str,
    sha256: &'static str,
    size: u64,
}

#[cfg(target_arch = "x86_64")]
const ASSET: Option<Asset> = Some(Asset {
    url: "https://github.com/Rikorose/DeepFilterNet/releases/download/v0.5.6/libdeep_filter_ladspa-0.5.6-x86_64-unknown-linux-gnu.so",
    sha256: "2ca3205c2911d389604a826a240e745597d50252b5cab81c8248252b335e2236",
    size: 52_711_472,
});
#[cfg(not(target_arch = "x86_64"))]
const ASSET: Option<Asset> = None;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineState {
    Idle,
    Running,
    Missing,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallKind {
    System,
    Downloaded,
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineStatus {
    /// False on CPUs upstream ships no build for.
    pub supported: bool,
    pub installed: Option<InstallKind>,
    pub download_bytes: u64,
    pub version: &'static str,
    pub state: EngineState,
}

/// `state` comes from the PipeWire loop, which owns the engine.
pub fn status(state: EngineState) -> EngineStatus {
    EngineStatus {
        supported: ASSET.is_some() || find_system().is_some(),
        installed: find().map(|(kind, _)| kind),
        download_bytes: ASSET.as_ref().map_or(0, |a| a.size),
        version: VERSION,
        state,
    }
}

fn ladspa_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var("LADSPA_PATH")
        .map(|v| std::env::split_paths(&v).collect())
        .unwrap_or_default();
    dirs.extend(
        [
            "/usr/lib/ladspa",
            "/usr/lib64/ladspa",
            "/usr/local/lib/ladspa",
            "/usr/lib/x86_64-linux-gnu/ladspa",
            "/usr/lib/aarch64-linux-gnu/ladspa",
        ]
        .map(PathBuf::from),
    );
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".ladspa"));
    }
    dirs
}

fn find_system() -> Option<PathBuf> {
    ladspa_dirs()
        .into_iter()
        .map(|d| d.join(PLUGIN_FILE))
        .find(|p| p.is_file())
}

/// Versioned, so a future pin never mistakes an older download for itself.
fn download_path() -> Option<PathBuf> {
    Some(
        dirs::data_dir()?
            .join("sink")
            .join("plugins")
            .join(format!("{DOWNLOAD_PREFIX}{VERSION}.so")),
    )
}

/// The plugin to run: a system install wins, so nobody downloads a second
/// copy of something they already have.
pub fn find() -> Option<(InstallKind, PathBuf)> {
    if let Some(p) = find_system() {
        return Some((InstallKind::System, p));
    }
    let p = download_path()?;
    p.is_file().then_some((InstallKind::Downloaded, p))
}

fn sha256_hex(path: &Path) -> Result<String, SinkError> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// curl first, wget as the fallback (Ubuntu desktop ships wget, not
/// always curl). Shelling out keeps a TLS stack out of the binary.
fn spawn_fetch(url: &str, dest: &Path) -> Result<Child, SinkError> {
    let curl = Command::new("curl")
        .args([
            "-fsSL",
            "--proto",
            "=https",
            "--tlsv1.2",
            "--retry",
            "2",
            "-o",
        ])
        .arg(dest)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match curl {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Command::new("wget")
            .args(["-q", "--https-only", "-O"])
            .arg(dest)
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    SinkError::Config("downloading needs curl or wget installed".into())
                } else {
                    e.into()
                }
            }),
        other => other.map_err(Into::into),
    }
}

static DOWNLOADING: AtomicBool = AtomicBool::new(false);

pub fn download(progress: impl Fn(u64, u64)) -> Result<PathBuf, SinkError> {
    let asset = ASSET
        .as_ref()
        .ok_or_else(|| SinkError::Config("DeepFilterNet has no build for this CPU".into()))?;
    let dest = download_path()
        .ok_or_else(|| SinkError::Config("cannot resolve the user data directory".into()))?;
    if DOWNLOADING.swap(true, Ordering::AcqRel) {
        return Err(SinkError::Config("a download is already running".into()));
    }
    let result = fetch_verified(asset, &dest, progress);
    DOWNLOADING.store(false, Ordering::Release);
    result.map(|()| dest)
}

fn fetch_verified(
    asset: &Asset,
    dest: &Path,
    progress: impl Fn(u64, u64),
) -> Result<(), SinkError> {
    let dir = dest
        .parent()
        .ok_or_else(|| SinkError::Config("bad plugin path".into()))?;
    fs::create_dir_all(dir)?;
    let part = dir.join(".download.part");
    let _ = fs::remove_file(&part);

    let mut child = spawn_fetch(asset.url, &part)?;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        let done = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        progress(done.min(asset.size), asset.size);
        std::thread::sleep(Duration::from_millis(150));
    };
    if !status.success() {
        let _ = fs::remove_file(&part);
        return Err(SinkError::Config(format!("download failed ({status})")));
    }
    install_verified(&part, dest, asset)?;
    progress(asset.size, asset.size);
    Ok(())
}

/// Anything but the pinned file is deleted, never left where it could load.
fn install_verified(part: &Path, dest: &Path, asset: &Asset) -> Result<(), SinkError> {
    let matches = fs::metadata(part)?.len() == asset.size && sha256_hex(part)? == asset.sha256;
    if !matches {
        let _ = fs::remove_file(part);
        return Err(SinkError::Config(
            "the download didn't match the expected file, so it was discarded".into(),
        ));
    }
    fs::rename(part, dest)?;
    if let Some(dir) = dest.parent() {
        remove_other_versions(dir, dest);
    }
    Ok(())
}

/// Downloads live outside the package, so they survive Sink updates; a
/// release that pins a newer plugin cleans up the one it replaced.
fn remove_other_versions(dir: &Path, keep: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let stale = path != keep
            && path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(DOWNLOAD_PREFIX) && n.ends_with(".so"));
        if stale {
            let _ = fs::remove_file(path);
        }
    }
}

pub fn remove_download() -> Result<(), SinkError> {
    match download_path().map(fs::remove_file) {
        Some(Err(e)) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

fn spa_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The output never autoconnects: Sink links it into the mic chain and
/// polices anything else, like its own playback streams.
fn filter_chain_conf(plugin: &Path, target: Option<&str>) -> String {
    let target = target
        .map(|t| format!("target.object = {}", spa_str(t)))
        .unwrap_or_default();
    format!(
        r#"context.properties = {{ log.level = 1 }}
context.spa-libs = {{
    audio.convert.* = audioconvert/libspa-audioconvert
    support.*       = support/libspa-support
}}
context.modules = [
    {{ name = libpipewire-module-rt flags = [ ifexists nofail ] }}
    {{ name = libpipewire-module-protocol-native }}
    {{ name = libpipewire-module-client-node }}
    {{ name = libpipewire-module-adapter }}
    {{ name = libpipewire-module-filter-chain
        args = {{
            node.description = "Sink noise suppression"
            media.name = "Sink noise suppression"
            audio.rate = 48000
            audio.position = [ MONO ]
            filter.graph = {{
                nodes = [ {{ type = ladspa name = df plugin = {plugin} label = deep_filter_mono }} ]
            }}
            capture.props = {{
                node.name = {capture}
                node.dont-reconnect = true
                {target}
            }}
            playback.props = {{
                node.name = {output}
                node.autoconnect = false
                node.dont-reconnect = true
            }}
        }}
    }}
]
"#,
        plugin = spa_str(&plugin.to_string_lossy()),
        capture = spa_str(CAPTURE_NAME),
        output = spa_str(OUTPUT_NAME),
    )
}

/// A fresh config path per engine start. Each watcher deletes only its own
/// file, so a rebuild's old watcher can never remove the new engine's config
/// before `pipewire` reads it. The first start clears what a crashed Sink
/// left behind (single-instance, so nothing else is using them).
fn next_conf_path() -> Result<PathBuf, SinkError> {
    use std::os::unix::fs::DirBuilderExt;
    static SERIAL: AtomicU32 = AtomicU32::new(0);
    // Only the per-user runtime dir: a shared /tmp fallback would let another
    // user plant the config `pipewire` loads. PipeWire needs it anyway.
    let dir = dirs::runtime_dir()
        .ok_or_else(|| SinkError::Config("XDG_RUNTIME_DIR is not set".into()))?
        .join("sink");
    let n = SERIAL.fetch_add(1, Ordering::Relaxed);
    if n == 0 {
        let _ = fs::remove_dir_all(&dir);
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    Ok(dir.join(format!("noise-suppression-{n}.conf")))
}

fn write_private(path: &Path, contents: &str) -> Result<(), SinkError> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?
        .write_all(contents.as_bytes())?;
    Ok(())
}

/// The running filter-chain. Dropping it stops the process.
pub struct EngineProcess {
    child: Arc<Mutex<Child>>,
    stopping: Arc<AtomicBool>,
    pid: u32,
}

impl EngineProcess {
    /// `on_exit` runs on a watcher thread, and only if the process dies
    /// without being stopped.
    pub fn spawn(
        plugin: &Path,
        target: Option<&str>,
        on_exit: impl FnOnce(u32) + Send + 'static,
    ) -> Result<Self, SinkError> {
        let conf = next_conf_path()?;
        write_private(&conf, &filter_chain_conf(plugin, target))?;

        let mut cmd = Command::new("pipewire");
        cmd.arg("-c")
            .arg(&conf)
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        // SAFETY: prctl is async-signal-safe and touches no Rust state.
        // PDEATHSIG ends the engine if Sink is killed before it can stop it.
        unsafe {
            use std::os::unix::process::CommandExt;
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        let child = cmd.spawn().map_err(|e| {
            let _ = fs::remove_file(&conf);
            SinkError::Config(format!("start noise suppression engine: {e}"))
        })?;
        let pid = child.id();
        let child = Arc::new(Mutex::new(child));
        let stopping = Arc::new(AtomicBool::new(false));

        let (watch_child, watch_stop) = (child.clone(), stopping.clone());
        std::thread::Builder::new()
            .name("noise-engine-watch".into())
            .spawn(move || {
                loop {
                    let exited = match watch_child.lock() {
                        Ok(mut c) => !matches!(c.try_wait(), Ok(None)),
                        Err(_) => true,
                    };
                    if exited {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
                let _ = fs::remove_file(&conf);
                if !watch_stop.load(Ordering::Acquire) {
                    on_exit(pid);
                }
            })
            .map_err(|e| SinkError::Config(format!("spawn engine watcher: {e}")))?;

        Ok(Self {
            child,
            stopping,
            pid,
        })
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }
}

impl Drop for EngineProcess {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        if let Ok(mut c) = self.child.lock() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spa_strings_escape_quotes_and_backslashes() {
        assert_eq!(spa_str(r#"a"b\c"#), r#""a\"b\\c""#);
    }

    #[test]
    fn conf_pins_the_target_and_never_autoconnects_the_output() {
        let conf = filter_chain_conf(Path::new("/p/lib.so"), Some("alsa_input.usb-mic"));
        assert!(conf.contains(r#"plugin = "/p/lib.so""#));
        assert!(conf.contains(r#"target.object = "alsa_input.usb-mic""#));
        let playback = &conf[conf.find("playback.props").unwrap()..];
        assert!(playback.contains("node.autoconnect = false"));
        assert!(playback.contains(OUTPUT_NAME));
    }

    #[test]
    fn conf_without_a_target_follows_the_default() {
        let conf = filter_chain_conf(Path::new("/p/lib.so"), None);
        assert!(!conf.contains("target.object"));
    }

    #[test]
    fn a_new_download_removes_older_versions_only() {
        let dir = std::env::temp_dir().join(format!("sink-df-versions-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let old = dir.join("libdeep_filter_ladspa-0.5.5.so");
        let new = dir.join("libdeep_filter_ladspa-0.5.6.so");
        let other = dir.join("notes.txt");
        for p in [&old, &new, &other] {
            fs::write(p, b"x").unwrap();
        }
        remove_other_versions(&dir, &new);
        let (old_gone, new_kept, other_kept) = (!old.exists(), new.exists(), other.exists());
        let _ = fs::remove_dir_all(&dir);
        assert!(old_gone && new_kept && other_kept);
    }

    #[test]
    fn download_name_carries_the_pinned_version() {
        let p = download_path().unwrap();
        assert!(p.ends_with(format!("libdeep_filter_ladspa-{VERSION}.so")));
        assert!(STRONG_URL.ends_with(VERSION));
    }

    /// A stand-in for the pinned asset: the 3 bytes "abc".
    const ABC: Asset = Asset {
        url: "",
        sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        size: 3,
    };

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sink-df-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_matching_download_is_installed_and_replaces_older_versions() {
        let dir = scratch("install-ok");
        let (part, dest) = (
            dir.join(".download.part"),
            dir.join("libdeep_filter_ladspa-0.5.6.so"),
        );
        let old = dir.join("libdeep_filter_ladspa-0.5.5.so");
        fs::write(&part, b"abc").unwrap();
        fs::write(&old, b"x").unwrap();
        let result = install_verified(&part, &dest, &ABC);
        let state = (dest.exists(), part.exists(), old.exists());
        let _ = fs::remove_dir_all(&dir);
        assert!(result.is_ok());
        assert_eq!(state, (true, false, false));
    }

    #[test]
    fn a_tampered_download_of_the_right_size_is_discarded() {
        let dir = scratch("install-hash");
        let (part, dest) = (dir.join(".download.part"), dir.join("plugin.so"));
        fs::write(&part, b"abd").unwrap();
        let result = install_verified(&part, &dest, &ABC);
        let state = (dest.exists(), part.exists());
        let _ = fs::remove_dir_all(&dir);
        assert!(result.is_err());
        assert_eq!(state, (false, false));
    }

    #[test]
    fn a_truncated_download_is_discarded() {
        let dir = scratch("install-size");
        let (part, dest) = (dir.join(".download.part"), dir.join("plugin.so"));
        fs::write(&part, b"ab").unwrap();
        let result = install_verified(&part, &dest, &ABC);
        let state = (dest.exists(), part.exists());
        let _ = fs::remove_dir_all(&dir);
        assert!(result.is_err());
        assert_eq!(state, (false, false));
    }

    #[test]
    fn hash_matches_a_known_vector() {
        let dir = std::env::temp_dir().join(format!("sink-df-hash-{}", std::process::id()));
        fs::write(&dir, b"abc").unwrap();
        let h = sha256_hex(&dir).unwrap();
        let _ = fs::remove_file(&dir);
        assert_eq!(
            h,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
