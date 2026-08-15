use tauri::State;

use crate::audio::backend::AudioBackend;
use crate::commands::routing::MAX_VOLUME;
use crate::persistence::buses::{is_bus_name, BusDef};
use crate::state::AppState;

/// Re-apply a mix's persisted volume/mute to its node. Bus nodes are born at
/// unity/unmuted, so this restores the saved level after create/rename/load.
/// Best-effort: a routing failure shouldn't abort bringing the mix up.
pub(crate) fn apply_bus_level(backend: &dyn AudioBackend, def: &BusDef) {
    if def.volume_percent != 100 {
        let _ = backend.set_sink_volume(&def.name, def.volume_percent);
    }
    if def.muted {
        let _ = backend.set_sink_mute(&def.name, true);
    }
}

/// Re-apply a mix's persisted per-member send levels to its node. Same
/// rationale as `apply_bus_level`: inserts don't exist until a member's
/// gain is set at least once, so a fresh/recreated bus needs this to
/// restore any non-unity faders.
pub(crate) fn apply_bus_member_gains(backend: &dyn AudioBackend, def: &BusDef) {
    for (member, percent) in &def.member_gains {
        let _ = backend.set_bus_member_gain(&def.name, member, *percent);
    }
}

/// The user's mixes (buses) with their member channels.
#[tauri::command]
pub fn list_buses(state: State<'_, AppState>) -> Result<Vec<BusDef>, String> {
    let mixer = state.lock_mixer()?;
    Ok(mixer.buses.buses.clone())
}

/// Create a new mix. Recorders see it under `label`. New mixes carry
/// every channel (auto-include) until the user unchecks some.
#[tauri::command]
pub fn add_bus(state: State<'_, AppState>, label: String) -> Result<(), String> {
    let (def, defs, prefs, all) = {
        let mut mixer = state.lock_mixer()?;
        let def = mixer.buses.add(&label).map_err(|e| e.to_string())?;
        (
            def,
            mixer.buses.clone(),
            mixer.prefs.clone(),
            channel_names(&mixer),
        )
    };
    if let Err(e) = state.backend.create_bus(&def.name, &prefs.decorate(&def.label)) {
        let mut mixer = state.lock_mixer()?;
        let _ = mixer.buses.remove(&def.name);
        return Err(e.to_string());
    }
    if let Err(e) = state
        .backend
        .set_bus_members(&def.name, &def.effective_members(&all), def.mic)
    {
        eprintln!("sink: members for new mix {} failed: {e}", def.name);
    }
    defs.save().map_err(|e| e.to_string())?;
    let mixer = state.lock_mixer()?;
    crate::commands::profiles::autosave_active(&mixer);
    Ok(())
}

/// Rename a mix. The node is recreated so recorders immediately see the
/// new name (the node name stays stable, so OBS configs keep working -
/// capture re-attaches automatically).
#[tauri::command]
pub fn rename_bus(state: State<'_, AppState>, name: String, label: String) -> Result<(), String> {
    let (def, defs, prefs, all) = {
        let mut mixer = state.lock_mixer()?;
        mixer.buses.rename(&name, &label).map_err(|e| e.to_string())?;
        let def = mixer
            .buses
            .get(&name)
            .cloned()
            .ok_or_else(|| "unknown mix".to_string())?;
        (
            def,
            mixer.buses.clone(),
            mixer.prefs.clone(),
            channel_names(&mixer),
        )
    };

    state.backend.destroy_bus(&name).map_err(|e| e.to_string())?;
    state
        .backend
        .create_bus(&def.name, &prefs.decorate(&def.label))
        .map_err(|e| e.to_string())?;
    state
        .backend
        .set_bus_members(&def.name, &def.effective_members(&all), def.mic)
        .map_err(|e| e.to_string())?;
    // The node was recreated fresh; restore this mix's saved level and any
    // per-member send gains.
    apply_bus_level(state.backend.as_ref(), &def);
    apply_bus_member_gains(state.backend.as_ref(), &def);

    defs.save().map_err(|e| e.to_string())?;
    let mixer = state.lock_mixer()?;
    crate::commands::profiles::autosave_active(&mixer);
    Ok(())
}

/// Delete a mix.
#[tauri::command]
pub fn remove_bus(state: State<'_, AppState>, name: String) -> Result<(), String> {
    state.backend.destroy_bus(&name).map_err(|e| e.to_string())?;
    let defs = {
        let mut mixer = state.lock_mixer()?;
        mixer.buses.remove(&name).map_err(|e| e.to_string())?;
        crate::commands::profiles::autosave_active(&mixer);
        mixer.buses.clone()
    };
    defs.save().map_err(|e| e.to_string())
}

/// Replace the channel set a mix carries. `channels` is what the user
/// sees checked; for auto-include mixes the complement (the unchecked
/// set) is what gets stored, so future channels keep flowing in.
#[tauri::command]
pub fn set_bus_members(
    state: State<'_, AppState>,
    name: String,
    channels: Vec<String>,
) -> Result<(), String> {
    // Validate against the definition set first, so a rejected request
    // (master mix, unknown name) never reaches the backend - otherwise
    // backend membership and the persisted definition could diverge.
    let (stored, mic) = {
        let mixer = state.lock_mixer()?;
        if crate::persistence::buses::is_master(&name) {
            return Err("the master mix always carries every channel".to_string());
        }
        let Some(def) = mixer.buses.get(&name) else {
            return Err("unknown mix".to_string());
        };
        let stored = if def.exclude {
            channel_names(&mixer)
                .into_iter()
                .filter(|c| !channels.contains(c))
                .collect()
        } else {
            channels.clone()
        };
        (stored, def.mic)
    };
    state
        .backend
        .set_bus_members(&name, &channels, mic)
        .map_err(|e| e.to_string())?;
    let defs = {
        let mut mixer = state.lock_mixer()?;
        mixer
            .buses
            .set_members(&name, stored)
            .map_err(|e| e.to_string())?;
        crate::commands::profiles::autosave_active(&mixer);
        mixer.buses.clone()
    };
    defs.save().map_err(|e| e.to_string())
}

/// Include (or drop) the processed virtual mic as a member of a mix,
/// alongside its channels - lets a Stream Mix carry your voice plus
/// game/media audio, selectable as one input device in Discord/OBS.
#[tauri::command]
pub fn set_bus_mic(state: State<'_, AppState>, name: String, mic: bool) -> Result<(), String> {
    let channels = {
        let mixer = state.lock_mixer()?;
        let Some(def) = mixer.buses.get(&name) else {
            return Err("unknown mix".to_string());
        };
        def.effective_members(&channel_names(&mixer))
    };
    state
        .backend
        .set_bus_members(&name, &channels, mic)
        .map_err(|e| e.to_string())?;
    let defs = {
        let mut mixer = state.lock_mixer()?;
        mixer.buses.set_mic(&name, mic).map_err(|e| e.to_string())?;
        crate::commands::profiles::autosave_active(&mixer);
        mixer.buses.clone()
    };
    defs.save().map_err(|e| e.to_string())
}

/// Set one member's send level within one specific mix (0-150%; 100 = no
/// override, same as the member's own level). Independent of the member's
/// own volume/EQ and of what you hear locally - only this mix's
/// recorders/listeners hear the difference. `member` is a channel sink
/// name, or "sink_mic" for the processed microphone.
#[tauri::command]
pub fn set_bus_member_gain(
    state: State<'_, AppState>,
    bus: String,
    member: String,
    percent: u8,
) -> Result<(), String> {
    if !is_bus_name(&bus) {
        return Err(format!("unknown mix: {bus}"));
    }
    let percent = percent.min(MAX_VOLUME);
    state
        .backend
        .set_bus_member_gain(&bus, &member, percent)
        .map_err(|e| e.to_string())?;
    let defs = {
        let mut mixer = state.lock_mixer()?;
        mixer
            .buses
            .set_member_gain(&bus, &member, percent)
            .map_err(|e| e.to_string())?;
        crate::commands::profiles::autosave_active(&mixer);
        mixer.buses.clone()
    };
    defs.save().map_err(|e| e.to_string())
}

/// Open (or focus, if already open) a small standalone window with the
/// per-member send faders for one mix - meant to be left open alongside
/// the main window while streaming/in a call.
#[tauri::command]
pub fn open_mix_fader_window(app: tauri::AppHandle, bus: String, label: String) -> Result<(), String> {
    use tauri::Manager;

    let window_label = format!("mix-fader-{bus}");
    if let Some(existing) = app.get_webview_window(&window_label) {
        let _ = existing.show();
        let _ = existing.set_focus();
        return Ok(());
    }
    let url = format!("index.html?mixFader={bus}");
    tauri::WebviewWindowBuilder::new(
        &app,
        &window_label,
        tauri::WebviewUrl::App(url.into()),
    )
    .title(format!("{label} \u{2013} Levels"))
    // Frameless custom chrome, matching the main window - the popout's own
    // React tree (MixFaderTitleBar) draws the headerbar and close button.
    .decorations(false)
    .transparent(true)
    // Roomy by default: a full-travel fader (matching the main board's
    // strips) plus its readout and mute row, with space for a second
    // member's strip beside it. Scrolls if resized smaller than this.
    .inner_size(700.0, 760.0)
    .min_inner_size(320.0, 440.0)
    .resizable(true)
    .build()
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Switch a mix between manual selection and auto-include mode. The
/// carried set is preserved; only what happens to future channels changes.
#[tauri::command]
pub fn set_bus_exclude(
    state: State<'_, AppState>,
    name: String,
    exclude: bool,
) -> Result<(), String> {
    let defs = {
        let mut mixer = state.lock_mixer()?;
        let all = channel_names(&mixer);
        mixer
            .buses
            .set_exclude(&name, exclude, &all)
            .map_err(|e| e.to_string())?;
        crate::commands::profiles::autosave_active(&mixer);
        mixer.buses.clone()
    };
    defs.save().map_err(|e| e.to_string())
}

/// Set a mix's playback level (0-150%) - what recorders hear. Unlike
/// `set_channel_volume`, this accepts mix nodes (including the master mix,
/// whose reserved name `set_channel_volume` rejects) and persists the level.
#[tauri::command]
pub fn set_bus_volume(state: State<'_, AppState>, name: String, volume: u8) -> Result<(), String> {
    if !is_bus_name(&name) {
        return Err(format!("unknown mix: {name}"));
    }
    let volume = volume.min(MAX_VOLUME);
    state
        .backend
        .set_sink_volume(&name, volume)
        .map_err(|e| e.to_string())?;
    let defs = {
        let mut mixer = state.lock_mixer()?;
        mixer.buses.set_volume(&name, volume).map_err(|e| e.to_string())?;
        crate::commands::profiles::autosave_active(&mixer);
        mixer.buses.clone()
    };
    defs.save().map_err(|e| e.to_string())
}

/// Mute or unmute a mix for recorders. Persisted, and accepts the master mix.
#[tauri::command]
pub fn set_bus_mute(state: State<'_, AppState>, name: String, muted: bool) -> Result<(), String> {
    if !is_bus_name(&name) {
        return Err(format!("unknown mix: {name}"));
    }
    state
        .backend
        .set_sink_mute(&name, muted)
        .map_err(|e| e.to_string())?;
    let defs = {
        let mut mixer = state.lock_mixer()?;
        mixer.buses.set_muted(&name, muted).map_err(|e| e.to_string())?;
        crate::commands::profiles::autosave_active(&mixer);
        mixer.buses.clone()
    };
    defs.save().map_err(|e| e.to_string())
}

/// The current channel sink names (the "all channels" set for mixes).
pub(crate) fn channel_names(mixer: &crate::mixer::state::MixerState) -> Vec<String> {
    mixer.channels.iter().map(|c| c.name.clone()).collect()
}
