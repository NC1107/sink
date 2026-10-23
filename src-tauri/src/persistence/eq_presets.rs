//! The user's local EQ preset library: named JSON files (the same schema
//! as the bundled presets) under `$XDG_CONFIG_HOME/sink/eq_presets/`, with
//! mic presets in its `mic/` subfolder so the two kinds can share a name.
//! Modeled on the profiles store, including its name sanitization.

use std::fs;
use std::path::PathBuf;

use crate::audio::presets::{EqPreset, PresetTarget, PRESET_SCHEMA};
use crate::error::SinkError;
use crate::persistence::profiles::sanitize_name;

fn presets_dir(target: PresetTarget) -> Result<PathBuf, SinkError> {
    let dir = crate::persistence::config_root()
        .ok_or_else(|| SinkError::Config("cannot resolve the user config directory".into()))?
        .join("sink")
        .join("eq_presets");
    Ok(match target {
        PresetTarget::Output => dir,
        PresetTarget::Mic => dir.join("mic"),
    })
}

/// The user's presets for `target`, sorted by name. The folder decides the
/// target, so a file dropped into the wrong one shows up where it was put.
/// Unreadable files are skipped (one corrupt preset must not hide the rest).
pub fn list(target: PresetTarget) -> Result<Vec<EqPreset>, SinkError> {
    let dir = presets_dir(target)?;
    let mut presets = Vec::new();
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(presets), // no library yet
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        match fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|raw| serde_json::from_str::<EqPreset>(&raw).map_err(|e| e.to_string()))
        {
            Ok(preset) if preset.schema == PRESET_SCHEMA && !preset.bands.is_empty() => {
                presets.push(EqPreset { target, ..preset });
            }
            Ok(_) => eprintln!("sink: skipping eq preset {}: bad schema", path.display()),
            Err(e) => eprintln!("sink: skipping eq preset {}: {e}", path.display()),
        }
    }
    presets.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(presets)
}

pub fn save(preset: &EqPreset) -> Result<(), SinkError> {
    let name = sanitize_name(&preset.name)?;
    if preset.bands.is_empty() {
        return Err(SinkError::Config("a preset needs at least one band".into()));
    }
    let dir = presets_dir(preset.target)?;
    crate::persistence::ensure_private_dir(&dir)?;
    let json = serde_json::to_string_pretty(preset)
        .map_err(|e| SinkError::Config(format!("serialize eq preset: {e}")))?;
    super::write_atomic(&dir.join(format!("{name}.json")), &json)?;
    Ok(())
}

pub fn delete(name: &str, target: PresetTarget) -> Result<(), SinkError> {
    let name = sanitize_name(name)?;
    let path = presets_dir(target)?.join(format!("{name}.json"));
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()), // idempotent
        Err(e) => Err(SinkError::Io(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traversal_names_are_rejected() {
        // sanitize_name (shared with profiles) is the barrier; pin that it
        // holds for the preset entry points too.
        assert!(delete("../etc/passwd", PresetTarget::Output).is_err());
        assert!(delete("../etc/passwd", PresetTarget::Mic).is_err());
        let preset = EqPreset {
            schema: PRESET_SCHEMA,
            name: "../escape".into(),
            target: PresetTarget::Output,
            author: None,
            description: None,
            preamp_db: 0.0,
            bands: crate::audio::types::default_eq_bands(),
        };
        assert!(save(&preset).is_err());
    }

    #[test]
    fn empty_band_presets_are_rejected() {
        let preset = EqPreset {
            schema: PRESET_SCHEMA,
            name: "Empty".into(),
            target: PresetTarget::Mic,
            author: None,
            description: None,
            preamp_db: 0.0,
            bands: Vec::new(),
        };
        assert!(save(&preset).is_err());
    }

    fn preset(name: &str, target: PresetTarget, gain_db: f32) -> EqPreset {
        let mut bands = crate::audio::types::default_eq_bands();
        bands[0].gain_db = gain_db;
        EqPreset {
            schema: PRESET_SCHEMA,
            name: name.into(),
            target,
            author: None,
            description: None,
            preamp_db: 0.0,
            bands,
        }
    }

    #[test]
    fn mic_and_output_libraries_are_separate() {
        let _root = crate::persistence::testing::TempConfig::new("eq-preset-targets");
        save(&preset("Mine", PresetTarget::Output, 3.0)).unwrap();
        save(&preset("Mine", PresetTarget::Mic, -3.0)).unwrap();

        let output = list(PresetTarget::Output).unwrap();
        let mic = list(PresetTarget::Mic).unwrap();
        assert_eq!(output, vec![preset("Mine", PresetTarget::Output, 3.0)]);
        assert_eq!(mic, vec![preset("Mine", PresetTarget::Mic, -3.0)]);

        delete("Mine", PresetTarget::Mic).unwrap();
        assert!(list(PresetTarget::Mic).unwrap().is_empty());
        assert_eq!(list(PresetTarget::Output).unwrap().len(), 1);
    }

    #[test]
    fn presets_saved_before_targets_existed_list_as_output() {
        let _root = crate::persistence::testing::TempConfig::new("eq-preset-legacy");
        let dir = presets_dir(PresetTarget::Output).unwrap();
        fs::create_dir_all(&dir).unwrap();
        let mut legacy = serde_json::to_value(preset("Old", PresetTarget::Output, 2.0)).unwrap();
        legacy.as_object_mut().unwrap().remove("target");
        fs::write(dir.join("Old.json"), legacy.to_string()).unwrap();

        assert_eq!(list(PresetTarget::Output).unwrap()[0].name, "Old");
        assert!(list(PresetTarget::Mic).unwrap().is_empty());
    }
}
