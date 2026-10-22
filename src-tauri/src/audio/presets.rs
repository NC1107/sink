//! Community EQ presets: a versioned JSON schema, with the "community
//! approved" set living in the repo's `presets/eq/` directory and embedded
//! into the binary at build time (see build.rs).

use serde::{Deserialize, Serialize};

use crate::audio::types::{EqBand, EqConfig};

/// What a preset is tuned for. A headphone curve and a voice curve solve
/// different problems, so each editor only offers its own kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresetTarget {
    /// What you hear: channels. Every preset from before the field existed.
    #[default]
    Output,
    /// What others hear: the mic chain.
    Mic,
}

/// The shareable preset file format (schema 1). Deliberately has no
/// `enabled` flag: applying a preset sets bands + preamp and enables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EqPreset {
    pub schema: u32,
    pub name: String,
    #[serde(default)]
    pub target: PresetTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub preamp_db: f32,
    pub bands: Vec<EqBand>,
}

pub const PRESET_SCHEMA: u32 = 1;

impl EqPreset {
    /// The channel config this preset applies (enabled, clamped).
    pub fn to_config(&self) -> EqConfig {
        let mut config = EqConfig {
            enabled: true,
            preamp_db: self.preamp_db,
            bands: self.bands.clone(),
        };
        config.clamp_ranges();
        config
    }
}

include!(concat!(env!("OUT_DIR"), "/eq_presets_generated.rs"));

/// Parse one bundled source, or explain why it's unusable.
fn parse_bundled(stem: &str, raw: &str) -> Result<EqPreset, String> {
    let preset: EqPreset = serde_json::from_str(raw).map_err(|e| format!("preset {stem}: {e}"))?;
    if preset.schema != PRESET_SCHEMA {
        return Err(format!(
            "preset {stem}: unsupported schema {}",
            preset.schema
        ));
    }
    if preset.bands.is_empty() {
        return Err(format!("preset {stem}: no bands"));
    }
    Ok(preset)
}

/// The bundled presets for `target`, sorted by name. Malformed entries are
/// logged and skipped, so a bad community preset degrades one entry, not the
/// whole menu.
pub fn bundled_presets(target: PresetTarget) -> Vec<EqPreset> {
    let mut presets: Vec<EqPreset> = BUNDLED_EQ_PRESET_SOURCES
        .iter()
        .filter_map(|(stem, raw)| match parse_bundled(stem, raw) {
            Ok(preset) => Some(preset),
            Err(e) => {
                eprintln!("sink: skipping bundled eq {e}");
                None
            }
        })
        .filter(|preset| preset.target == target)
        .collect();
    presets.sort_by(|a, b| a.name.cmp(&b.name));
    presets
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::types::EqBandKind;

    #[test]
    fn bundled_presets_parse_and_sort_by_name() {
        let presets = bundled_presets(PresetTarget::Output);
        assert!(
            presets.iter().any(|p| p.name == "Flat"),
            "the Flat reference preset must ship"
        );
        for target in [PresetTarget::Output, PresetTarget::Mic] {
            let presets = bundled_presets(target);
            let names: Vec<&str> = presets.iter().map(|p| p.name.as_str()).collect();
            let mut sorted = names.clone();
            sorted.sort();
            assert_eq!(names, sorted);
            for p in &presets {
                assert_eq!(p.schema, PRESET_SCHEMA);
                assert_eq!(p.target, target);
                assert!(!p.bands.is_empty());
            }
        }
    }

    #[test]
    fn every_bundled_source_parses() {
        let parsed =
            bundled_presets(PresetTarget::Output).len() + bundled_presets(PresetTarget::Mic).len();
        assert_eq!(parsed, BUNDLED_EQ_PRESET_SOURCES.len());
    }

    #[test]
    fn mic_presets_stay_out_of_the_output_menu() {
        let mic = bundled_presets(PresetTarget::Mic);
        assert!(mic.iter().any(|p| p.name == "Broadcast"));
        let output = bundled_presets(PresetTarget::Output);
        assert!(output.iter().all(|p| p.name != "Broadcast"));
        assert!(mic.iter().all(|p| p.name != "Bass Boost"));
    }

    #[test]
    fn a_preset_without_a_target_is_an_output_preset() {
        let preset = parse_bundled("old", r#"{"schema":1,"name":"x","bands":[{"kind":"peaking","freq_hz":100,"gain_db":1,"q":1}]}"#).unwrap();
        assert_eq!(preset.target, PresetTarget::Output);
        let mic = parse_bundled("new", r#"{"schema":1,"name":"x","target":"mic","bands":[{"kind":"peaking","freq_hz":100,"gain_db":1,"q":1}]}"#).unwrap();
        assert_eq!(mic.target, PresetTarget::Mic);
    }

    #[test]
    fn flat_preset_is_numerically_flat() {
        let presets = bundled_presets(PresetTarget::Output);
        let flat = presets.iter().find(|p| p.name == "Flat").expect("shipped");
        assert_eq!(flat.preamp_db, 0.0);
        assert!(flat.bands.iter().all(|b| b.gain_db == 0.0));
    }

    #[test]
    fn malformed_bundled_entry_is_skipped_not_panicked() {
        assert!(parse_bundled("bad", "{not json").is_err());
        assert!(parse_bundled("bad", r#"{"schema":2,"name":"x","bands":[]}"#).is_err());
        assert!(
            parse_bundled("bad", r#"{"schema":1,"name":"x","bands":[]}"#).is_err(),
            "zero bands is rejected"
        );
    }

    #[test]
    fn to_config_enables_and_clamps() {
        let preset = EqPreset {
            schema: 1,
            name: "Hot".into(),
            target: PresetTarget::Output,
            author: None,
            description: None,
            preamp_db: -99.0,
            bands: vec![EqBand {
                kind: EqBandKind::Peaking,
                freq_hz: 90000.0,
                gain_db: 4.0,
                q: 1.0,
            }],
        };
        let config = preset.to_config();
        assert!(config.enabled);
        assert_eq!(config.preamp_db, -24.0);
        assert_eq!(config.bands[0].freq_hz, 20000.0);
    }
}
