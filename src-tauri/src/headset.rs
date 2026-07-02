//! Opt-in SteelSeries (and other `headsetcontrol`-supported) wireless headset
//! detection.
//!
//! Some wireless dongles keep their USB sink node alive when the headset
//! powers off (the base station *is* the sound card), so nothing disappears
//! from the audio graph and the normal follow-default failover has nothing to
//! react to - audio keeps flowing into a headset that isn't there. We can't
//! see that at the PipeWire layer, but `headsetcontrol` reads it over the
//! vendor HID interface. When enabled, this module polls it and, on a
//! confirmed power-off, moves the system default off the dead sink to the best
//! available output (restoring it when the headset returns).
//!
//! Design note: detection is deliberately *conservative*. We only treat a
//! headset as disconnected when `headsetcontrol` reports an explicit
//! battery-unavailable status, so a JSON-schema mismatch or a transient error
//! can never wrongly pull audio off a headset that is actually in use - it
//! just means the feature does nothing, same as it being off.

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::Serialize;

use crate::audio::backend::AudioBackend;
use crate::audio::types::OutputDevice;

/// How often to poll `headsetcontrol` while detection is enabled.
const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Consecutive identical reads required before acting - debounces a blip.
const DEBOUNCE: u8 = 2;

/// What `headsetcontrol -o json` tells us about a supported wireless headset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadsetReading {
    /// The `headsetcontrol` binary is not on `PATH`.
    ToolMissing,
    /// It ran but errored (couldn't execute, or unparsable output).
    ToolError(String),
    /// It ran fine but reported no *supported* device - either nothing is
    /// plugged in, or (commonly) the installed `headsetcontrol` is too old for
    /// this device. The UI turns this into an actionable hint.
    NoSupportedDevice,
    /// A supported headset's base/dongle is present. `connected` is whether
    /// the wireless headset itself is powered on.
    Present { model: String, connected: bool },
}

/// Parse `headsetcontrol -o json`. Pure, so the whole decision path is
/// testable without the binary or the hardware.
pub fn parse_reading(json: &str) -> HeadsetReading {
    let value: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(e) => return HeadsetReading::ToolError(format!("unparsable headsetcontrol json: {e}")),
    };
    let device = value
        .get("devices")
        .and_then(|d| d.as_array())
        .and_then(|d| d.first());
    let Some(device) = device else {
        return HeadsetReading::NoSupportedDevice;
    };
    let model = device
        .get("device")
        .and_then(|d| d.as_str())
        .or_else(|| device.get("product").and_then(|p| p.as_str()))
        .unwrap_or("Headset")
        .to_string();
    // Only an explicit unavailable-family battery status counts as "off"; every
    // other value (available, charging, or a key we didn't recognise) is read
    // as "on" so we never yank audio off a working headset.
    let battery_status = device
        .get("battery")
        .and_then(|b| b.get("status"))
        .and_then(|s| s.as_str());
    let connected = !matches!(
        battery_status.map(str::to_ascii_uppercase).as_deref(),
        Some("BATTERY_UNAVAILABLE") | Some("BATTERY_TIMEOUT") | Some("BATTERY_HIDERROR")
    );
    HeadsetReading::Present { model, connected }
}

/// Reads headset state. Abstracted so tests can inject readings and a native
/// HID backend could replace the subprocess later.
pub trait HeadsetDetector: Send + Sync {
    fn read(&self) -> HeadsetReading;
}

/// The `headsetcontrol` CLI detector (Option A).
pub struct HeadsetControl;

impl HeadsetDetector for HeadsetControl {
    fn read(&self) -> HeadsetReading {
        match Command::new("headsetcontrol").args(["-o", "json"]).output() {
            Ok(out) => parse_reading(&String::from_utf8_lossy(&out.stdout)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => HeadsetReading::ToolMissing,
            Err(e) => HeadsetReading::ToolError(e.to_string()),
        }
    }
}

/// A SteelSeries/Arctis output device, matched by name so we know which sink
/// goes dead when the headset powers off.
fn is_headset_sink(device: &OutputDevice) -> bool {
    let hay = format!("{} {}", device.name, device.description).to_ascii_lowercase();
    hay.contains("steelseries") || hay.contains("arctis")
}

/// The best output to fall over to: the first real device that is neither the
/// headset itself nor one of Sink's own virtual channel sinks.
fn pick_fallback(devices: &[OutputDevice]) -> Option<String> {
    devices
        .iter()
        .find(|d| !is_headset_sink(d) && !d.name.starts_with("sink_"))
        .map(|d| d.name.clone())
}

/// What to do on a confirmed state change. Pure and idempotent: `has_saved`
/// (whether we've already moved the default away) plus the confirmed
/// connected state fully determine the action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    None,
    FailOver,
    Restore,
}

fn decide(connected: bool, has_saved: bool) -> Action {
    match (connected, has_saved) {
        (false, false) => Action::FailOver, // headset off, not yet switched
        (true, true) => Action::Restore,    // headset back, we had switched
        _ => Action::None,
    }
}

/// UI-facing snapshot of detection state.
#[derive(Debug, Clone, Serialize)]
pub struct HeadsetStatus {
    /// The monitor is running (the setting is on).
    pub enabled: bool,
    /// `headsetcontrol` is installed.
    pub tool_installed: bool,
    /// A supported device is present (base/dongle recognised).
    pub device_present: bool,
    /// The wireless headset is powered on.
    pub connected: bool,
    pub model: Option<String>,
    /// A one-line actionable note when something needs attention.
    pub message: Option<String>,
}

impl HeadsetStatus {
    fn from_reading(reading: &HeadsetReading, enabled: bool) -> Self {
        let mut status = HeadsetStatus {
            enabled,
            tool_installed: !matches!(reading, HeadsetReading::ToolMissing),
            device_present: matches!(reading, HeadsetReading::Present { .. }),
            connected: matches!(reading, HeadsetReading::Present { connected: true, .. }),
            model: None,
            message: None,
        };
        match reading {
            HeadsetReading::ToolMissing => {
                status.message = Some("Install headsetcontrol to use this".into());
            }
            HeadsetReading::ToolError(e) => status.message = Some(e.clone()),
            HeadsetReading::NoSupportedDevice => {
                status.message =
                    Some("No supported headset found (or headsetcontrol is too old for it)".into());
            }
            HeadsetReading::Present { model, connected } => {
                status.model = Some(model.clone());
                status.message = Some(if *connected {
                    format!("{model} connected")
                } else {
                    format!("{model} is off - audio moved to your speakers")
                });
            }
        }
        status
    }
}

/// Owns the polling thread. Started/stopped by the settings toggle.
pub struct HeadsetMonitor {
    running: Arc<AtomicBool>,
    handle: Mutex<Option<JoinHandle<()>>>,
    detector: Arc<dyn HeadsetDetector>,
}

impl HeadsetMonitor {
    pub fn new() -> Self {
        Self::with_detector(Arc::new(HeadsetControl))
    }

    pub fn with_detector(detector: Arc<dyn HeadsetDetector>) -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            handle: Mutex::new(None),
            detector,
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// A one-shot read for the UI (independent of whether the monitor runs).
    pub fn status(&self) -> HeadsetStatus {
        HeadsetStatus::from_reading(&self.detector.read(), self.is_running())
    }

    /// Start polling. No-op if already running.
    pub fn start(&self, backend: Arc<dyn AudioBackend>) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        let running = self.running.clone();
        let detector = self.detector.clone();
        let handle = std::thread::spawn(move || monitor_loop(&running, backend, detector.as_ref()));
        if let Ok(mut slot) = self.handle.lock() {
            *slot = Some(handle);
        }
    }

    /// Stop polling and join the thread.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        let handle = self.handle.lock().ok().and_then(|mut slot| slot.take());
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }
}

impl Default for HeadsetMonitor {
    fn default() -> Self {
        Self::new()
    }
}

fn monitor_loop(
    running: &AtomicBool,
    backend: Arc<dyn AudioBackend>,
    detector: &dyn HeadsetDetector,
) {
    let mut stable: Option<bool> = None; // last confirmed connected state
    let mut candidate: Option<bool> = None; // debounce candidate
    let mut count: u8 = 0;
    // (device we switched away from, device we switched to) while headset is off.
    let mut saved: Option<(String, String)> = None;

    while running.load(Ordering::Relaxed) {
        std::thread::sleep(POLL_INTERVAL);
        if !running.load(Ordering::Relaxed) {
            break;
        }

        let (connected, model) = match detector.read() {
            HeadsetReading::Present { connected, model } => (connected, model),
            // No supported device / tool issue: stop tracking, act on nothing.
            _ => {
                candidate = None;
                count = 0;
                continue;
            }
        };

        // Debounce: require DEBOUNCE identical reads before trusting a change.
        if candidate == Some(connected) {
            count = count.saturating_add(1);
        } else {
            candidate = Some(connected);
            count = 1;
        }
        if count < DEBOUNCE || stable == Some(connected) {
            continue;
        }
        stable = Some(connected);

        match decide(connected, saved.is_some()) {
            Action::FailOver => {
                if let Some(pair) = fail_over(backend.as_ref(), &model) {
                    saved = Some(pair);
                }
            }
            Action::Restore => {
                if let Some((from, to)) = saved.take() {
                    restore(backend.as_ref(), &from, &to);
                }
            }
            Action::None => {}
        }
    }
}

/// Move the system default off the (dead) headset sink to the best fallback,
/// but only if the headset sink is actually the current default. Returns the
/// (from, to) pair so it can be restored.
fn fail_over(backend: &dyn AudioBackend, _model: &str) -> Option<(String, String)> {
    let (default_out, _) = backend.get_default_devices().ok()?;
    let default_out = default_out?;
    let devices = backend.list_output_devices().ok()?;

    let headset_is_default = devices
        .iter()
        .any(|d| d.name == default_out && is_headset_sink(d));
    if !headset_is_default {
        return None; // user isn't listening on the headset - nothing to move
    }

    let fallback = pick_fallback(&devices)?;
    match backend.set_default_output(&fallback) {
        Ok(()) => Some((default_out, fallback)),
        Err(e) => {
            eprintln!("sink: headset failover could not switch default: {e}");
            None
        }
    }
}

/// Restore the default to the headset - but only if nothing else has changed
/// it since (never stomp a manual switch the user made in the meantime).
fn restore(backend: &dyn AudioBackend, from: &str, to: &str) {
    match backend.get_default_devices() {
        Ok((Some(current), _)) if current == to => {
            if let Err(e) = backend.set_default_output(from) {
                eprintln!("sink: headset restore could not switch default back: {e}");
            }
        }
        _ => {} // default moved elsewhere; leave the user's choice alone
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(name: &str, desc: &str) -> OutputDevice {
        OutputDevice {
            index: 0,
            name: name.to_string(),
            description: desc.to_string(),
        }
    }

    #[test]
    fn parses_no_device_when_list_empty() {
        let json = r#"{ "device_count": 0, "devices": [] }"#;
        assert_eq!(parse_reading(json), HeadsetReading::NoSupportedDevice);
    }

    #[test]
    fn parses_connected_when_battery_available() {
        let json = r#"{ "device_count": 1, "devices": [
            { "device": "SteelSeries Arctis Nova 7", "battery": { "status": "BATTERY_AVAILABLE", "level": 80 } }
        ] }"#;
        assert_eq!(
            parse_reading(json),
            HeadsetReading::Present { model: "SteelSeries Arctis Nova 7".into(), connected: true }
        );
    }

    #[test]
    fn parses_charging_as_connected() {
        let json = r#"{ "devices": [ { "device": "X", "battery": { "status": "BATTERY_CHARGING" } } ] }"#;
        assert_eq!(
            parse_reading(json),
            HeadsetReading::Present { model: "X".into(), connected: true }
        );
    }

    #[test]
    fn parses_unavailable_battery_as_disconnected() {
        let json = r#"{ "devices": [ { "device": "Nova 7", "battery": { "status": "BATTERY_UNAVAILABLE" } } ] }"#;
        assert_eq!(
            parse_reading(json),
            HeadsetReading::Present { model: "Nova 7".into(), connected: false }
        );
    }

    #[test]
    fn unknown_battery_status_is_treated_as_connected_fail_safe() {
        // A schema we didn't anticipate must never read as "off".
        let json = r#"{ "devices": [ { "device": "Y", "battery": { "status": "SOMETHING_NEW" } } ] }"#;
        assert_eq!(
            parse_reading(json),
            HeadsetReading::Present { model: "Y".into(), connected: true }
        );
        let no_battery = r#"{ "devices": [ { "device": "Z" } ] }"#;
        assert_eq!(
            parse_reading(no_battery),
            HeadsetReading::Present { model: "Z".into(), connected: true }
        );
    }

    #[test]
    fn garbage_json_is_a_tool_error_not_a_disconnect() {
        assert!(matches!(parse_reading("not json"), HeadsetReading::ToolError(_)));
    }

    #[test]
    fn fallback_skips_the_headset_and_virtual_sinks() {
        let devices = vec![
            dev("sink_game", "Game"),
            dev("alsa_output.usb-SteelSeries_Arctis_Nova_Pro-00.analog-stereo", "Arctis Nova Pro"),
            dev("alsa_output.pci-0000_01_00.1.hdmi-stereo", "GB203 HDMI"),
            dev("alsa_output.pci-0000_00_1f.3.analog-stereo", "Built-in Speakers"),
        ];
        assert_eq!(
            pick_fallback(&devices).as_deref(),
            Some("alsa_output.pci-0000_01_00.1.hdmi-stereo")
        );
    }

    #[test]
    fn fallback_is_none_when_only_headset_and_virtuals_exist() {
        let devices = vec![dev("sink_chat", "Chat"), dev("x.arctis.y", "Arctis 7")];
        assert_eq!(pick_fallback(&devices), None);
    }

    #[test]
    fn decide_matrix() {
        assert_eq!(decide(false, false), Action::FailOver); // off, not switched
        assert_eq!(decide(false, true), Action::None); // off, already switched
        assert_eq!(decide(true, true), Action::Restore); // back, restore
        assert_eq!(decide(true, false), Action::None); // on, nothing to do
    }
}
