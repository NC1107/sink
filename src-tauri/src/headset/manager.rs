//! Ties the base station to the app: discovery/reconnection supervisor, the
//! status read loop, a heartbeat that refreshes battery, and the shared writer
//! that Tauri command handlers use to send control packets.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use crate::audio::pw_native::levels::LevelStore;

use super::hidraw::{DeviceKind, HidDevice};
use super::media::OledFrame;
use super::oled_controller::{self, AuxData, OledCommand, OledContent};
use super::protocol::{self, HeadsetStatus};
use super::protocol_apw::{self, ApwStatus};

/// Event names emitted to the frontend.
const EV_STATUS: &str = "headset-status";
const EV_PRESENCE: &str = "headset-presence";
const EV_CHATMIX: &str = "headset-chatmix";

/// Truncate to `max` chars, adding an ellipsis when clipped (OLED lines are
/// narrow, so notifications need trimming).
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('\u{2026}');
        out
    }
}

/// Shared, thread-safe access to the base station.
pub struct HeadsetManager {
    /// Command writer (control/output reports). `None` while disconnected.
    writer: Mutex<Option<Arc<Mutex<HidDevice>>>>,
    /// Channel to the OLED draw thread. `None` while disconnected.
    oled_tx: Mutex<Option<Sender<OledCommand>>>,
    /// Latest decoded status snapshot (Nova Pro shape; the Arctis Pro's
    /// smaller frame is folded into the same struct so the UI stays uniform).
    status: Mutex<HeadsetStatus>,
    /// Which family is currently connected, if any.
    kind: Mutex<Option<DeviceKind>>,
    connected: AtomicBool,
    app: Mutex<Option<AppHandle>>,
    /// Running `dbus-monitor` child when notification mirroring is on.
    notify_child: Mutex<Option<std::process::Child>>,
    /// PipeWire peak meters, when the native backend is driving (VU mode).
    levels: Mutex<Option<Arc<LevelStore>>>,
}

impl Default for HeadsetManager {
    fn default() -> Self {
        Self {
            writer: Mutex::new(None),
            oled_tx: Mutex::new(None),
            status: Mutex::new(HeadsetStatus::default()),
            kind: Mutex::new(None),
            connected: AtomicBool::new(false),
            app: Mutex::new(None),
            notify_child: Mutex::new(None),
            levels: Mutex::new(None),
        }
    }
}

impl HeadsetManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub fn status(&self) -> HeadsetStatus {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Which device family is connected (None when nothing is).
    pub fn kind(&self) -> Option<DeviceKind> {
        self.kind.lock().ok().and_then(|k| *k)
    }

    /// Model name for the UI.
    pub fn model_name(&self) -> Option<&'static str> {
        self.kind().map(|k| match k {
            DeviceKind::NovaPro => "Arctis Nova Pro Wireless",
            DeviceKind::ArctisPro => "Arctis Pro Wireless",
        })
    }

    /// Run a control command through the writer. Returns an error string
    /// suitable for a Tauri command result.
    pub fn send(&self, packet: &[u8]) -> Result<(), String> {
        let guard = self.writer.lock().map_err(|_| "writer lock poisoned")?;
        let writer = guard
            .as_ref()
            .ok_or("no Arctis Nova Pro Wireless base station connected")?;
        let mut dev = writer.lock().map_err(|_| "device lock poisoned")?;
        dev.write_command(packet)
            .map_err(|e| format!("write failed: {e}"))
    }

    /// Send a sequence of control commands in order.
    pub fn send_all(&self, packets: &[Vec<u8>]) -> Result<(), String> {
        for p in packets {
            self.send(p)?;
        }
        Ok(())
    }

    /// Push data owned by other subsystems (mouse battery, app list) to the
    /// OLED thread so its modes can render them.
    pub fn push_aux(&self, aux: AuxData) {
        let _ = self.oled(OledCommand::UpdateAux(Box::new(aux)));
    }

    /// Send a message to the OLED thread (no-op error if disconnected).
    pub fn oled(&self, cmd: OledCommand) -> Result<(), String> {
        let guard = self.oled_tx.lock().map_err(|_| "oled lock poisoned")?;
        let tx = guard.as_ref().ok_or("no base station connected")?;
        tx.send(cmd).map_err(|_| "OLED thread is gone".to_string())
    }

    /// Convenience: show media frames on the OLED.
    pub fn oled_play(&self, frames: Vec<OledFrame>, looping: bool) -> Result<(), String> {
        self.oled(OledCommand::Set(OledContent::Frames {
            frames: Arc::new(frames),
            looping,
        }))
    }

    /// Hand the manager the PipeWire meter store so the OLED VU mode can
    /// read peaks. Called once at startup; absent on the pactl fallback.
    pub fn set_levels(&self, levels: Arc<LevelStore>) {
        if let Ok(mut slot) = self.levels.lock() {
            *slot = Some(levels);
        }
    }

    /// Whether desktop-notification mirroring is currently running.
    pub fn notify_mirror_running(&self) -> bool {
        self.notify_child
            .lock()
            .map(|c| c.is_some())
            .unwrap_or(false)
    }

    /// Start/stop mirroring desktop notifications to the OLED. Persists the
    /// choice so it survives restarts.
    pub fn set_notify_mirror(self: &Arc<Self>, enabled: bool) -> Result<(), String> {
        let _ = crate::persistence::notify_mirror::set(enabled);
        if enabled {
            self.start_notify_mirror();
        } else if let Ok(mut slot) = self.notify_child.lock() {
            if let Some(mut child) = slot.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        Ok(())
    }

    /// Spawn `dbus-monitor` and forward each desktop notification's summary +
    /// body to the OLED as a timed notification. No-op if already running.
    fn start_notify_mirror(self: &Arc<Self>) {
        {
            let mut slot = match self.notify_child.lock() {
                Ok(s) => s,
                Err(_) => return,
            };
            if slot.is_some() {
                return;
            }
            let child = std::process::Command::new("dbus-monitor")
                .arg("interface='org.freedesktop.Notifications',member='Notify'")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn();
            match child {
                Ok(mut c) => {
                    let stdout = c.stdout.take();
                    *slot = Some(c);
                    if let Some(stdout) = stdout {
                        let me = Arc::clone(self);
                        std::thread::spawn(move || me.notify_reader(stdout));
                    }
                }
                Err(e) => {
                    eprintln!("sink: dbus-monitor unavailable ({e}); notification mirror off");
                }
            }
        }
    }

    /// Parse `dbus-monitor` output. Each Notify carries its string args in
    /// order: app_name, app_icon, summary, body — so the 3rd and 4th quoted
    /// strings after a Notify header are what we show.
    fn notify_reader(self: Arc<Self>, stdout: std::process::ChildStdout) {
        use std::io::{BufRead, BufReader};
        let reader = BufReader::new(stdout);
        let mut capturing = false;
        let mut strings: Vec<String> = Vec::new();
        for line in reader.lines().map_while(Result::ok) {
            let trimmed = line.trim_start();
            if line.contains("member=Notify") {
                capturing = true;
                strings.clear();
                continue;
            }
            if capturing {
                if let Some(rest) = trimmed.strip_prefix("string \"") {
                    // Strip the trailing quote.
                    let val = rest.strip_suffix('"').unwrap_or(rest).to_string();
                    strings.push(val);
                    if strings.len() >= 4 {
                        let summary = strings[2].clone();
                        let body = strings[3].clone();
                        capturing = false;
                        let mut lines = Vec::new();
                        if !summary.is_empty() {
                            lines.push(truncate(&summary, 20));
                        }
                        if !body.is_empty() {
                            lines.push(truncate(&body, 21));
                        }
                        if !lines.is_empty() {
                            let _ = self.oled(OledCommand::Notify {
                                lines,
                                duration: Duration::from_secs(5),
                            });
                        }
                    }
                } else if trimmed.starts_with("method call") || trimmed.starts_with("signal ") {
                    capturing = false;
                }
            }
        }
        // dbus-monitor exited; clear the handle so it can be restarted.
        if let Ok(mut slot) = self.notify_child.lock() {
            *slot = None;
        }
    }

    /// Start the background supervisor. Call once from Tauri setup.
    pub fn start(self: &Arc<Self>, app: AppHandle) {
        if let Ok(mut slot) = self.app.lock() {
            *slot = Some(app);
        }
        let me = Arc::clone(self);
        std::thread::spawn(move || me.supervise());
    }

    fn emit_status(&self) {
        if let (Ok(app), Ok(status)) = (self.app.lock(), self.status.lock()) {
            if let Some(app) = app.as_ref() {
                let _ = app.emit(EV_STATUS, &*status);
            }
        }
    }

    fn emit_presence(&self, present: bool) {
        self.connected.store(present, Ordering::Relaxed);
        if let Ok(app) = self.app.lock() {
            if let Some(app) = app.as_ref() {
                let _ = app.emit(EV_PRESENCE, present);
            }
        }
    }

    /// Discovery/reconnect loop. Each successful connection runs the read loop
    /// until the device goes away, then falls back to polling for it again.
    fn supervise(self: Arc<Self>) {
        loop {
            let writer = match HidDevice::discover() {
                Ok(w) => w,
                Err(_) => {
                    std::thread::sleep(Duration::from_secs(3));
                    continue;
                }
            };
            let path = writer.device_path();

            // Open a second fd for blocking reads before we move the writer.
            let mut reader = match writer.open_reader() {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("sink: headset reader open failed: {e}");
                    std::thread::sleep(Duration::from_secs(3));
                    continue;
                }
            };

            let kind = path.kind;
            if let Ok(mut slot) = self.kind.lock() {
                *slot = Some(kind);
            }

            // Only the Nova generation has an OLED we can drive.
            if kind.has_oled() {
                let (oled_tx, oled_rx) = std::sync::mpsc::channel();
                let path = path.clone();
                let levels = self.levels.lock().ok().and_then(|l| l.clone());
                std::thread::spawn(move || oled_controller::run(path, oled_rx, levels));
                if let Ok(mut slot) = self.oled_tx.lock() {
                    *slot = Some(oled_tx);
                }
            }

            let writer = Arc::new(Mutex::new(writer));
            if let Ok(mut slot) = self.writer.lock() {
                *slot = Some(Arc::clone(&writer));
            }
            self.emit_presence(true);

            // Safe handshake: pull an initial snapshot (and, on Nova, enable
            // the event stream). Never writes user settings.
            let init = match kind {
                DeviceKind::NovaPro => protocol::init_safe(),
                DeviceKind::ArctisPro => protocol_apw::init_safe(),
            };
            let _ = self.send_all(&init);

            // Resume notification mirroring if the user left it on.
            if crate::persistence::notify_mirror::is_enabled() {
                self.start_notify_mirror();
            }

            // Heartbeat: refresh the full status every 2 s so battery/charge
            // stay current even without a triggering event.
            let hb_writer = Arc::clone(&writer);
            let hb_stop = Arc::new(AtomicBool::new(false));
            let hb_stop2 = Arc::clone(&hb_stop);
            let heartbeat = std::thread::spawn(move || {
                while !hb_stop2.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_secs(2));
                    let ok = hb_writer
                        .lock()
                        .ok()
                        .map(|mut d| match kind {
                            DeviceKind::NovaPro => {
                                d.write_command(&protocol::status_query()).is_ok()
                                    && d.write_command(&protocol::volume_query()).is_ok()
                            }
                            DeviceKind::ArctisPro => {
                                d.write_command(&protocol_apw::status_query()).is_ok()
                            }
                        })
                        .unwrap_or(false);
                    if !ok {
                        break;
                    }
                }
            });

            // Read loop — blocks until the device disappears or errors.
            let mut buf = [0u8; protocol::COMMAND_LEN];
            loop {
                match reader.read_report(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) if kind == DeviceKind::ArctisPro => {
                        // Pre-Nova frame: power state + battery only. Fold it
                        // into the shared status shape the UI already renders.
                        let mut apw = ApwStatus::default();
                        if apw.apply_frame(&buf) {
                            if let Ok(mut s) = self.status.lock() {
                                s.present = true;
                                s.headset_battery_percent = apw.battery_percent;
                                s.power_status = Some(
                                    if apw.online { "online" } else { "offline" }.to_string(),
                                );
                            }
                            self.emit_status();
                        }
                    }
                    Ok(_) => {
                        // The hardware ChatMix wheel drives the software mix
                        // live via a dedicated event (kept out of the status
                        // snapshot so periodic heartbeats can't re-apply it).
                        if let Some((game, chat)) = protocol::chatmix_from_frame(&buf) {
                            if let Ok(app) = self.app.lock() {
                                if let Some(app) = app.as_ref() {
                                    let _ = app.emit(EV_CHATMIX, (game, chat));
                                }
                            }
                        }
                        let changed = self
                            .status
                            .lock()
                            .map(|mut s| s.apply_frame(&buf))
                            .unwrap_or(false);
                        if changed {
                            self.emit_status();
                            // Feed the OLED dashboard/notifications the latest.
                            let snapshot = self.status();
                            let _ = self.oled(OledCommand::UpdateStatus(Box::new(snapshot)));
                        }
                    }
                }
            }

            // Teardown: device gone. Stop heartbeat + OLED, mark disconnected,
            // then loop back to discovery.
            hb_stop.store(true, Ordering::Relaxed);
            let _ = heartbeat.join();
            if let Ok(mut slot) = self.oled_tx.lock() {
                if let Some(tx) = slot.take() {
                    let _ = tx.send(OledCommand::Shutdown);
                }
            }
            if let Ok(mut slot) = self.writer.lock() {
                *slot = None;
            }
            if let Ok(mut slot) = self.kind.lock() {
                *slot = None;
            }
            if let Ok(mut s) = self.status.lock() {
                *s = HeadsetStatus::default();
            }
            self.emit_presence(false);
        }
    }
}
