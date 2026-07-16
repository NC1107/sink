use std::collections::HashMap;

use tauri::State;

use crate::audio::types::EqConfig;
use crate::state::AppState;

/// All channels' EQ configs in one round-trip (channels without an entry
/// have never been configured — the frontend treats them as default).
#[tauri::command]
pub fn get_channel_eq_configs(
    state: State<'_, AppState>,
) -> Result<HashMap<String, EqConfig>, String> {
    Ok(state.lock_mixer()?.eq.configs.clone())
}

/// Apply and persist one channel's parametric EQ.
#[tauri::command]
pub fn set_channel_eq(
    state: State<'_, AppState>,
    sink_name: String,
    mut config: EqConfig,
) -> Result<(), String> {
    config.clamp_ranges();
    state
        .backend
        .set_channel_eq(&sink_name, &config)
        .map_err(|e| e.to_string())?;
    let eq = {
        let mut mixer = state.lock_mixer()?;
        mixer.eq.set(&sink_name, config);
        crate::commands::profiles::autosave_active(&mixer);
        mixer.eq.clone()
    };
    eq.save().map_err(|e| e.to_string())
}
