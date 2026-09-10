use tauri::State;

use crate::audio::types::{MicConfig, OutputDevice};
use crate::persistence::mic;
use crate::state::AppState;

#[tauri::command]
pub fn get_mic_config(state: State<'_, AppState>) -> Result<MicConfig, String> {
    let mixer = state.lock_mixer()?;
    Ok(mixer.mic.clone())
}

/// Apply and persist the mic chain configuration. The published label is
/// decorated per the device-naming preference at the backend boundary;
/// the stored config stays raw.
#[tauri::command]
pub fn set_mic_config(state: State<'_, AppState>, mut config: MicConfig) -> Result<(), String> {
    config.clamp_ranges();
    let mut applied = config.clone();
    applied.output_label = state.lock_mixer()?.prefs.decorate(&config.output_label);
    state
        .backend
        .set_mic_config(&applied)
        .map_err(|e| e.to_string())?;
    {
        let mut mixer = state.lock_mixer()?;
        mixer.mic = config.clone();
    }
    mic::save(&config).map_err(|e| e.to_string())
}

/// Hardware microphones available as the chain's input.
#[tauri::command]
pub fn get_input_devices(state: State<'_, AppState>) -> Result<Vec<OutputDevice>, String> {
    state
        .backend
        .list_input_devices()
        .map_err(|e| e.to_string())
}
