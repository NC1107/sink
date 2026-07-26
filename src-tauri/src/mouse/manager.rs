//! Discovery, connection supervision and command dispatch for the mouse.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::protocol::{self, MouseConfig};

const EV_STATUS: &str = "mouse-status";
const EV_PRESENCE: &str = "mouse-presence";

/// Live mouse state pushed to the UI.
#[derive(Debug, Clone, Serialize, Default)]
pub struct MouseStatus {
    pub present: bool,
    pub model: Option<String>,
    pub wireless: bool,
    pub battery_percent: Option<u8>,
    pub charging: bool,
}

/// A discovered mouse control node.
#[derive(Clone)]
struct MousePath {
    dev: PathBuf,
    product_id: u16,
}

/// Locate the Aerox's configuration interface (USB interface 3). The mouse
/// exposes several HID interfaces; only that one accepts config commands.
///
/// Both the cable and the 2.4 GHz dongle can be plugged in at once, each
/// enumerating its own node. The cable is preferred: when it's connected the
/// mouse is physically on it, and the idle dongle just answers `0xff`.
fn find_mouse() -> Option<MousePath> {
    let entries = std::fs::read_dir("/sys/class/hidraw").ok()?;
    let mut wireless: Option<MousePath> = None;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("hidraw") {
            continue;
        }
        let sys = Path::new("/sys/class/hidraw").join(&*name).join("device");
        let Some(pid) = matching_product(&sys) else {
            continue;
        };
        if interface_number(&sys) != Some(protocol::CONTROL_INTERFACE) {
            continue;
        }
        let found = MousePath {
            dev: PathBuf::from("/dev").join(&*name),
            product_id: pid,
        };
        if protocol::is_wireless(pid) {
            wireless.get_or_insert(found);
        } else {
            return Some(found);
        }
    }
    wireless
}

fn matching_product(sys_device: &Path) -> Option<u16> {
    let uevent = std::fs::read_to_string(sys_device.join("uevent")).ok()?;
    let hid_id = uevent.lines().find_map(|l| l.strip_prefix("HID_ID="))?;
    let mut parts = hid_id.split(':');
    let _bus = parts.next()?;
    let vendor = u16::from_str_radix(parts.next()?.trim(), 16).ok()?;
    let product = u16::from_str_radix(parts.next()?.trim(), 16).ok()?;
    if vendor == protocol::VENDOR_ID && protocol::PRODUCT_IDS.contains(&product) {
        Some(product)
    } else {
        None
    }
}

/// The USB interface number this HID node belongs to.
///
/// Read from `bInterfaceNumber` on the owning USB interface directory rather
/// than parsed out of the path: the HID node's own directory name looks like
/// `0003:1038:185A.0020`, whose trailing number is a HID device index, not an
/// interface — reading that instead silently matches the wrong node.
fn interface_number(sys_device: &Path) -> Option<u8> {
    let real = std::fs::canonicalize(sys_device).ok()?;
    // Walk up until a directory exposes bInterfaceNumber (usually the parent).
    let mut dir = real.as_path();
    for _ in 0..4 {
        let candidate = dir.join("bInterfaceNumber");
        if let Ok(raw) = std::fs::read_to_string(&candidate) {
            return u8::from_str_radix(raw.trim(), 16).ok();
        }
        dir = dir.parent()?;
    }
    None
}

pub struct MouseManager {
    dev: Mutex<Option<MousePath>>,
    status: Mutex<MouseStatus>,
    config: Mutex<MouseConfig>,
    connected: AtomicBool,
    app: Mutex<Option<AppHandle>>,
}

impl Default for MouseManager {
    fn default() -> Self {
        Self {
            dev: Mutex::new(None),
            status: Mutex::new(MouseStatus::default()),
            config: Mutex::new(MouseConfig::default()),
            connected: AtomicBool::new(false),
            app: Mutex::new(None),
        }
    }
}

impl MouseManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub fn status(&self) -> MouseStatus {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn config(&self) -> MouseConfig {
        self.config.lock().map(|c| c.clone()).unwrap_or_default()
    }

    /// The product id of the connected mouse (commands are encoded per
    /// transport, so callers need it).
    pub fn product_id(&self) -> Option<u16> {
        self.dev.lock().ok().and_then(|d| d.as_ref().map(|d| d.product_id))
    }

    /// Send a pre-encoded command to the mouse.
    pub fn send(&self, packet: &[u8]) -> Result<(), String> {
        let guard = self.dev.lock().map_err(|_| "mouse lock poisoned")?;
        let path = guard.as_ref().ok_or("no SteelSeries mouse connected")?;
        let mut file = OpenOptions::new()
            .write(true)
            .open(&path.dev)
            .map_err(|e| format!("open mouse: {e}"))?;
        file.write_all(packet)
            .map_err(|e| format!("write mouse: {e}"))?;
        // The device drops commands sent back-to-back; rivalcfg uses ~50 ms.
        std::thread::sleep(Duration::from_millis(50));
        Ok(())
    }

    /// Apply a command built from the connected product id, then persist.
    pub fn send_with_pid<F>(&self, build: F) -> Result<(), String>
    where
        F: FnOnce(u16) -> Vec<u8>,
    {
        let pid = self.product_id().ok_or("no SteelSeries mouse connected")?;
        self.send(&build(pid))
    }

    /// Persist current settings to the mouse's onboard memory.
    pub fn save(&self) -> Result<(), String> {
        self.send_with_pid(protocol::save)
    }

    /// Replace the cached config (so the UI survives a reconnect).
    pub fn set_config(&self, config: MouseConfig) {
        if let Ok(mut c) = self.config.lock() {
            *c = config;
        }
    }

    pub fn start(self: &Arc<Self>, app: AppHandle) {
        if let Ok(mut slot) = self.app.lock() {
            *slot = Some(app);
        }
        let me = Arc::clone(self);
        std::thread::spawn(move || me.supervise());
    }

    fn emit_presence(&self, present: bool) {
        self.connected.store(present, Ordering::Relaxed);
        if let Ok(app) = self.app.lock() {
            if let Some(app) = app.as_ref() {
                let _ = app.emit(EV_PRESENCE, present);
            }
        }
    }

    fn emit_status(&self) {
        if let (Ok(app), Ok(status)) = (self.app.lock(), self.status.lock()) {
            if let Some(app) = app.as_ref() {
                let _ = app.emit(EV_STATUS, &*status);
            }
        }
    }

    /// Poll for the mouse; while it's present, refresh battery periodically.
    fn supervise(self: Arc<Self>) {
        loop {
            let found = find_mouse();
            match found {
                Some(path) => {
                    let wireless = protocol::is_wireless(path.product_id);
                    let first_time = !self.is_connected();
                    if let Ok(mut slot) = self.dev.lock() {
                        *slot = Some(path.clone());
                    }
                    if first_time {
                        if let Ok(mut s) = self.status.lock() {
                            s.present = true;
                            s.wireless = wireless;
                            s.model = Some("Aerox 9 Wireless".to_string());
                        }
                        self.emit_presence(true);
                    }
                    self.refresh_battery(&path);
                    std::thread::sleep(Duration::from_secs(30));
                }
                None => {
                    if self.is_connected() {
                        if let Ok(mut slot) = self.dev.lock() {
                            *slot = None;
                        }
                        if let Ok(mut s) = self.status.lock() {
                            *s = MouseStatus::default();
                        }
                        self.emit_presence(false);
                    }
                    std::thread::sleep(Duration::from_secs(5));
                }
            }
        }
    }

    /// Query battery and push the result to the UI. Best effort: a mouse that
    /// is asleep simply won't answer.
    fn refresh_battery(&self, path: &MousePath) {
        let Ok(mut file) = OpenOptions::new().read(true).write(true).open(&path.dev) else {
            return;
        };
        if file
            .write_all(&protocol::battery_query(path.product_id))
            .is_err()
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(60));
        let mut buf = [0u8; 8];
        use std::io::Read;
        let Ok(n) = file.read(&mut buf) else { return };
        if n == 0 {
            return;
        }
        if let Some((percent, charging)) = protocol::parse_battery(&buf[..n]) {
            if let Ok(mut s) = self.status.lock() {
                s.battery_percent = Some(percent);
                s.charging = charging;
            }
            self.emit_status();
        }
    }
}
