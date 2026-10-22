use std::collections::HashMap;

use serde::Serialize;
use tauri::State;

use crate::audio::eq_import::parse_autoeq;
use crate::audio::presets::{bundled_presets, EqPreset, PresetTarget, PRESET_SCHEMA};
use crate::audio::types::EqConfig;
use crate::persistence::eq_presets;
use crate::state::AppState;

/// All channels' EQ configs in one round-trip (channels without an entry
/// have never been configured - the frontend treats them as default).
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

/// The presets for one editor: bundled first, then the user's library, each
/// sorted by name.
#[tauri::command]
pub fn list_eq_presets(target: PresetTarget) -> Result<Vec<EqPresetEntry>, String> {
    let mut entries: Vec<EqPresetEntry> = bundled_presets(target)
        .into_iter()
        .map(|preset| EqPresetEntry {
            source: "bundled".into(),
            preset,
        })
        .collect();
    entries.extend(
        eq_presets::list(target)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|preset| EqPresetEntry {
                source: "user".into(),
                preset,
            }),
    );
    Ok(entries)
}

fn user_preset(name: String, target: PresetTarget, mut config: EqConfig) -> EqPreset {
    config.clamp_ranges();
    EqPreset {
        schema: PRESET_SCHEMA,
        name,
        target,
        author: None,
        description: None,
        preamp_db: config.preamp_db,
        bands: config.bands,
    }
}

/// Save the given config into the user's local preset library.
#[tauri::command]
pub fn save_user_eq_preset(
    name: String,
    target: PresetTarget,
    config: EqConfig,
) -> Result<(), String> {
    eq_presets::save(&user_preset(name, target, config)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_user_eq_preset(name: String, target: PresetTarget) -> Result<(), String> {
    eq_presets::delete(&name, target).map_err(|e| e.to_string())
}

/// Write the editor's curve as preset JSON to `path` (picked via the native
/// save dialog; the file I/O stays in Rust so no fs plugin scope is needed).
/// It takes the curve itself so channels and the mic export the same way.
#[tauri::command]
pub fn export_eq_to_file(
    name: String,
    target: PresetTarget,
    config: EqConfig,
    path: String,
) -> Result<(), String> {
    let json = serde_json::to_string_pretty(&user_preset(name, target, config))
        .map_err(|e| e.to_string())?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::testing::TempConfig;

    #[test]
    fn exported_curve_imports_back_clamped() {
        let _root = TempConfig::new("eq-export");
        let path = crate::persistence::config_root()
            .unwrap()
            .join("mic-eq.json");
        let mut config = EqConfig {
            preamp_db: -3.0,
            ..EqConfig::default()
        };
        config.bands[0].freq_hz = 99999.0;
        export_eq_to_file(
            "Clean Mic".into(),
            PresetTarget::Mic,
            config,
            path.display().to_string(),
        )
        .unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"name\": \"Clean Mic\""));
        assert!(text.contains("\"target\": \"mic\""));
        let back = import_eq_file(path.display().to_string()).unwrap();
        assert_eq!(back.preamp_db, -3.0);
        assert_eq!(back.bands[0].freq_hz, 20000.0);
        assert_eq!(back.bands.len(), EqConfig::default().bands.len());
    }
}
