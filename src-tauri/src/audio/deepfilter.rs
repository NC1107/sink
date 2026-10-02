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
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
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
    /// Not requested.
    Idle,
    Running,
    /// Requested, but the plugin is neither installed nor downloaded.
    Missing,
    /// The process died or would not start; the chain fell back to Light
    /// until the user picks Strong again.
    Failed,
}

static STATE: AtomicU8 = AtomicU8::new(0);

pub fn set_state(state: EngineState) {
    STATE.store(state as u8, Ordering::Relaxed);
}

fn state() -> EngineState {
    match STATE.load(Ordering::Relaxed) {
        1 => EngineState::Running,
        2 => EngineState::Missing,
        3 => EngineState::Failed,
        _ => EngineState::Idle,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallKind {
    /// Packaged by the distro or installed by the user (e.g. for EasyEffects).
    System,
    Downloaded,
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineStatus {
    /// False on CPUs upstream ships no build for.
    pub supported: bool,
    pub installed: Option<InstallKind>,
    pub download_bytes: u64,
    pub state: EngineState,
}

pub fn status() -> EngineStatus {
    EngineStatus {
        supported: ASSET.is_some() || find_system().is_some(),
        installed: find().map(|(kind, _)| kind),
        download_bytes: ASSET.as_ref().map_or(0, |a| a.size),
        state: state(),
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
            .join("libdeep_filter_ladspa-0.5.6.so"),
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

/// Fetch and verify the plugin, reporting (bytes so far, total) as it goes.
/// Only a file matching the pinned size and hash is ever moved into place.
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
    let cleanup = |msg: String| {
        let _ = fs::remove_file(&part);
        SinkError::Config(msg)
    };
    if !status.success() {
        return Err(cleanup(format!("download failed ({status})")));
    }
    let size = fs::metadata(&part)?.len();
    if size != asset.size || sha256_hex(&part)? != asset.sha256 {
        return Err(cleanup(
            "the download didn't match the expected file, so it was discarded".into(),
        ));
    }
    progress(asset.size, asset.size);
    fs::rename(&part, dest)?;
    Ok(())
}

pub fn remove_download() -> Result<(), SinkError> {
    match download_path() {
        Some(p) => match fs::remove_file(p) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        },
        None => Ok(()),
    }
}

/// A string literal in PipeWire's SPA-JSON config syntax.
fn spa_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Minimal standalone config: just enough modules to host one filter-chain.
/// The output stream never autoconnects; Sink links it into the mic chain
/// and polices anything else, exactly like its own playback streams.
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
    static SERIAL: AtomicU32 = AtomicU32::new(0);
    let dir = dirs::runtime_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("sink");
    let n = SERIAL.fetch_add(1, Ordering::Relaxed);
    if n == 0 {
        let _ = fs::remove_dir_all(&dir);
    }
    fs::create_dir_all(&dir)?;
    Ok(dir.join(format!("noise-suppression-{n}.conf")))
}

/// The running filter-chain. Dropping it stops the process.
pub struct EngineProcess {
    child: Arc<Mutex<Child>>,
    stopping: Arc<AtomicBool>,
    pid: u32,
}

impl EngineProcess {
    /// Start the filter-chain on `target` (the hardware mic). `on_exit` runs
    /// on a watcher thread if the process dies without being stopped.
    pub fn spawn(
        plugin: &Path,
        target: Option<&str>,
        on_exit: impl FnOnce(u32) + Send + 'static,
    ) -> Result<Self, SinkError> {
        let conf = next_conf_path()?;
        fs::write(&conf, filter_chain_conf(plugin, target))?;

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
