//! Curated 10-band curves for the headset's *hardware* equalizer.
//!
//! Bands are fixed at 31, 62, 125, 250, 500, 1k, 2k, 4k, 8k and 16k Hz, and
//! each gain is dB in the device's ±10 range (encoded in 0.5 dB units by
//! [`super::protocol::set_eq_bands`]).
//!
//! Curves avoid stacking boosts across the whole spectrum: where the low end
//! is pushed hard, the low-mids are pulled back instead of lifting everything,
//! which keeps headroom and stops the mix turning to mud.

use serde::Serialize;

/// Centre frequency labels, matching the band order used everywhere.
/// (The UI carries its own copy; kept here as the canonical definition.)
#[allow(dead_code)]
pub const BAND_LABELS: [&str; 10] = [
    "31", "62", "125", "250", "500", "1k", "2k", "4k", "8k", "16k",
];

#[derive(Serialize, Clone)]
pub struct EqPreset {
    pub name: &'static str,
    pub category: &'static str,
    pub bands: [f32; 10],
    pub description: &'static str,
}

/// The bundled preset library, grouped by category for the picker.
pub const PRESETS: &[EqPreset] = &[
    // ---- Utility ----
    EqPreset {
        name: "Flat / Reference",
        category: "Utility",
        bands: [0.0; 10],
        description: "No colouration — the device's own tuning.",
    },
    EqPreset {
        name: "Bass Boost",
        category: "Utility",
        bands: [6.0, 5.5, 4.0, 1.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        description: "Clean low-end lift that leaves the mids and highs alone.",
    },
    EqPreset {
        name: "Treble Boost",
        category: "Utility",
        bands: [0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 2.0, 3.5, 5.0, 6.0],
        description: "Rising top end for air and detail.",
    },
    EqPreset {
        name: "V-Shape / Loudness",
        category: "Utility",
        bands: [5.0, 4.0, 2.0, -1.0, -2.0, -1.5, 0.0, 2.0, 4.0, 5.0],
        description: "Scooped mids with lifted extremes — the classic fun curve.",
    },
    EqPreset {
        name: "Warm",
        category: "Utility",
        bands: [3.0, 3.0, 2.0, 1.5, 0.5, 0.0, -1.0, -1.5, -2.0, -2.0],
        description: "Full lows and softened highs for long, fatigue-free sessions.",
    },
    EqPreset {
        name: "Bright",
        category: "Utility",
        bands: [-1.0, -1.0, -0.5, 0.0, 0.5, 1.0, 2.0, 3.0, 4.0, 4.5],
        description: "Tilts energy upward for clarity on dark-sounding sources.",
    },
    // ---- Music ----
    EqPreset {
        name: "Hardstyle",
        category: "Music",
        bands: [4.0, 5.5, 4.0, -2.0, -3.0, -1.5, 1.0, 3.0, 3.5, 2.5],
        description: "Hard kick punch with scooped low-mids and a crisp screech lead.",
    },
    EqPreset {
        name: "Hardcore / Uptempo",
        category: "Music",
        bands: [3.5, 5.0, 3.0, -2.5, -3.0, -1.0, 1.5, 3.5, 4.0, 3.0],
        description: "Fast distorted kicks kept tight, with aggressive presence up top.",
    },
    EqPreset {
        name: "EDM / Festival",
        category: "Music",
        bands: [4.0, 4.0, 2.0, -1.0, -2.0, -1.0, 1.0, 3.0, 4.0, 3.5],
        description: "Big-room weight and sparkle without clogging the middle.",
    },
    EqPreset {
        name: "Drum & Bass",
        category: "Music",
        bands: [5.0, 4.5, 1.5, -1.5, -1.5, 0.0, 1.5, 2.5, 3.0, 2.5],
        description: "Deep sub weight with snappy breaks.",
    },
    EqPreset {
        name: "Dubstep",
        category: "Music",
        bands: [5.5, 5.0, 2.0, -2.0, -2.5, -1.0, 1.0, 2.5, 3.0, 2.0],
        description: "Maximum sub with room for the wobble to cut.",
    },
    EqPreset {
        name: "Techno",
        category: "Music",
        bands: [3.5, 4.0, 2.0, -1.0, -1.0, 0.0, 0.5, 1.5, 2.0, 1.5],
        description: "Driving low end, restrained elsewhere — keeps the groove.",
    },
    EqPreset {
        name: "Trance",
        category: "Music",
        bands: [3.0, 3.0, 1.0, -0.5, -0.5, 0.0, 1.0, 2.5, 3.5, 3.5],
        description: "Warm foundation with shimmering supersaws and pads.",
    },
    EqPreset {
        name: "Hip-Hop / Trap",
        category: "Music",
        bands: [5.0, 4.5, 2.5, -1.0, -1.5, -0.5, 0.5, 2.0, 2.5, 2.0],
        description: "808 weight while vocals stay forward.",
    },
    EqPreset {
        name: "Rock / Metal",
        category: "Music",
        bands: [2.0, 2.5, 1.0, -0.5, -1.0, 0.5, 2.0, 2.5, 2.0, 1.5],
        description: "Guitar bite and kick punch without harshness.",
    },
    EqPreset {
        name: "Pop",
        category: "Music",
        bands: [2.5, 2.5, 1.0, 0.0, -0.5, 0.5, 1.5, 2.0, 2.5, 2.0],
        description: "Gentle smile curve that flatters modern masters.",
    },
    EqPreset {
        name: "Jazz",
        category: "Music",
        bands: [2.0, 2.0, 1.0, 0.5, 0.0, 0.0, 0.5, 1.0, 1.5, 1.5],
        description: "Natural tone with a touch of warmth and air.",
    },
    EqPreset {
        name: "Classical",
        category: "Music",
        bands: [1.5, 1.5, 0.5, 0.0, 0.0, 0.0, 0.5, 1.0, 1.5, 2.0],
        description: "Near-neutral, preserving dynamics and hall detail.",
    },
    EqPreset {
        name: "Acoustic",
        category: "Music",
        bands: [1.5, 2.0, 1.0, 0.0, 0.0, 0.5, 1.5, 2.0, 2.0, 1.5],
        description: "Body for the guitar plus string detail.",
    },
    EqPreset {
        name: "Lo-Fi",
        category: "Music",
        bands: [2.5, 3.0, 2.0, 1.0, 0.0, -0.5, -1.5, -2.5, -3.0, -3.5],
        description: "Rolled-off highs and thick lows for that tape feel.",
    },
    // ---- Gaming ----
    EqPreset {
        name: "FPS Footsteps",
        category: "Gaming",
        bands: [-3.0, -2.5, -1.5, -1.0, 1.0, 2.0, 4.0, 4.5, 3.5, 1.5],
        description: "Cuts explosions, lifts the band where footsteps and reloads live.",
    },
    EqPreset {
        name: "Competitive / Esports",
        category: "Gaming",
        bands: [-2.5, -2.0, -1.0, 0.0, 1.0, 2.0, 3.5, 4.0, 3.0, 1.5],
        description: "Maximum positional cue clarity, minimum low-end masking.",
    },
    EqPreset {
        name: "Immersive / Cinematic",
        category: "Gaming",
        bands: [3.5, 3.0, 1.5, 0.0, -0.5, 0.0, 1.0, 2.0, 3.0, 3.0],
        description: "Weighty and wide for single-player worlds.",
    },
    EqPreset {
        name: "Racing / Sim",
        category: "Gaming",
        bands: [3.0, 3.0, 1.5, 0.0, -0.5, 0.5, 1.5, 2.0, 2.0, 1.5],
        description: "Engine rumble plus tyre and surface detail.",
    },
    EqPreset {
        name: "Horror / Spatial",
        category: "Gaming",
        bands: [2.0, 1.5, 0.0, -0.5, 0.0, 1.0, 2.0, 3.0, 3.5, 3.0],
        description: "Pulls out faint ambience and directional cues.",
    },
    // ---- Movies ----
    EqPreset {
        name: "Cinematic Movies",
        category: "Movies",
        bands: [4.0, 3.5, 1.5, -0.5, 0.0, 0.5, 1.5, 2.0, 2.5, 2.5],
        description: "Cinema-style low end with intelligible dialogue.",
    },
    EqPreset {
        name: "Dialogue Clarity",
        category: "Movies",
        bands: [-2.0, -1.5, -0.5, 0.5, 1.5, 2.5, 3.0, 2.5, 1.5, 0.0],
        description: "Speech forward, rumble held back.",
    },
    EqPreset {
        name: "Late-Night",
        category: "Movies",
        bands: [-4.0, -3.5, -2.0, 0.0, 1.5, 2.0, 2.0, 1.5, 0.0, -1.5],
        description: "Quiet-hours listening: no bass to travel, speech still clear.",
    },
    // ---- Voice ----
    EqPreset {
        name: "Podcast / Voice",
        category: "Voice",
        bands: [-3.0, -2.5, -1.0, 0.5, 1.5, 2.0, 2.5, 2.0, 1.0, -0.5],
        description: "Optimised for spoken word — no rumble, no sibilance.",
    },
    EqPreset {
        name: "Vocal Presence",
        category: "Voice",
        bands: [-1.0, -1.0, -0.5, 0.5, 1.5, 2.5, 2.5, 1.5, 0.5, 0.0],
        description: "Lifts voices out of a busy mix.",
    },
];

/// Look up a preset by name.
pub fn find(name: &str) -> Option<&'static EqPreset> {
    PRESETS.iter().find(|p| p.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_is_in_range_and_half_step() {
        for p in PRESETS {
            for (i, b) in p.bands.iter().enumerate() {
                assert!(
                    (-10.0..=10.0).contains(b),
                    "{} band {i} out of range: {b}",
                    p.name
                );
                // 0.5 dB grid — the device encodes in half-decibel units.
                assert!(
                    (b * 2.0).fract() == 0.0,
                    "{} band {i} not on the 0.5 dB grid: {b}",
                    p.name
                );
            }
        }
    }

    #[test]
    fn names_are_unique_and_findable() {
        let mut seen = std::collections::HashSet::new();
        for p in PRESETS {
            assert!(seen.insert(p.name), "duplicate preset name: {}", p.name);
            assert!(find(p.name).is_some());
        }
        assert!(PRESETS.len() >= 24, "expected a large library");
    }
}
