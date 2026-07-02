use serde::Serialize;
use tauri::State;

use crate::headset::HeadsetStatus;
use crate::persistence::autostart;
use crate::persistence::prefs::{DeviceLabelStyle, Prefs};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct BackendInfo {
    /// True = native PipeWire backend; false = pactl subprocess fallback.
    pub native: bool,
}

#[tauri::command]
pub fn get_backend_info(state: State<'_, AppState>) -> BackendInfo {
    BackendInfo {
        native: state.backend_native,
    }
}

#[tauri::command]
pub fn get_autostart() -> bool {
    autostart::is_enabled()
}

/// Enable/disable the systemd user unit for autostart on login.
#[tauri::command]
pub fn set_autostart(enabled: bool) -> Result<bool, String> {
    let result = if enabled {
        autostart::enable()
    } else {
        autostart::disable()
    };
    result.map_err(|e| e.to_string())?;
    Ok(autostart::is_enabled())
}

#[tauri::command]
pub fn get_prefs(state: State<'_, AppState>) -> Result<Prefs, String> {
    Ok(state.lock_mixer()?.prefs.clone())
}

/// Set the device naming style. Existing nodes keep their labels until
/// they are recreated (restart or rename).
#[tauri::command]
pub fn set_device_label_style(
    state: State<'_, AppState>,
    style: DeviceLabelStyle,
) -> Result<(), String> {
    let prefs = {
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.device_label_style = style;
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())
}

/// Toggle "start minimized" (boot to tray when autostarting). Rewrites
/// the systemd unit when autostart is already enabled so the flag tracks
/// the preference.
#[tauri::command]
pub fn set_start_minimized(state: State<'_, AppState>, minimized: bool) -> Result<(), String> {
    let prefs = {
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.start_minimized = minimized;
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())?;
    if autostart::is_enabled() {
        autostart::enable().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Current SteelSeries headset-detection status for the settings UI (installed?
/// supported device present? headset on/off?).
#[tauri::command]
pub fn get_headset_status(state: State<'_, AppState>) -> HeadsetStatus {
    state.headset.status()
}

/// Turn opt-in SteelSeries headset-off detection on or off. Persists the
/// preference and starts/stops the polling monitor.
#[tauri::command]
pub fn set_headset_detection(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<HeadsetStatus, String> {
    let prefs = {
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.headset_detection = enabled;
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())?;
    if enabled {
        state.headset.start(state.backend.clone());
    } else {
        state.headset.stop();
    }
    Ok(state.headset.status())
}

/// Show or hide the title-bar balance slider.
#[tauri::command]
pub fn set_balance_visible(state: State<'_, AppState>, visible: bool) -> Result<(), String> {
    let prefs = {
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.show_balance = visible;
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())
}

/// Pick the two channels the balance slider blends.
#[tauri::command]
pub fn set_balance_channels(
    state: State<'_, AppState>,
    a: Option<String>,
    b: Option<String>,
) -> Result<(), String> {
    let prefs = {
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.balance_a = a;
        mixer.prefs.balance_b = b;
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())
}

/// Mark the first-run tutorial as completed (never shown again, until a
/// factory reset).
#[tauri::command]
pub fn set_onboarded(state: State<'_, AppState>) -> Result<(), String> {
    let prefs = {
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.onboarded = true;
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())
}

/// Factory reset: tear down our audio nodes, wipe every saved file, undo
/// autostart, and relaunch as if freshly installed.
#[tauri::command]
pub fn reset_app(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    // Best-effort teardown — the relaunch recreates everything anyway.
    for err in state.teardown_virtual_sinks() {
        eprintln!("sink: reset teardown: {err}");
    }
    let _ = autostart::disable();
    crate::persistence::wipe_all().map_err(|e| e.to_string())?;
    app.restart()
}

#[derive(Debug, Clone, Serialize)]
pub struct DefaultDevices {
    pub output: Option<String>,
    pub input: Option<String>,
}

/// Current system default output/input device node names.
#[tauri::command]
pub fn get_default_devices(state: State<'_, AppState>) -> Result<DefaultDevices, String> {
    let (output, input) = state
        .backend
        .get_default_devices()
        .map_err(|e| e.to_string())?;
    Ok(DefaultDevices { output, input })
}

/// Set the system default output device.
#[tauri::command]
pub fn set_default_output(state: State<'_, AppState>, name: String) -> Result<(), String> {
    state
        .backend
        .set_default_output(&name)
        .map_err(|e| e.to_string())
}

/// Set the system default input device.
#[tauri::command]
pub fn set_default_input(state: State<'_, AppState>, name: String) -> Result<(), String> {
    state
        .backend
        .set_default_input(&name)
        .map_err(|e| e.to_string())
}
