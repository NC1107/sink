use serde::{Deserialize, Serialize};

/// True if `sink_name` is one of our managed virtual channels. Channels
/// are user-defined (see persistence::channels) but always carry the
/// `sink_` prefix; Sink's own service nodes are excluded.
pub fn is_virtual_sink(sink_name: &str) -> bool {
    sink_name.starts_with("sink_")
        && !crate::persistence::channels::RESERVED_SINK_NAMES.contains(&sink_name)
}

/// Property values that are useless as names — media frameworks announcing
/// themselves, or placeholder stream titles.
const GENERIC_NAMES: [&str; 13] = [
    "WEBRTC VoiceEngine",
    "audio-src",
    "Playback Stream",
    "playStream",
    "audio stream",
    "Audio Stream",
    "audio player",
    "media player",
    "output",
    "Playback",
    "ALSA Playback",
    "Audio output",
    "Audio Source",
];

/// Runtime/wrapper names that hide the real app — e.g. Spotify is a
/// Chromium shell, so application.name says "Chromium" while the process
/// binary says "spotify". A wrapper beats a generic, but a real name
/// (usually the binary) beats both.
const WRAPPER_NAMES: [&str; 14] = [
    "Chromium",
    "Google Chrome",
    "Chrome",
    "Electron",
    "WINE",
    "wine64-preloader",
    "java",
    "python",
    "python3",
    "node",
    "mono",
    "dotnet",
    "QtWebEngine",
    "CEF",
];

fn name_quality(value: &str) -> u8 {
    if GENERIC_NAMES.iter().any(|g| g.eq_ignore_ascii_case(value)) {
        0
    } else if WRAPPER_NAMES.iter().any(|w| w.eq_ignore_ascii_case(value)) {
        1
    } else {
        2
    }
}

/// Prettify a value for display: lone all-lowercase binary names get a
/// capital ("spotify" → "Spotify"). Identity matching always uses the raw
/// value, so this never affects routing rules.
fn prettify(value: &str) -> String {
    if !value.contains(' ') && value.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
        let mut chars = value.chars();
        match chars.next() {
            Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
            None => value.to_string(),
        }
    } else {
        value.to_string()
    }
}

/// Resolve a stream's identity: returns (display name, match property,
/// raw match value). The best-quality candidate along the chain wins:
/// real app names beat runtime wrappers beat generic stream titles.
pub fn resolve_identity(get: impl Fn(&str) -> Option<String>) -> (String, String, String) {
    const CHAIN: [&str; 4] = [
        "application.name",
        "application.process.binary",
        "media.name",
        "node.name",
    ];
    let mut best: Option<(u8, String, String)> = None;
    for key in CHAIN {
        if let Some(value) = get(key) {
            // Empty/whitespace property values are noise, not identities.
            if value.trim().is_empty() {
                continue;
            }
            let quality = name_quality(&value);
            // (map_or keeps MSRV 1.77 — Option::is_none_or is 1.82+.)
            if best.as_ref().map_or(true, |(q, _, _)| quality > *q) {
                let stop = quality == 2;
                best = Some((quality, key.to_string(), value));
                if stop {
                    break;
                }
            }
        }
    }
    match best {
        Some((_, key, value)) => (prettify(&value), key, value),
        None => (
            "Unknown".to_string(),
            "application.name".to_string(),
            "Unknown".to_string(),
        ),
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;
    use std::collections::HashMap;

    fn resolve(props: &[(&str, &str)]) -> (String, String, String) {
        let map: HashMap<String, String> = props
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        resolve_identity(|key| map.get(key).cloned())
    }

    #[test]
    fn spotify_masquerading_as_chromium_resolves_via_binary() {
        let (display, prop, value) = resolve(&[
            ("application.name", "Chromium"),
            ("application.process.binary", "spotify"),
            ("media.name", "Playback"),
        ]);
        assert_eq!(display, "Spotify"); // prettified for the UI
        assert_eq!(prop, "application.process.binary");
        assert_eq!(value, "spotify"); // raw for rule matching
    }

    #[test]
    fn discord_webrtc_resolves_via_binary() {
        let (display, prop, _) = resolve(&[
            ("application.name", "WEBRTC VoiceEngine"),
            ("application.process.binary", "Discord"),
        ]);
        assert_eq!(display, "Discord");
        assert_eq!(prop, "application.process.binary");
    }

    #[test]
    fn all_wrapper_chain_keeps_the_first_hit() {
        // Every candidate is a wrapper (equal quality): the strict `>`
        // ranking must keep the first one, not let later ties override it.
        let (display, prop, value) = resolve(&[
            ("application.name", "Electron"),
            ("application.process.binary", "node"),
            ("media.name", "java"),
        ]);
        assert_eq!(prop, "application.name");
        assert_eq!(value, "Electron");
        assert_eq!(display, "Electron");
    }

    #[test]
    fn real_browser_keeps_its_wrapper_name() {
        let (display, _, _) = resolve(&[
            ("application.name", "Chromium"),
            ("application.process.binary", "chromium"),
            ("media.name", "Playback"),
        ]);
        assert_eq!(display, "Chromium"); // wrapper beats generic; no better candidate
    }

    #[test]
    fn firefox_application_name_wins_immediately() {
        let (display, prop, _) = resolve(&[
            ("application.name", "Firefox"),
            ("application.process.binary", "firefox"),
        ]);
        assert_eq!(display, "Firefox");
        assert_eq!(prop, "application.name");
    }

    #[test]
    fn empty_values_never_win() {
        let (display, _, value) = resolve(&[
            ("application.name", ""),
            ("media.name", "  "),
            ("node.name", "real-app"),
        ]);
        assert_eq!(display, "Real-app");
        assert_eq!(value, "real-app");
    }

    #[test]
    fn pure_generic_still_shows_something() {
        let (display, _, value) = resolve(&[("media.name", "audio-src"), ("node.name", "audio-src")]);
        assert_eq!(display, "Audio-src");
        assert_eq!(value, "audio-src");
    }
}

/// A running application audio stream (a PulseAudio "sink input").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStream {
    pub index: u32,
    /// Display name (possibly prettified — not for matching).
    pub app_name: String,
    /// PipeWire property the identity was read from (e.g. "application.name").
    pub match_prop: String,
    /// Raw property value; with `match_prop` this is the stream's stable
    /// identity for assignments, aliases and WirePlumber rules.
    pub match_value: String,
    /// User-chosen display name overriding `app_name` (set via rename).
    pub alias: Option<String>,
    pub icon_name: Option<String>,
    /// Resolved absolute icon file path (desktop-entry based), ready for
    /// the asset protocol. Filled in by the command layer.
    pub icon_path: Option<String>,
    /// Producing process id — unlocks /proc-based desktop-entry lookup
    /// (cgroup scope, flatpak info, exe path) for icon resolution.
    #[serde(default)]
    pub pid: Option<u32>,
    /// Name of the virtual sink the stream is routed to, if it is one of ours.
    pub assigned_sink: Option<String>,
    pub volume_percent: u8,
    pub muted: bool,
    /// True while the stream is actively producing audio (node running /
    /// not corked) — drives the activity indicator in the app list.
    pub active: bool,
}

fn default_true() -> bool {
    true
}

/// One of the user-defined virtual channels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualSink {
    /// e.g. "sink_game"
    pub name: String,
    /// e.g. "Game"
    pub label: String,
    /// Material Symbol for the strip icon.
    #[serde(default)]
    pub icon: Option<String>,
    pub volume_percent: u8,
    pub muted: bool,
    /// Whether this channel feeds the Stream Mix source (what OBS records).
    #[serde(default = "default_true")]
    pub stream_mix: bool,
}

/// A physical audio output device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputDevice {
    pub index: u32,
    pub name: String,
    pub description: String,
}

fn default_mic_label() -> String {
    "Sink Mic".to_string()
}
fn default_gate_threshold() -> f32 {
    -40.0
}
fn default_comp_threshold() -> f32 {
    -18.0
}
fn default_comp_ratio() -> f32 {
    3.0
}
fn default_limiter_ceiling() -> f32 {
    -1.0
}

/// Phase 3 mic chain configuration (persisted; applied live).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MicConfig {
    pub enabled: bool,
    /// node.name of the hardware mic to capture (None = system default).
    pub input_device: Option<String>,
    /// What other apps list the processed mic as (node description).
    #[serde(default = "default_mic_label")]
    pub output_label: String,
    /// 0–200; 100 = unity.
    pub gain_percent: u8,
    pub gate_enabled: bool,
    pub comp_enabled: bool,
    pub limiter_enabled: bool,
    pub muted: bool,
    /// Gate opens above this level (dBFS).
    #[serde(default = "default_gate_threshold")]
    pub gate_threshold_db: f32,
    /// Compression starts above this level (dBFS).
    #[serde(default = "default_comp_threshold")]
    pub comp_threshold_db: f32,
    /// Compression ratio (N:1).
    #[serde(default = "default_comp_ratio")]
    pub comp_ratio: f32,
    /// Hard ceiling (dBFS).
    #[serde(default = "default_limiter_ceiling")]
    pub limiter_ceiling_db: f32,
}

impl MicConfig {
    /// Clamp numeric fields to their documented, DSP-safe ranges and replace
    /// non-finite values, so a malformed or hostile IPC payload can't push
    /// the mic chain out of range (TD-050).
    pub fn clamp_ranges(&mut self) {
        fn finite(v: f32, fallback: f32, lo: f32, hi: f32) -> f32 {
            if v.is_finite() {
                v.clamp(lo, hi)
            } else {
                fallback
            }
        }
        self.gain_percent = self.gain_percent.min(200);
        self.gate_threshold_db = finite(self.gate_threshold_db, default_gate_threshold(), -100.0, 0.0);
        self.comp_threshold_db = finite(self.comp_threshold_db, default_comp_threshold(), -100.0, 0.0);
        self.comp_ratio = finite(self.comp_ratio, default_comp_ratio(), 1.0, 20.0);
        self.limiter_ceiling_db = finite(self.limiter_ceiling_db, default_limiter_ceiling(), -60.0, 0.0);
    }
}

impl Default for MicConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            input_device: None,
            output_label: default_mic_label(),
            gain_percent: 100,
            gate_enabled: true,
            comp_enabled: true,
            limiter_enabled: true,
            muted: false,
            gate_threshold_db: default_gate_threshold(),
            comp_threshold_db: default_comp_threshold(),
            comp_ratio: default_comp_ratio(),
            limiter_ceiling_db: default_limiter_ceiling(),
        }
    }
}

#[cfg(test)]
mod mic_clamp_tests {
    use super::*;

    #[test]
    fn clamp_ranges_bounds_out_of_range_values() {
        let mut c = MicConfig {
            gain_percent: 255,
            gate_threshold_db: 40.0,
            comp_threshold_db: -400.0,
            comp_ratio: 1000.0,
            limiter_ceiling_db: 12.0,
            ..MicConfig::default()
        };
        c.clamp_ranges();
        assert_eq!(c.gain_percent, 200);
        assert_eq!(c.gate_threshold_db, 0.0);
        assert_eq!(c.comp_threshold_db, -100.0);
        assert_eq!(c.comp_ratio, 20.0);
        assert_eq!(c.limiter_ceiling_db, 0.0);
    }

    #[test]
    fn clamp_ranges_replaces_non_finite_with_defaults() {
        let mut c = MicConfig {
            gate_threshold_db: f32::NAN,
            comp_threshold_db: f32::INFINITY,
            comp_ratio: f32::NEG_INFINITY,
            ..MicConfig::default()
        };
        c.clamp_ranges();
        assert_eq!(c.gate_threshold_db, default_gate_threshold());
        assert_eq!(c.comp_threshold_db, default_comp_threshold());
        assert_eq!(c.comp_ratio, default_comp_ratio());
    }

    #[test]
    fn clamp_ranges_leaves_valid_values_untouched() {
        let mut c = MicConfig {
            gain_percent: 120,
            gate_threshold_db: -45.0,
            comp_threshold_db: -18.0,
            comp_ratio: 4.0,
            limiter_ceiling_db: -1.0,
            ..MicConfig::default()
        };
        let before = c.clone();
        c.clamp_ranges();
        assert_eq!(c, before);
    }
}

/// Hard cap on parametric EQ bands per channel. Ten matches the Sonar EQ
/// users already know, keeps preset validation simple, and bounds the RT
/// cost per channel.
pub const MAX_EQ_BANDS: usize = 10;

/// Parametric EQ band shapes (RBJ Audio EQ Cookbook designs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EqBandKind {
    Peaking,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
}

fn default_band_q() -> f32 {
    1.0
}

/// One parametric EQ band.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EqBand {
    pub kind: EqBandKind,
    pub freq_hz: f32,
    /// Ignored by LowPass/HighPass (their shape has no gain parameter).
    #[serde(default)]
    pub gain_db: f32,
    /// Peaking/LowPass/HighPass: filter Q. Shelves: RBJ shelf slope S —
    /// one field, two meanings, so presets stay a flat 4-field record.
    #[serde(default = "default_band_q")]
    pub q: f32,
}

impl EqBand {
    /// TD-050: clamp to DSP-safe ranges, replacing non-finite values, so a
    /// hostile IPC payload or preset file can't blow up the filter design.
    pub fn clamp_ranges(&mut self) {
        fn finite(v: f32, fallback: f32, lo: f32, hi: f32) -> f32 {
            if v.is_finite() {
                v.clamp(lo, hi)
            } else {
                fallback
            }
        }
        self.freq_hz = finite(self.freq_hz, 1000.0, 20.0, 20000.0);
        self.gain_db = finite(self.gain_db, 0.0, -24.0, 24.0);
        self.q = finite(self.q, default_band_q(), 0.1, 10.0);
    }
}

/// The Sonar-style starting layout: shelves at the extremes, three mids,
/// everything flat. Five bands; the UI can add up to MAX_EQ_BANDS.
pub fn default_eq_bands() -> Vec<EqBand> {
    [
        (EqBandKind::LowShelf, 100.0, 0.71),
        (EqBandKind::Peaking, 500.0, 1.0),
        (EqBandKind::Peaking, 1500.0, 1.0),
        (EqBandKind::Peaking, 5000.0, 1.0),
        (EqBandKind::HighShelf, 10000.0, 0.71),
    ]
    .into_iter()
    .map(|(kind, freq_hz, q)| EqBand {
        kind,
        freq_hz,
        gain_db: 0.0,
        q,
    })
    .collect()
}

/// A channel's parametric EQ (persisted per channel; applied live).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EqConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Headroom trim applied before the band cascade (dB). Boost-heavy
    /// curves need this negative to avoid clipping.
    #[serde(default)]
    pub preamp_db: f32,
    #[serde(default = "default_eq_bands")]
    pub bands: Vec<EqBand>,
}

impl EqConfig {
    /// TD-050-style sanitization for the whole config (see EqBand).
    pub fn clamp_ranges(&mut self) {
        if !self.preamp_db.is_finite() {
            self.preamp_db = 0.0;
        }
        self.preamp_db = self.preamp_db.clamp(-24.0, 24.0);
        self.bands.truncate(MAX_EQ_BANDS);
        for band in &mut self.bands {
            band.clamp_ranges();
        }
    }
}

impl Default for EqConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            preamp_db: 0.0,
            bands: default_eq_bands(),
        }
    }
}

#[cfg(test)]
mod eq_clamp_tests {
    use super::*;

    #[test]
    fn band_clamp_bounds_out_of_range_values() {
        let mut b = EqBand {
            kind: EqBandKind::Peaking,
            freq_hz: 99999.0,
            gain_db: -80.0,
            q: 0.0,
        };
        b.clamp_ranges();
        assert_eq!(b.freq_hz, 20000.0);
        assert_eq!(b.gain_db, -24.0);
        assert_eq!(b.q, 0.1);
    }

    #[test]
    fn band_clamp_replaces_non_finite_with_defaults() {
        let mut b = EqBand {
            kind: EqBandKind::Peaking,
            freq_hz: f32::NAN,
            gain_db: f32::INFINITY,
            q: f32::NEG_INFINITY,
        };
        b.clamp_ranges();
        assert_eq!(b.freq_hz, 1000.0);
        assert_eq!(b.gain_db, 0.0);
        assert_eq!(b.q, default_band_q());
    }

    #[test]
    fn config_clamp_truncates_to_max_bands() {
        let mut c = EqConfig::default();
        c.bands = vec![c.bands[0]; MAX_EQ_BANDS + 5];
        c.preamp_db = f32::NAN;
        c.clamp_ranges();
        assert_eq!(c.bands.len(), MAX_EQ_BANDS);
        assert_eq!(c.preamp_db, 0.0);
    }

    #[test]
    fn config_without_fields_deserializes_with_defaults() {
        // Old profile/eq JSON without these keys must keep loading.
        let c: EqConfig = serde_json::from_str("{}").unwrap();
        assert!(!c.enabled);
        assert_eq!(c.preamp_db, 0.0);
        assert_eq!(c.bands, default_eq_bands());
    }

    #[test]
    fn band_kind_serializes_snake_case() {
        let json = serde_json::to_string(&EqBandKind::LowShelf).unwrap();
        assert_eq!(json, "\"low_shelf\"");
    }
}
