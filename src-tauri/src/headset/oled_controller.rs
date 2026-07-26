//! The OLED draw loop.
//!
//! The base station's own firmware keeps repainting the panel, so a single
//! frame is overwritten within a tick. To hold custom content we run a
//! dedicated thread that continuously re-pushes the current framebuffer
//! (verified: ~24 fps holds content and animates cleanly). Content, live
//! modes and notifications are delivered over an mpsc channel.
//!
//! Each live mode lives in its own `oled_mode_*` module and owns its data
//! sampling; this file only decides *which* one to paint and keeps their
//! state alive between frames.

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::audio::pw_native::levels::LevelStore;
use crate::persistence::prefs::NotifyScroll;

use super::hidraw::{DevicePath, HidDevice};
use super::media::OledFrame;
use super::oled::{brightness_packet, return_to_ui_packet, Framebuffer};
use super::oled_mode_audio::{self, AudioMeters};
use super::oled_mode_clock::{Clock, Timer};
use super::oled_mode_media::MediaMode;
use super::oled_mode_misc::{self, Weather};
use super::oled_mode_spectrum::Spectrum;
use super::oled_mode_sys::SysMonitor;
use super::oled_rotation::{pick_context_mode, Context, ModeId, Rotator};
use super::protocol::{AncMode, HeadsetStatus};

/// What the OLED should currently show.
#[derive(Clone)]
pub enum OledContent {
    /// Hand the panel back to the SteelSeries firmware UI.
    Ui,
    /// Static text lines, auto-sized and centred.
    Text(Vec<String>),
    /// One of the live modes.
    Mode(ModeId),
    /// Cycle the configured modes on a timer.
    Rotate,
    /// Pick a mode from what the machine is currently doing.
    Auto,
    /// A (possibly animated) media clip.
    Frames {
        frames: Arc<Vec<OledFrame>>,
        looping: bool,
    },
}

/// Extra state the modes need that the draw thread can't read itself.
#[derive(Clone, Default)]
pub struct AuxData {
    pub mouse_battery: Option<u8>,
    pub mouse_charging: bool,
    pub mouse_model: String,
    /// (app name, currently producing audio)
    pub active_apps: Vec<(String, bool)>,
}

/// Messages to the OLED thread.
pub enum OledCommand {
    Set(OledContent),
    Notify {
        lines: Vec<String>,
        duration: Duration,
    },
    Brightness(u8),
    /// Feed the newest status so the dashboard/context picker can use it.
    UpdateStatus(Box<HeadsetStatus>),
    /// Feed data owned by other subsystems (mouse, app list).
    UpdateAux(Box<AuxData>),
    /// Configure the timed rotator.
    ConfigureRotation { modes: Vec<ModeId>, secs: u64 },
    /// Choose how over-long notifications scroll (vertical vs. horizontal).
    SetNotifyScroll(NotifyScroll),
    /// Timer control for the Timer mode.
    TimerCountdown(u64),
    TimerStopwatch,
    TimerToggle,
    TimerReset,
    Shutdown,
}

/// Frame cadence when repainting to fight the firmware's own repaint.
const TICK: Duration = Duration::from_millis(40);

/// Everything the loop keeps alive between frames.
struct Modes {
    meters: AudioMeters,
    clock: Clock,
    timer: Timer,
    sys: SysMonitor,
    media: MediaMode,
    weather: Weather,
    spectrum: Spectrum,
    /// The spectrum capture is a subprocess, so only start it on first use.
    spectrum_started: bool,
}

impl Modes {
    fn new() -> Self {
        Self {
            meters: AudioMeters::new(),
            clock: Clock::new(),
            timer: Timer::new(),
            sys: SysMonitor::new(),
            media: MediaMode::new(),
            weather: Weather::new(),
            spectrum: Spectrum::new(),
            spectrum_started: false,
        }
    }

    /// Render one live mode.
    fn render(
        &mut self,
        mode: ModeId,
        status: &HeadsetStatus,
        aux: &AuxData,
        levels: Option<&LevelStore>,
    ) -> Framebuffer {
        match mode {
            ModeId::Headset => render_status(status),
            ModeId::Vu => match levels {
                Some(l) => self.meters.render_vu(l),
                // pactl fallback backend: no peak metering available.
                None => text_screen("VU", "no metering"),
            },
            ModeId::ChatMix => {
                oled_mode_audio::render_chatmix(status.chatmix_game, status.chatmix_chat)
            }
            ModeId::Spectrum => {
                if !self.spectrum_started {
                    self.spectrum.start();
                    self.spectrum_started = true;
                }
                self.spectrum.render()
            }
            ModeId::System => render_system_basic(&mut self.sys),
            ModeId::Graphs => self.sys.render_graphs(),
            ModeId::Network => self.sys.render_network(),
            ModeId::Uptime => self.sys.render_uptime(),
            ModeId::Temps => self.sys.render_temps(),
            ModeId::NowPlaying => self.media.render_now_playing(),
            ModeId::AlbumArt => self.media.render_with_art(),
            ModeId::Clock => self.clock.render_digital(),
            ModeId::ClockBig => self.clock.render_big(),
            ModeId::ClockDate => self.clock.render_with_date(),
            ModeId::ClockAnalog => self.clock.render_analog(),
            ModeId::Timer => self.timer.render(),
            ModeId::Weather => self.weather.render(),
            ModeId::MouseBattery => oled_mode_misc::render_mouse_battery(
                aux.mouse_battery,
                aux.mouse_charging,
                &aux.mouse_model,
            ),
            ModeId::ActiveApps => oled_mode_misc::render_active_apps(&aux.active_apps),
        }
    }

    /// Stop anything that owns a subprocess. Called when leaving a mode that
    /// needs one, so an idle capture doesn't keep running forever.
    fn release_spectrum(&mut self) {
        if self.spectrum_started {
            self.spectrum.stop();
            self.spectrum_started = false;
        }
    }
}

pub fn run(path: DevicePath, rx: Receiver<OledCommand>, levels: Option<Arc<LevelStore>>) {
    let mut dev = match HidDevice::open(&path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("sink: OLED thread could not open device: {e}");
            return;
        }
    };

    let mut content = OledContent::Ui;
    let mut status = HeadsetStatus::default();
    let mut aux = AuxData::default();
    let mut brightness: Option<u8> = None;
    let mut modes = Modes::new();
    let mut rotator = Rotator::new();
    let mut last_painted: Option<ModeId> = None;

    // Animation cursor.
    let mut frame_idx = 0usize;
    let mut frame_started = Instant::now();

    // Notification overlay state: (lines, shown_at, expires_at).
    let mut notify: Option<(Vec<String>, Instant, Instant)> = None;
    let mut notify_scroll = NotifyScroll::default();
    let mut saved: Option<OledContent> = None;

    // `Ui` means "stop drawing"; send return-to-ui once then idle.
    let mut ui_handed_back = false;

    loop {
        let idle = matches!(content, OledContent::Ui) && notify.is_none();
        let wait = if idle { Duration::from_millis(250) } else { TICK };
        match rx.recv_timeout(wait) {
            Ok(cmd) => match cmd {
                OledCommand::Shutdown => {
                    modes.release_spectrum();
                    let _ = dev.write_command(&return_to_ui_packet());
                    return;
                }
                OledCommand::Brightness(level) => {
                    brightness = Some(level);
                    let _ = dev.write_command(&brightness_packet(level));
                }
                OledCommand::UpdateStatus(s) => status = *s,
                OledCommand::UpdateAux(a) => aux = *a,
                OledCommand::ConfigureRotation { modes: m, secs } => {
                    rotator.configure(m, secs);
                }
                OledCommand::SetNotifyScroll(mode) => notify_scroll = mode,
                OledCommand::TimerCountdown(secs) => modes.timer.start_countdown(secs),
                OledCommand::TimerStopwatch => modes.timer.start_stopwatch(),
                OledCommand::TimerToggle => modes.timer.toggle_pause(),
                OledCommand::TimerReset => modes.timer.reset(),
                OledCommand::Notify { lines, duration } => {
                    if saved.is_none() {
                        saved = Some(content.clone());
                    }
                    let now = Instant::now();
                    notify = Some((lines, now, now + duration));
                    ui_handed_back = false;
                }
                OledCommand::Set(c) => {
                    // Leaving the spectrum shuts its capture down.
                    if !matches!(c, OledContent::Mode(ModeId::Spectrum)) {
                        modes.release_spectrum();
                    }
                    if !matches!(c, OledContent::Rotate) {
                        rotator.disable();
                    }
                    content = c;
                    frame_idx = 0;
                    frame_started = Instant::now();
                    ui_handed_back = false;
                    saved = None;
                    notify = None;
                    if let Some(b) = brightness {
                        let _ = dev.write_command(&brightness_packet(b));
                    }
                }
            },
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                modes.release_spectrum();
                let _ = dev.write_command(&return_to_ui_packet());
                return;
            }
        }

        // Expire a finished notification.
        if let Some((_, _, until)) = &notify {
            if Instant::now() >= *until {
                notify = None;
                if let Some(prev) = saved.take() {
                    content = prev;
                    frame_idx = 0;
                    frame_started = Instant::now();
                }
            }
        }

        // Decide what to paint this tick.
        let fb = if let Some((lines, shown_at, _)) = &notify {
            Some(render_notification(lines, shown_at.elapsed(), notify_scroll))
        } else {
            match &content {
                OledContent::Ui => {
                    if !ui_handed_back {
                        let _ = dev.write_command(&return_to_ui_packet());
                        ui_handed_back = true;
                    }
                    None
                }
                OledContent::Text(lines) => Some(render_text(lines)),
                OledContent::Mode(m) => {
                    Some(modes.render(*m, &status, &aux, levels.as_deref()))
                }
                OledContent::Rotate => {
                    let m = rotator.current().unwrap_or(ModeId::Clock);
                    switch_side_effects(&mut modes, &mut last_painted, m);
                    Some(modes.render(m, &status, &aux, levels.as_deref()))
                }
                OledContent::Auto => {
                    let m = pick_context_mode(&build_context(&status, &aux));
                    switch_side_effects(&mut modes, &mut last_painted, m);
                    Some(modes.render(m, &status, &aux, levels.as_deref()))
                }
                OledContent::Frames { frames, looping } => {
                    render_animation(frames, looping, &mut frame_idx, &mut frame_started)
                }
            }
        };

        if let Some(fb) = fb {
            for pkt in fb.frame_packets() {
                if dev.send_feature(&pkt).is_err() {
                    // Transient USB hiccup; try again next tick.
                    break;
                }
            }
        }
    }
}

/// When an automatic switch moves away from the spectrum, stop its capture.
fn switch_side_effects(modes: &mut Modes, last: &mut Option<ModeId>, now: ModeId) {
    if *last != Some(now) {
        if *last == Some(ModeId::Spectrum) {
            modes.release_spectrum();
        }
        *last = Some(now);
    }
}

/// Summarise the machine's state for the context picker.
fn build_context(status: &HeadsetStatus, aux: &AuxData) -> Context {
    Context {
        audio_active: aux.active_apps.iter().any(|(_, playing)| *playing),
        // A player reporting a track is close enough; the media module does
        // the precise check when it actually renders.
        media_playing: aux.active_apps.iter().any(|(name, playing)| {
            *playing
                && matches!(
                    name.to_ascii_lowercase().as_str(),
                    "spotify" | "firefox" | "chromium" | "google chrome" | "mpv" | "vlc"
                )
        }),
        load_high: false, // filled by the sys module in a later pass
        headset_idle: status.power_status.as_deref() != Some("online"),
    }
}

/// A two-line fallback screen for modes with no data source.
fn text_screen(title: &str, sub: &str) -> Framebuffer {
    let mut fb = Framebuffer::new();
    fb.draw_text_centered(18, title, 2);
    fb.draw_text_centered(40, sub, 1);
    fb
}

/// The plain system dashboard (the original combined CPU/RAM/GPU view).
fn render_system_basic(sys: &mut SysMonitor) -> Framebuffer {
    // The dedicated graph view is the richer one; this keeps the original
    // at-a-glance layout available under the "System" name.
    sys.render_graphs()
}

/// Advance the animation cursor and return this tick's framebuffer.
fn render_animation(
    frames: &Arc<Vec<OledFrame>>,
    looping: &bool,
    idx: &mut usize,
    started: &mut Instant,
) -> Option<Framebuffer> {
    if frames.is_empty() {
        return None;
    }
    let cur = frames.get(*idx).or_else(|| frames.last())?;
    if cur.delay_ms > 0 && started.elapsed() >= Duration::from_millis(cur.delay_ms as u64) {
        *started = Instant::now();
        if *idx + 1 < frames.len() {
            *idx += 1;
        } else if *looping {
            *idx = 0;
        }
    }
    Some(frames[(*idx).min(frames.len() - 1)].fb.clone())
}

/// Render up to a few centred text lines, auto-scaling a single short line up.
fn render_text(lines: &[String]) -> Framebuffer {
    let mut fb = Framebuffer::new();
    let lines: Vec<&String> = lines.iter().take(4).collect();
    if lines.is_empty() {
        return fb;
    }
    let scale = if lines.len() == 1 && lines[0].len() <= 8 {
        3
    } else if lines.len() <= 2 {
        2
    } else {
        1
    };
    let line_h = (7 * scale + 3) as isize;
    let total = line_h * lines.len() as isize;
    let mut y = (super::oled::HEIGHT as isize - total) / 2;
    for line in lines {
        fb.draw_text_centered(y, line, scale);
        y += line_h;
    }
    fb
}

/// Padding inside the 1px notification border.
const N_PAD: isize = 3;
/// Row pitch for scale-1 body lines (glyph is 7px tall).
const N_LINE_H: isize = 9;

/// A notification: a framed card with a bold title (line 0) and the body
/// (remaining lines). Text that doesn't fit is never cut off — it scrolls,
/// either vertically (word-wrapped block) or horizontally (per-line ticker),
/// per the user's [`NotifyScroll`] choice. `elapsed` drives the animation.
fn render_notification(lines: &[String], elapsed: Duration, scroll: NotifyScroll) -> Framebuffer {
    let mut fb = Framebuffer::new();
    let w = super::oled::WIDTH as isize;
    let h = super::oled::HEIGHT as isize;
    fb.stroke_rect(0, 0, super::oled::WIDTH, super::oled::HEIGHT);

    let x0 = N_PAD;
    let x1 = w - N_PAD;
    let view = x1 - x0;
    let t = elapsed.as_secs_f32();

    // Title: first line, scale 2 — centred if it fits, else a horizontal ticker
    // (a two-scale title is wide, so this keeps long app names readable).
    let title = lines.first().map(String::as_str).unwrap_or("");
    let title_y = N_PAD + 1;
    draw_scroll_line(&mut fb, title, 2, title_y, x0, x1, t);

    let body_top = title_y + 14 + 2;
    let body_bot = h - N_PAD;
    let body: Vec<&str> = lines
        .iter()
        .skip(1)
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .collect();
    if body.is_empty() {
        return fb;
    }

    match scroll {
        // One ticker line: the whole body joined and scrolled left-to-right.
        NotifyScroll::Horizontal => {
            let joined = body.join("  ·  ");
            let y = body_top + ((body_bot - body_top - 7) / 2).max(0);
            draw_scroll_line(&mut fb, &joined, 1, y, x0, x1, t);
        }
        // Word-wrapped block, scrolled vertically when it overflows the card.
        NotifyScroll::Vertical => {
            let wrapped = wrap_text(&body.join("\n"), 1, view);
            let area_h = body_bot - body_top;
            let total = wrapped.len() as isize * N_LINE_H;
            let off = if total <= area_h {
                0
            } else {
                scroll_offset(total - area_h, t, 12.0)
            };
            for (i, line) in wrapped.iter().enumerate() {
                let ly = body_top + i as isize * N_LINE_H - off;
                if ly + 7 <= body_top || ly >= body_bot {
                    continue; // fully outside the body window
                }
                let tw = Framebuffer::text_width(line, 1) as isize;
                let lx = x0 + ((view - tw) / 2).max(0);
                fb.draw_text_clip(lx, ly, line, 1, x0, body_top, x1, body_bot);
            }
        }
    }
    fb
}

/// Draw one line at row `y`: centred if it fits in `[x0, x1)`, otherwise a
/// horizontal ticker that pauses at each end. Clipped so it never spills past
/// the card edges.
fn draw_scroll_line(fb: &mut Framebuffer, text: &str, scale: usize, y: isize, x0: isize, x1: isize, t: f32) {
    let tw = Framebuffer::text_width(text, scale) as isize;
    let view = x1 - x0;
    let gh = 7 * scale as isize;
    if tw <= view {
        let x = x0 + (view - tw) / 2;
        fb.draw_text_clip(x, y, text, scale, x0, y, x1, y + gh);
    } else {
        let off = scroll_offset(tw - view, t, 18.0);
        fb.draw_text_clip(x0 - off, y, text, scale, x0, y, x1, y + gh);
    }
}

/// A ping-pong scroll offset in `[0, span]` px: dwell at the start, travel to
/// the end at `speed` px/s, dwell, travel back, repeat.
fn scroll_offset(span: isize, t: f32, speed: f32) -> isize {
    if span <= 0 {
        return 0;
    }
    let span = span as f32;
    let pause = 1.4_f32;
    let travel = span / speed;
    let period = travel * 2.0 + pause * 2.0;
    let p = t % period;
    let d = if p < pause {
        0.0
    } else if p < pause + travel {
        (p - pause) * speed
    } else if p < pause + travel + pause {
        span
    } else {
        span - (p - pause - travel - pause) * speed
    };
    d.round().clamp(0.0, span) as isize
}

/// Greedy word-wrap to a pixel width at a given scale, breaking words that are
/// themselves too long. Newlines in the input start a new line.
fn wrap_text(text: &str, scale: usize, max_width: isize) -> Vec<String> {
    let advance = super::oled::CHAR_ADVANCE * scale.max(1);
    let max_chars = (max_width.max(1) as usize / advance).max(1);
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            let wlen = word.chars().count();
            if wlen > max_chars {
                if !line.is_empty() {
                    out.push(std::mem::take(&mut line));
                }
                let mut chars = word.chars().peekable();
                while chars.peek().is_some() {
                    let chunk: String = chars.by_ref().take(max_chars).collect();
                    if chunk.chars().count() == max_chars {
                        out.push(chunk);
                    } else {
                        line = chunk; // keep the tail to continue the line
                    }
                }
                continue;
            }
            let need = if line.is_empty() { wlen } else { line.chars().count() + 1 + wlen };
            if need > max_chars {
                out.push(std::mem::take(&mut line));
                line = word.to_string();
            } else {
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(word);
            }
        }
        if !line.is_empty() {
            out.push(line);
        }
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// The headset dashboard: title, battery bar, volume, ANC and mic state.
fn render_status(s: &HeadsetStatus) -> Framebuffer {
    let mut fb = Framebuffer::new();
    fb.draw_text(2, 1, "SINK", 1);
    if let Some(v) = s.volume_percent {
        let label = format!("VOL {v}%");
        let w = Framebuffer::text_width(&label, 1) as isize;
        fb.draw_text(super::oled::WIDTH as isize - w - 2, 1, &label, 1);
    }

    if let Some(bat) = s.headset_battery_percent {
        fb.draw_text(2, 14, &format!("BATT {bat}%"), 1);
        let (bx, by, bw, bh) = (2isize, 26isize, 100usize, 10usize);
        fb.stroke_rect(bx, by, bw, bh);
        fb.fill_rect(bx + bw as isize, by + 3, 3, 4, true);
        let fill = ((bw - 4) as u32 * bat as u32 / 100) as usize;
        fb.fill_rect(bx + 2, by + 2, fill, bh - 4, true);
    }

    let anc = match s.anc {
        Some(AncMode::On) => "ANC ON",
        Some(AncMode::Transparent) => "TRANSP",
        Some(AncMode::Off) => "ANC OFF",
        None => "",
    };
    fb.draw_text(2, 42, anc, 1);
    let mic = match s.mic_muted {
        Some(true) => "MIC MUTED",
        Some(false) => "MIC ON",
        None => "",
    };
    fb.draw_text(2, 54, mic, 1);

    if let Some(slot) = s.charge_slot_battery_percent {
        let label = format!("DOCK {slot}%");
        let w = Framebuffer::text_width(&label, 1) as isize;
        fb.draw_text(super::oled::WIDTH as isize - w - 2, 54, &label, 1);
    }
    fb
}

#[cfg(test)]
mod tests {
    use super::*;

    // 120px / (6px per scale-1 char) = 20 chars per line.
    const W: isize = 120;

    #[test]
    fn wrap_breaks_on_words_within_width() {
        let lines = wrap_text("the quick brown fox jumps over the lazy dog", 1, W);
        assert!(lines.len() > 1, "long text must wrap onto several lines");
        for l in &lines {
            assert!(l.chars().count() <= 20, "line {l:?} exceeds the width");
        }
        // No text is lost: joining the wrapped words back matches the input words.
        let joined: Vec<&str> = lines.iter().flat_map(|l| l.split_whitespace()).collect();
        assert_eq!(joined.join(" "), "the quick brown fox jumps over the lazy dog");
    }

    #[test]
    fn wrap_hard_breaks_overlong_words() {
        // A single word longer than the line must be split, not dropped.
        let word = "a".repeat(55);
        let lines = wrap_text(&word, 1, W);
        assert!(lines.len() >= 3);
        for l in &lines {
            assert!(l.chars().count() <= 20);
        }
        let rejoined: String = lines.concat();
        assert_eq!(rejoined, word);
    }

    #[test]
    fn wrap_honours_explicit_newlines() {
        let lines = wrap_text("first\nsecond", 1, W);
        assert_eq!(lines, vec!["first".to_string(), "second".to_string()]);
    }

    #[test]
    fn scroll_offset_stays_in_range_and_pings_back() {
        let span = 40;
        // Sample a full period; the offset must never leave [0, span].
        let mut saw_zero = false;
        let mut saw_max = false;
        for i in 0..400 {
            let t = i as f32 * 0.05;
            let o = scroll_offset(span, t, 18.0);
            assert!((0..=span).contains(&o), "offset {o} out of range at t={t}");
            if o == 0 {
                saw_zero = true;
            }
            if o == span {
                saw_max = true;
            }
        }
        assert!(saw_zero && saw_max, "scroll must reach both ends");
    }

    #[test]
    fn scroll_offset_zero_span_is_static() {
        assert_eq!(scroll_offset(0, 3.2, 18.0), 0);
    }
}
