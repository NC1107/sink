//! AutoEq text-format import. AutoEq (the community headphone-correction
//! project) publishes parametric EQs as plain text:
//!
//! ```text
//! Preamp: -6.0 dB
//! Filter 1: ON PK Fc 105 Hz Gain -2.4 dB Q 0.70
//! ```
//!
//! Tolerant token parsing, no regex: disabled and unrecognized filter
//! lines are skipped (a partial import beats a hard failure over one
//! exotic filter type); it errors only when nothing usable remains.

use crate::audio::types::{EqBand, EqBandKind, EqConfig, MAX_EQ_BANDS};
use crate::error::SinkError;

fn kind_from_token(token: &str) -> Option<EqBandKind> {
    match token {
        "PK" | "PEQ" | "Modal" => Some(EqBandKind::Peaking),
        "LS" | "LSC" => Some(EqBandKind::LowShelf),
        "HS" | "HSC" => Some(EqBandKind::HighShelf),
        "LP" | "LPQ" => Some(EqBandKind::LowPass),
        "HP" | "HPQ" => Some(EqBandKind::HighPass),
        _ => None,
    }
}

/// Value following a `label` token (e.g. "Fc" -> 105.0 from "Fc 105 Hz").
fn value_after(tokens: &[&str], label: &str) -> Option<f32> {
    tokens
        .iter()
        .position(|t| t.eq_ignore_ascii_case(label))
        .and_then(|i| tokens.get(i + 1))
        .and_then(|v| v.parse().ok())
}

fn parse_filter_line(line: &str) -> Option<EqBand> {
    // "Filter N: ON PK Fc 105 Hz Gain -2.4 dB Q 0.70"
    let rest = line.split(':').nth(1)?.trim();
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    if tokens.first().map(|t| t.eq_ignore_ascii_case("ON")) != Some(true) {
        return None; // disabled ("OFF") or malformed
    }
    let kind = kind_from_token(tokens.get(1)?)?;
    let freq_hz = value_after(&tokens, "Fc")?;
    let gain_db = value_after(&tokens, "Gain").unwrap_or(0.0);
    // Shelf lines in AutoEq's fixed-band output often omit Q.
    let q = value_after(&tokens, "Q").unwrap_or(match kind {
        EqBandKind::LowShelf | EqBandKind::HighShelf => 0.71,
        _ => 1.0,
    });
    let mut band = EqBand {
        kind,
        freq_hz,
        gain_db,
        q,
    };
    band.clamp_ranges();
    Some(band)
}

/// Parse an AutoEq result block into a (disabled, preview-ready) EqConfig.
/// Keeps the first MAX_EQ_BANDS filters in file order — AutoEq emits them
/// in descending importance already.
pub fn parse_autoeq(text: &str) -> Result<EqConfig, SinkError> {
    let mut preamp_db = 0.0f32;
    let mut bands: Vec<EqBand> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.to_ascii_lowercase().starts_with("preamp") {
            if let Some(v) = line
                .split(':')
                .nth(1)
                .and_then(|rest| rest.split_whitespace().next())
                .and_then(|v| v.parse::<f32>().ok())
            {
                preamp_db = v;
            }
        } else if line.to_ascii_lowercase().starts_with("filter") && bands.len() < MAX_EQ_BANDS {
            if let Some(band) = parse_filter_line(line) {
                bands.push(band);
            }
        }
    }
    if bands.is_empty() {
        return Err(SinkError::Parse(
            "no usable filters found (expected AutoEq lines like \
             'Filter 1: ON PK Fc 105 Hz Gain -2.4 dB Q 0.70')"
                .into(),
        ));
    }
    let mut config = EqConfig {
        enabled: false,
        preamp_db,
        bands,
    };
    config.clamp_ranges();
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_preamp_and_peaking_filter() {
        let config = parse_autoeq(
            "Preamp: -6.0 dB\nFilter 1: ON PK Fc 105 Hz Gain -2.4 dB Q 0.70\n",
        )
        .expect("parses");
        assert_eq!(config.preamp_db, -6.0);
        assert_eq!(config.bands.len(), 1);
        let b = &config.bands[0];
        assert_eq!(b.kind, EqBandKind::Peaking);
        assert_eq!(b.freq_hz, 105.0);
        assert_eq!(b.gain_db, -2.4);
        assert_eq!(b.q, 0.7);
        assert!(!config.enabled, "imports preview disabled");
    }

    #[test]
    fn skips_disabled_filters() {
        let config = parse_autoeq(
            "Filter 1: OFF PK Fc 100 Hz Gain 3.0 dB Q 1.0\n\
             Filter 2: ON PK Fc 200 Hz Gain 1.0 dB Q 1.0\n",
        )
        .expect("parses");
        assert_eq!(config.bands.len(), 1);
        assert_eq!(config.bands[0].freq_hz, 200.0);
    }

    #[test]
    fn maps_shelf_and_pass_abbreviations() {
        let config = parse_autoeq(
            "Filter 1: ON LSC Fc 105 Hz Gain 2.0 dB\n\
             Filter 2: ON HSC Fc 10000 Hz Gain -1.0 dB Q 0.71\n\
             Filter 3: ON HP Fc 80 Hz\n",
        )
        .expect("parses");
        assert_eq!(config.bands[0].kind, EqBandKind::LowShelf);
        assert_eq!(config.bands[0].q, 0.71, "shelf without Q gets the slope default");
        assert_eq!(config.bands[1].kind, EqBandKind::HighShelf);
        assert_eq!(config.bands[2].kind, EqBandKind::HighPass);
    }

    #[test]
    fn skips_unknown_filter_kinds() {
        let config = parse_autoeq(
            "Filter 1: ON XYZ Fc 100 Hz Gain 3.0 dB Q 1.0\n\
             Filter 2: ON PK Fc 200 Hz Gain 1.0 dB Q 1.0\n",
        )
        .expect("parses");
        assert_eq!(config.bands.len(), 1);
    }

    #[test]
    fn caps_at_max_bands_keeping_file_order() {
        let mut text = String::from("Preamp: -1.0 dB\n");
        for i in 1..=14 {
            text.push_str(&format!(
                "Filter {i}: ON PK Fc {} Hz Gain 1.0 dB Q 1.0\n",
                i * 100
            ));
        }
        let config = parse_autoeq(&text).expect("parses");
        assert_eq!(config.bands.len(), MAX_EQ_BANDS);
        assert_eq!(config.bands[0].freq_hz, 100.0, "first filters win");
        assert_eq!(config.bands[9].freq_hz, 1000.0);
    }

    #[test]
    fn errors_on_text_with_no_usable_filters() {
        assert!(parse_autoeq("hello world").is_err());
        assert!(parse_autoeq("Preamp: -3.0 dB\n").is_err());
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let config = parse_autoeq(
            "Preamp: -80.0 dB\nFilter 1: ON PK Fc 99999 Hz Gain 80 dB Q 900\n",
        )
        .expect("parses");
        assert_eq!(config.preamp_db, -24.0);
        assert_eq!(config.bands[0].freq_hz, 20000.0);
        assert_eq!(config.bands[0].gain_db, 24.0);
        assert_eq!(config.bands[0].q, 10.0);
    }
}
