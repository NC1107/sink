//! Tauri commands for the SteelSeries mouse (Aerox 9 Wireless).

use serde::Serialize;
use tauri::State;

use crate::mouse::manager::MouseStatus;
use crate::mouse::protocol::{self, MouseConfig};
use crate::state::AppState;

#[derive(Serialize)]
pub struct MouseSnapshot {
    pub connected: bool,
    pub status: MouseStatus,
    pub config: MouseConfig,
    /// Selectable DPI steps the sensor actually supports.
    pub dpi_steps: Vec<u16>,
    pub polling_rates: Vec<u16>,
}

#[tauri::command]
pub fn get_mouse_status(state: State<'_, AppState>) -> MouseSnapshot {
    MouseSnapshot {
        connected: state.mouse.is_connected(),
        status: state.mouse.status(),
        config: state.mouse.config(),
        dpi_steps: protocol::dpi_steps(),
        polling_rates: protocol::POLLING_RATES.to_vec(),
    }
}

/// Set the DPI preset list and which entry is active, then persist.
#[tauri::command]
pub fn mouse_set_dpi(
    state: State<'_, AppState>,
    presets: Vec<u16>,
    selected: u8,
) -> Result<(), String> {
    state
        .mouse
        .send_with_pid(|pid| protocol::set_dpi(pid, &presets, selected))?;
    let mut cfg = state.mouse.config();
    cfg.dpi_presets = presets;
    cfg.dpi_selected = selected;
    state.mouse.set_config(cfg);
    state.mouse.save()
}

#[tauri::command]
pub fn mouse_set_polling(state: State<'_, AppState>, hz: u16) -> Result<(), String> {
    state.mouse.send_with_pid(|pid| protocol::set_polling(pid, hz))?;
    let mut cfg = state.mouse.config();
    cfg.polling_hz = hz;
    state.mouse.set_config(cfg);
    state.mouse.save()
}

/// Set one lighting zone's colour (0 = top, 1 = middle, 2 = bottom).
#[tauri::command]
pub fn mouse_set_zone_color(
    state: State<'_, AppState>,
    zone: u8,
    r: u8,
    g: u8,
    b: u8,
) -> Result<(), String> {
    state
        .mouse
        .send_with_pid(|pid| protocol::set_zone_color(pid, zone, r, g, b))?;
    let mut cfg = state.mouse.config();
    // Setting a solid colour implicitly turns the rainbow effect off.
    cfg.rainbow = false;
    if let Some(slot) = cfg.zones.get_mut(zone as usize) {
        *slot = [r, g, b];
    }
    state.mouse.set_config(cfg);
    state.mouse.save()
}

/// Enable the rainbow effect across all zones.
#[tauri::command]
pub fn mouse_set_rainbow(state: State<'_, AppState>) -> Result<(), String> {
    state.mouse.send_with_pid(protocol::set_rainbow)?;
    let mut cfg = state.mouse.config();
    cfg.rainbow = true;
    state.mouse.set_config(cfg);
    state.mouse.save()
}

/// Reactive click-flash colour; pass `null` to disable.
#[tauri::command]
pub fn mouse_set_reactive(
    state: State<'_, AppState>,
    rgb: Option<[u8; 3]>,
) -> Result<(), String> {
    let tuple = rgb.map(|c| (c[0], c[1], c[2]));
    state
        .mouse
        .send_with_pid(|pid| protocol::set_reactive(pid, tuple))?;
    let mut cfg = state.mouse.config();
    cfg.reactive = rgb;
    state.mouse.set_config(cfg);
    state.mouse.save()
}

/// Sleep timeout in minutes (0 = never, max 20).
#[tauri::command]
pub fn mouse_set_sleep(state: State<'_, AppState>, minutes: u16) -> Result<(), String> {
    state
        .mouse
        .send_with_pid(|pid| protocol::set_sleep_minutes(pid, minutes))?;
    let mut cfg = state.mouse.config();
    cfg.sleep_minutes = minutes;
    state.mouse.set_config(cfg);
    state.mouse.save()
}

/// Which lighting effects come back after the mouse power-cycles.
#[tauri::command]
pub fn mouse_set_startup_lighting(
    state: State<'_, AppState>,
    rainbow: bool,
    reactive: bool,
) -> Result<(), String> {
    state
        .mouse
        .send_with_pid(|pid| protocol::set_startup_lighting(pid, rainbow, reactive))?;
    state.mouse.save()
}

/// Light-dim timeout in seconds (0 = never, max 1200).
#[tauri::command]
pub fn mouse_set_dim(state: State<'_, AppState>, seconds: u16) -> Result<(), String> {
    state
        .mouse
        .send_with_pid(|pid| protocol::set_dim_seconds(pid, seconds))?;
    let mut cfg = state.mouse.config();
    cfg.dim_seconds = seconds;
    state.mouse.set_config(cfg);
    state.mouse.save()
}
