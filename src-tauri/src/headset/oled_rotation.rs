//! Automatic screen selection for the OLED: a timed rotator and a
//! context-aware picker.
//!
//! Both sit *above* the content modes — they only ever decide *which* screen
//! the draw loop should paint next, never how to paint it. That keeps the
//! rendering modules independent and makes the policy easy to reason about.

use std::time::{Duration, Instant};

/// Every screen the OLED can show, as a stable identifier. Kept as a plain
/// enum (rather than strings) so the compiler catches a missing arm when a new
/// mode is added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeId {
    Headset,
    System,
    Graphs,
    Network,
    Uptime,
    Temps,
    NowPlaying,
    AlbumArt,
    Clock,
    ClockBig,
    ClockDate,
    ClockAnalog,
    Vu,
    ChatMix,
    Spectrum,
    Weather,
    MouseBattery,
    ActiveApps,
    Timer,
}

impl ModeId {
    /// Parse the identifier the frontend sends.
    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "headset" => ModeId::Headset,
            "system" => ModeId::System,
            "graphs" => ModeId::Graphs,
            "network" => ModeId::Network,
            "uptime" => ModeId::Uptime,
            "temps" => ModeId::Temps,
            "now_playing" => ModeId::NowPlaying,
            "album_art" => ModeId::AlbumArt,
            "clock" => ModeId::Clock,
            "clock_big" => ModeId::ClockBig,
            "clock_date" => ModeId::ClockDate,
            "clock_analog" => ModeId::ClockAnalog,
            "vu" => ModeId::Vu,
            "chatmix" => ModeId::ChatMix,
            "spectrum" => ModeId::Spectrum,
            "weather" => ModeId::Weather,
            "mouse_battery" => ModeId::MouseBattery,
            "active_apps" => ModeId::ActiveApps,
            "timer" => ModeId::Timer,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ModeId::Headset => "headset",
            ModeId::System => "system",
            ModeId::Graphs => "graphs",
            ModeId::Network => "network",
            ModeId::Uptime => "uptime",
            ModeId::Temps => "temps",
            ModeId::NowPlaying => "now_playing",
            ModeId::AlbumArt => "album_art",
            ModeId::Clock => "clock",
            ModeId::ClockBig => "clock_big",
            ModeId::ClockDate => "clock_date",
            ModeId::ClockAnalog => "clock_analog",
            ModeId::Vu => "vu",
            ModeId::ChatMix => "chatmix",
            ModeId::Spectrum => "spectrum",
            ModeId::Weather => "weather",
            ModeId::MouseBattery => "mouse_battery",
            ModeId::ActiveApps => "active_apps",
            ModeId::Timer => "timer",
        }
    }

    /// Human label for the picker UI.
    pub fn label(self) -> &'static str {
        match self {
            ModeId::Headset => "Headset",
            ModeId::System => "System",
            ModeId::Graphs => "CPU/GPU graphs",
            ModeId::Network => "Network",
            ModeId::Uptime => "Uptime & load",
            ModeId::Temps => "Temperatures",
            ModeId::NowPlaying => "Now playing",
            ModeId::AlbumArt => "Album art",
            ModeId::Clock => "Clock",
            ModeId::ClockBig => "Clock (big)",
            ModeId::ClockDate => "Clock & date",
            ModeId::ClockAnalog => "Clock (analog)",
            ModeId::Vu => "VU meters",
            ModeId::ChatMix => "ChatMix",
            ModeId::Spectrum => "Spectrum",
            ModeId::Weather => "Weather",
            ModeId::MouseBattery => "Mouse battery",
            ModeId::ActiveApps => "Active apps",
            ModeId::Timer => "Timer",
        }
    }

    pub fn category(self) -> &'static str {
        match self {
            ModeId::Headset | ModeId::ChatMix | ModeId::Vu | ModeId::Spectrum => "Audio",
            ModeId::System | ModeId::Graphs | ModeId::Network | ModeId::Uptime | ModeId::Temps => {
                "System"
            }
            ModeId::NowPlaying | ModeId::AlbumArt => "Media",
            ModeId::Clock | ModeId::ClockBig | ModeId::ClockDate | ModeId::ClockAnalog
            | ModeId::Timer => "Clock",
            ModeId::Weather | ModeId::MouseBattery | ModeId::ActiveApps => "Info",
        }
    }
}

/// All modes, in picker order.
pub const ALL_MODES: &[ModeId] = &[
    ModeId::Headset,
    ModeId::Vu,
    ModeId::ChatMix,
    ModeId::Spectrum,
    ModeId::System,
    ModeId::Graphs,
    ModeId::Network,
    ModeId::Uptime,
    ModeId::Temps,
    ModeId::NowPlaying,
    ModeId::AlbumArt,
    ModeId::Clock,
    ModeId::ClockBig,
    ModeId::ClockDate,
    ModeId::ClockAnalog,
    ModeId::Timer,
    ModeId::Weather,
    ModeId::MouseBattery,
    ModeId::ActiveApps,
];

/// Facts the context picker reasons about. The draw loop fills this in from
/// state it already tracks, so the picker itself stays pure and testable.
#[derive(Debug, Clone, Copy, Default)]
pub struct Context {
    /// Something is producing audio right now.
    pub audio_active: bool,
    /// A media player reports Playing.
    pub media_playing: bool,
    /// Sustained high CPU/GPU — a decent proxy for "a game is running".
    pub load_high: bool,
    /// The headset is on the charger or offline; nothing worth showing.
    pub headset_idle: bool,
}

/// Chooses a screen from the current context. Deliberately simple and
/// explicit — the point is predictability, not cleverness.
pub fn pick_context_mode(ctx: &Context) -> ModeId {
    if ctx.load_high && ctx.audio_active {
        // Gaming: positional audio matters more than metadata.
        ModeId::Vu
    } else if ctx.media_playing {
        ModeId::NowPlaying
    } else if ctx.audio_active {
        ModeId::Vu
    } else if ctx.headset_idle {
        ModeId::Clock
    } else {
        ModeId::Headset
    }
}

/// Cycles a user-chosen list of screens on a timer.
pub struct Rotator {
    modes: Vec<ModeId>,
    interval: Duration,
    idx: usize,
    last_switch: Instant,
    enabled: bool,
}

impl Default for Rotator {
    fn default() -> Self {
        Self {
            modes: Vec::new(),
            interval: Duration::from_secs(10),
            idx: 0,
            last_switch: Instant::now(),
            enabled: false,
        }
    }
}

impl Rotator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled && !self.modes.is_empty()
    }

    /// Start cycling `modes` every `secs` seconds. An empty list disables it.
    pub fn configure(&mut self, modes: Vec<ModeId>, secs: u64) {
        self.enabled = !modes.is_empty();
        self.modes = modes;
        // Clamp so a stray 0 can't spin the draw loop switching every frame.
        self.interval = Duration::from_secs(secs.clamp(2, 3600));
        self.idx = 0;
        self.last_switch = Instant::now();
    }

    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// The screen to show now, advancing when the interval has elapsed.
    pub fn current(&mut self) -> Option<ModeId> {
        if !self.is_enabled() {
            return None;
        }
        if self.last_switch.elapsed() >= self.interval {
            self.idx = (self.idx + 1) % self.modes.len();
            self.last_switch = Instant::now();
        }
        self.modes.get(self.idx).copied()
    }

    /// Skip to the next screen immediately.
    pub fn advance(&mut self) {
        if !self.modes.is_empty() {
            self.idx = (self.idx + 1) % self.modes.len();
            self.last_switch = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_ids_round_trip() {
        for m in ALL_MODES {
            assert_eq!(ModeId::from_str(m.as_str()), Some(*m), "{:?}", m);
        }
        assert_eq!(ModeId::from_str("nope"), None);
    }

    #[test]
    fn context_prefers_vu_while_gaming() {
        let ctx = Context { audio_active: true, load_high: true, ..Default::default() };
        assert_eq!(pick_context_mode(&ctx), ModeId::Vu);
    }

    #[test]
    fn context_shows_media_when_only_music_plays() {
        let ctx = Context { media_playing: true, audio_active: true, ..Default::default() };
        assert_eq!(pick_context_mode(&ctx), ModeId::NowPlaying);
    }

    #[test]
    fn context_falls_back_to_clock_when_idle() {
        let ctx = Context { headset_idle: true, ..Default::default() };
        assert_eq!(pick_context_mode(&ctx), ModeId::Clock);
    }

    #[test]
    fn rotator_advances_and_wraps() {
        let mut r = Rotator::new();
        r.configure(vec![ModeId::Clock, ModeId::Vu], 2);
        assert_eq!(r.current(), Some(ModeId::Clock));
        r.advance();
        assert_eq!(r.current(), Some(ModeId::Vu));
        r.advance();
        assert_eq!(r.current(), Some(ModeId::Clock));
    }

    #[test]
    fn rotator_is_disabled_without_modes() {
        let mut r = Rotator::new();
        r.configure(vec![], 5);
        assert!(!r.is_enabled());
        assert_eq!(r.current(), None);
    }

    #[test]
    fn rotator_clamps_a_zero_interval() {
        let mut r = Rotator::new();
        r.configure(vec![ModeId::Clock], 0);
        // A zero interval would switch every frame; it must be clamped up.
        assert!(r.interval >= Duration::from_secs(2));
    }
}
