use std::collections::HashMap;

use serde::Serialize;
use tauri::State;

use crate::audio::eq_import::parse_autoeq;
use crate::audio::presets::{bundled_presets, EqPreset, PRESET_SCHEMA};
use crate::audio::types::EqConfig;
use crate::persistence::eq_presets;
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

#[derive(Debug, Clone, Serialize)]
pub struct EqPresetEntry {
    /// "bundled" (ships in the binary) or "user" (local library).
    pub source: String,
    pub preset: EqPreset,
}

/// Bundled presets first, then the user's library, each sorted by name.
#[tauri::command]
pub fn list_eq_presets() -> Result<Vec<EqPresetEntry>, String> {
    let mut entries: Vec<EqPresetEntry> = bundled_presets()
        .into_iter()
        .map(|preset| EqPresetEntry {
            source: "bundled".into(),
            preset,
        })
        .collect();
    entries.extend(
        eq_presets::list()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|preset| EqPresetEntry {
                source: "user".into(),
                preset,
            }),
    );
    Ok(entries)
}

/// Save the given config into the user's local preset library.
#[tauri::command]
pub fn save_user_eq_preset(name: String, mut config: EqConfig) -> Result<(), String> {
    config.clamp_ranges();
    let preset = EqPreset {
        schema: PRESET_SCHEMA,
        name,
        author: None,
        description: None,
        preamp_db: config.preamp_db,
        bands: config.bands,
    };
    eq_presets::save(&preset).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_user_eq_preset(name: String) -> Result<(), String> {
    eq_presets::delete(&name).map_err(|e| e.to_string())
}

/// A channel's EQ as shareable preset JSON (pretty-printed, schema 1).
#[tauri::command]
pub fn export_channel_eq(state: State<'_, AppState>, sink_name: String) -> Result<String, String> {
    let (config, label) = {
        let mixer = state.lock_mixer()?;
        let label = mixer
            .channels
            .iter()
            .find(|c| c.name == sink_name)
            .map(|c| c.label.clone())
            .unwrap_or_else(|| sink_name.clone());
        (mixer.eq.get(&sink_name), label)
    };
    let preset = EqPreset {
        schema: PRESET_SCHEMA,
        name: label,
        author: None,
        description: None,
        preamp_db: config.preamp_db,
        bands: config.bands,
    };
    serde_json::to_string_pretty(&preset).map_err(|e| e.to_string())
}

/// Write a channel's EQ preset JSON to `path` (picked via the native save
/// dialog; the file I/O stays in Rust so no fs plugin scope is needed).
#[tauri::command]
pub fn export_channel_eq_to_file(
    state: State<'_, AppState>,
    sink_name: String,
    path: String,
) -> Result<(), String> {
    let json = export_channel_eq(state, sink_name)?;
    std::fs::write(&path, json).map_err(|e| format!("write {path}: {e}"))
}

/// Parse pasted preset text: our JSON schema or an AutoEq result block.
/// Returns a disabled config for preview-then-apply in the modal.
#[tauri::command]
pub fn import_eq_config(text: String) -> Result<EqConfig, String> {
    let trimmed = text.trim();
    if trimmed.starts_with('{') {
        let preset: EqPreset = serde_json::from_str(trimmed).map_err(|e| e.to_string())?;
        if preset.schema != PRESET_SCHEMA {
            return Err(format!("unsupported preset schema {}", preset.schema));
        }
        if preset.bands.is_empty() {
            return Err("preset has no bands".into());
        }
        let mut config = preset.to_config();
        config.enabled = false;
        Ok(config)
    } else {
        parse_autoeq(trimmed).map_err(|e| e.to_string())
    }
}

/// Read + parse a preset file picked via the native open dialog.
#[tauri::command]
pub fn import_eq_file(path: String) -> Result<EqConfig, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("read {path}: {e}"))?;
    import_eq_config(text)
}
