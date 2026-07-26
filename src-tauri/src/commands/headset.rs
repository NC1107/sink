//! Tauri commands for the Arctis Nova Pro Wireless base station.
//!
//! Setters apply live to the device; the frontend calls [`headset_save`]
//! (debounced) to persist to the headset's own memory, mirroring how Sink
//! debounces audio faders.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use tauri::State;

use crate::headset::clips;
use crate::headset::hidraw::DeviceKind;
use crate::headset::media;
use crate::headset::oled_controller::{OledCommand, OledContent};
use crate::headset::oled_rotation::ModeId;
use crate::headset::protocol::{self, AncMode, HeadsetStatus, LineOut};
use crate::headset::protocol_apw;
use crate::state::AppState;

/// Presence + latest decoded status, for the initial UI paint.
#[derive(Serialize)]
pub struct HeadsetSnapshot {
    pub connected: bool,
    pub status: HeadsetStatus,
    /// Whether video playback is available (ffmpeg on PATH).
    pub video_supported: bool,
    /// Connected model name, e.g. "Arctis Nova Pro Wireless".
    pub model: Option<String>,
    /// False on models without a driveable OLED (e.g. Arctis Pro Wireless),
    /// so the UI can hide display features instead of failing at runtime.
    pub has_oled: bool,
}

#[tauri::command]
pub fn get_headset_status(state: State<'_, AppState>) -> HeadsetSnapshot {
    HeadsetSnapshot {
        connected: state.headset.is_connected(),
        status: state.headset.status(),
        video_supported: media::ffmpeg_available(),
        model: state.headset.model_name().map(str::to_owned),
        has_oled: state.headset.kind().map(|k| k.has_oled()).unwrap_or(false),
    }
}

// --- device settings ----------------------------------------------------

/// Sidetone. The UI works in the Nova generation's 0..3 steps; the Arctis Pro
/// generation takes 0..9, so scale across when that model is connected.
#[tauri::command]
pub fn headset_set_sidetone(state: State<'_, AppState>, level: u8) -> Result<(), String> {
    match state.headset.kind() {
        Some(DeviceKind::ArctisPro) => {
            let scaled = (level.min(3) as u16 * 9 / 3) as u8;
            state.headset.send(&protocol_apw::set_sidetone(scaled))?;
            state.headset.send(&protocol_apw::save())
        }
        _ => state.headset.send(&protocol::set_sidetone(level)),
    }
}

#[tauri::command]
pub fn headset_set_mic_volume(state: State<'_, AppState>, level: u8) -> Result<(), String> {
    state.headset.send(&protocol::set_mic_volume(level))
}

#[tauri::command]
pub fn headset_set_mic_led(state: State<'_, AppState>, level: u8) -> Result<(), String> {
    state.headset.send(&protocol::set_mic_led(level))
}

#[tauri::command]
pub fn headset_set_anc(state: State<'_, AppState>, mode: String) -> Result<(), String> {
    let mode = match mode.as_str() {
        "on" => AncMode::On,
        "transparent" => AncMode::Transparent,
        "off" => AncMode::Off,
        other => return Err(format!("unknown ANC mode: {other}")),
    };
    state.headset.send(&protocol::set_anc(mode))
}

#[tauri::command]
pub fn headset_set_transparency(state: State<'_, AppState>, level: u8) -> Result<(), String> {
    state.headset.send(&protocol::set_transparency_level(level))
}

/// Auto shut-off. The UI passes the Nova generation's step index; the Arctis
/// Pro generation wants minutes (in 10-minute units), so translate.
#[tauri::command]
pub fn headset_set_auto_off(state: State<'_, AppState>, idx: u8) -> Result<(), String> {
    match state.headset.kind() {
        Some(DeviceKind::ArctisPro) => {
            const STEP_MINUTES: [u16; 7] = [0, 1, 5, 10, 15, 30, 60];
            let minutes = STEP_MINUTES[idx.min(6) as usize];
            state.headset.send(&protocol_apw::set_auto_off(minutes))?;
            state.headset.send(&protocol_apw::save())
        }
        _ => state.headset.send(&protocol::set_auto_off(idx)),
    }
}

#[tauri::command]
pub fn headset_set_gain_high(state: State<'_, AppState>, high: bool) -> Result<(), String> {
    state.headset.send(&protocol::set_gain_high(high))
}

#[tauri::command]
pub fn headset_set_wireless_range(state: State<'_, AppState>, range: bool) -> Result<(), String> {
    state.headset.send(&protocol::set_wireless_range(range))
}

#[tauri::command]
pub fn headset_set_line_out(state: State<'_, AppState>, mode: String) -> Result<(), String> {
    let mode = match mode.as_str() {
        "speaker" => LineOut::Speaker,
        "stream" => LineOut::Stream,
        other => return Err(format!("unknown line-out mode: {other}")),
    };
    state.headset.send(&protocol::set_line_out(mode))
}

#[tauri::command]
pub fn headset_set_line_out_volumes(
    state: State<'_, AppState>,
    left: u8,
    right: u8,
    aux: u8,
) -> Result<(), String> {
    state
        .headset
        .send(&protocol::set_line_out_volumes(left, right, aux))
}

/// Write the 10-band hardware EQ (values in dB, ±10). Selects the custom
/// preset slot first, as the device requires.
#[tauri::command]
pub fn headset_set_eq_bands(state: State<'_, AppState>, bands: Vec<f32>) -> Result<(), String> {
    state.headset.send(&protocol::select_custom_eq())?;
    state.headset.send(&protocol::set_eq_bands(&bands))
}

#[tauri::command]
pub fn headset_set_eq_preset(state: State<'_, AppState>, preset: u8) -> Result<(), String> {
    state.headset.send(&protocol::set_eq_preset(preset))
}

/// The bundled hardware-EQ preset library (name, category, 10 band gains).
#[tauri::command]
pub fn headset_eq_presets() -> Vec<crate::headset::eq_presets::EqPreset> {
    crate::headset::eq_presets::PRESETS.to_vec()
}

/// Apply a bundled preset by name to the device's custom EQ slot.
#[tauri::command]
pub fn headset_apply_eq_preset(
    state: State<'_, AppState>,
    name: String,
) -> Result<Vec<f32>, String> {
    let preset = crate::headset::eq_presets::find(&name)
        .ok_or_else(|| format!("unknown preset: {name}"))?;
    state.headset.send(&protocol::select_custom_eq())?;
    state.headset.send(&protocol::set_eq_bands(&preset.bands))?;
    Ok(preset.bands.to_vec())
}

/// Persist current settings to the headset's own memory.
#[tauri::command]
pub fn headset_save(state: State<'_, AppState>) -> Result<(), String> {
    state.headset.send(&protocol::save())
}

/// Whether the WirePlumber anti-crackle headroom quirk is installed.
#[tauri::command]
pub fn headset_get_alsa_headroom() -> bool {
    crate::persistence::alsa_headroom::is_enabled()
}

/// Install/remove the WirePlumber headroom quirk (restarts WirePlumber).
#[tauri::command]
pub fn headset_set_alsa_headroom(enabled: bool) -> Result<(), String> {
    crate::persistence::alsa_headroom::set_enabled(enabled).map_err(|e| e.to_string())
}

// --- OLED ---------------------------------------------------------------

#[tauri::command]
pub fn headset_oled_text(state: State<'_, AppState>, lines: Vec<String>) -> Result<(), String> {
    state
        .headset
        .oled(OledCommand::Set(OledContent::Text(lines)))
}

#[tauri::command]
pub fn headset_oled_status(state: State<'_, AppState>) -> Result<(), String> {
    state.headset.oled(OledCommand::Set(OledContent::Mode(ModeId::Headset)))
}

/// One selectable OLED live mode.
#[derive(Serialize)]
pub struct ModeEntry {
    pub id: String,
    pub label: String,
    pub category: String,
}

/// Every live mode the OLED can show, for the picker UI.
#[tauri::command]
pub fn headset_oled_modes() -> Vec<ModeEntry> {
    crate::headset::oled_rotation::ALL_MODES
        .iter()
        .map(|m| ModeEntry {
            id: m.as_str().to_string(),
            label: m.label().to_string(),
            category: m.category().to_string(),
        })
        .collect()
}

/// Show the live system dashboard (CPU / RAM / GPU) on the OLED.
#[tauri::command]
pub fn headset_oled_system(state: State<'_, AppState>) -> Result<(), String> {
    state.headset.oled(OledCommand::Set(OledContent::Mode(ModeId::System)))
}

/// Show now-playing media with an animated equalizer on the OLED.
#[tauri::command]
pub fn headset_oled_now_playing(state: State<'_, AppState>) -> Result<(), String> {
    state.headset.oled(OledCommand::Set(OledContent::Mode(ModeId::NowPlaying)))
}

/// Whether desktop-notification mirroring to the OLED is enabled.
#[tauri::command]
pub fn headset_get_notify_mirror(state: State<'_, AppState>) -> bool {
    state.headset.notify_mirror_running()
        || crate::persistence::notify_mirror::is_enabled()
}

/// Enable/disable mirroring desktop notifications to the OLED.
#[tauri::command]
pub fn headset_set_notify_mirror(state: State<'_, AppState>, enabled: bool) -> Result<(), String> {
    state.headset.set_notify_mirror(enabled)
}

/// How notifications are shown on the OLED: seconds on screen + scroll style.
#[derive(Serialize)]
pub struct NotifyDisplay {
    pub duration_secs: u64,
    pub scroll: crate::persistence::prefs::NotifyScroll,
}

#[tauri::command]
pub fn headset_get_notify_display(state: State<'_, AppState>) -> NotifyDisplay {
    let (duration_secs, scroll) = state.headset.notify_display();
    NotifyDisplay { duration_secs, scroll }
}

#[tauri::command]
pub fn headset_set_notify_display(
    state: State<'_, AppState>,
    duration_secs: u64,
    scroll: crate::persistence::prefs::NotifyScroll,
) -> Result<(), String> {
    state.headset.set_notify_display(duration_secs, scroll)
}

#[tauri::command]
pub fn headset_oled_notify(
    state: State<'_, AppState>,
    lines: Vec<String>,
    duration_ms: u64,
) -> Result<(), String> {
    state.headset.oled(OledCommand::Notify {
        lines,
        duration: Duration::from_millis(duration_ms.clamp(500, 60_000)),
    })
}

/// Decode a media file (image, GIF, or video) and play it on the OLED.
#[tauri::command]
pub fn headset_oled_media(
    state: State<'_, AppState>,
    path: String,
    looping: bool,
) -> Result<(), String> {
    let frames = media::load_media(&PathBuf::from(path))?;
    state.headset.oled_play(frames, looping)
}

/// One selectable built-in animation, as sent to the UI.
#[derive(Serialize)]
pub struct ClipEntry {
    pub id: String,
    pub label: String,
    pub category: String,
}

/// List the built-in procedural animations, grouped by category.
#[tauri::command]
pub fn headset_oled_clips() -> Vec<ClipEntry> {
    clips::BUILTIN_CLIPS
        .iter()
        .map(|c| ClipEntry {
            id: c.id.to_string(),
            label: c.label.to_string(),
            category: c.category.to_string(),
        })
        .collect()
}

/// Play a built-in procedural animation on the OLED.
#[tauri::command]
pub fn headset_oled_clip(state: State<'_, AppState>, name: String) -> Result<(), String> {
    let frames = clips::generate(&name).ok_or_else(|| format!("unknown clip: {name}"))?;
    state.headset.oled_play(frames, true)
}

/// Show any live mode by its identifier.
#[tauri::command]
pub fn headset_oled_mode(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mode = ModeId::from_str(&id).ok_or_else(|| format!("unknown mode: {id}"))?;
    state
        .headset
        .oled(OledCommand::Set(OledContent::Mode(mode)))
}

/// Cycle through `ids` every `secs` seconds. An empty list stops rotating.
#[tauri::command]
pub fn headset_oled_rotate(
    state: State<'_, AppState>,
    ids: Vec<String>,
    secs: u64,
) -> Result<(), String> {
    let modes: Vec<ModeId> = ids.iter().filter_map(|i| ModeId::from_str(i)).collect();
    if modes.is_empty() {
        return state.headset.oled(OledCommand::Set(OledContent::Ui));
    }
    state
        .headset
        .oled(OledCommand::ConfigureRotation { modes, secs })?;
    state.headset.oled(OledCommand::Set(OledContent::Rotate))
}

/// Let the display pick a mode from what the machine is doing.
#[tauri::command]
pub fn headset_oled_auto(state: State<'_, AppState>) -> Result<(), String> {
    state.headset.oled(OledCommand::Set(OledContent::Auto))
}

/// Timer controls for the Timer mode.
#[tauri::command]
pub fn headset_timer_countdown(state: State<'_, AppState>, secs: u64) -> Result<(), String> {
    state.headset.oled(OledCommand::TimerCountdown(secs))?;
    state
        .headset
        .oled(OledCommand::Set(OledContent::Mode(ModeId::Timer)))
}

#[tauri::command]
pub fn headset_timer_stopwatch(state: State<'_, AppState>) -> Result<(), String> {
    state.headset.oled(OledCommand::TimerStopwatch)?;
    state
        .headset
        .oled(OledCommand::Set(OledContent::Mode(ModeId::Timer)))
}

#[tauri::command]
pub fn headset_timer_toggle(state: State<'_, AppState>) -> Result<(), String> {
    state.headset.oled(OledCommand::TimerToggle)
}

#[tauri::command]
pub fn headset_timer_reset(state: State<'_, AppState>) -> Result<(), String> {
    state.headset.oled(OledCommand::TimerReset)
}

#[tauri::command]
pub fn headset_oled_brightness(state: State<'_, AppState>, level: u8) -> Result<(), String> {
    state.headset.oled(OledCommand::Brightness(level))
}

#[tauri::command]
pub fn headset_oled_return_ui(state: State<'_, AppState>) -> Result<(), String> {
    state.headset.oled(OledCommand::Set(OledContent::Ui))
}
