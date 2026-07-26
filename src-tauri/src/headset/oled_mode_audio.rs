//! Audio-related OLED screens: a live VU meter fed by the PipeWire
//! [`LevelStore`], a ChatMix balance display and a transient volume popup.
//!
//! Everything here is pure drawing on top of [`Framebuffer`] — no I/O, no
//! allocation beyond a couple of short strings — so the render loop can call
//! these at panel rate (~24 fps) without a second thought.

use std::time::Instant;

use super::oled::{Framebuffer, HEIGHT, WIDTH};
use crate::audio::pw_native::levels::{LevelStore, MAX_METERS};

/// Bottom of the useful dynamic range. Amplitudes are mapped through dB
/// because a linear bar spends 90% of its travel in the top few dB and looks
/// dead at normal listening levels.
const FLOOR_DB: f32 = -48.0;

/// Envelope time constants (seconds). Fast attack keeps transients honest;
/// slow release is what makes the bar read as a level rather than a strobe.
const ATTACK_TAU: f32 = 0.012;
const RELEASE_TAU: f32 = 0.22;

/// Peak tick behaviour: hold, then slide down at a constant rate.
const PEAK_HOLD_S: f32 = 0.6;
const PEAK_FALL_PER_S: f32 = 0.55;

/// Per-meter smoothing state.
#[derive(Default, Clone)]
struct MeterState {
    /// Meter name that owns this slot; slots are recycled when a channel is
    /// deleted, so a name change must reset the envelope instead of letting
    /// the old channel's level bleed into the new one.
    owner: String,
    env: f32,
    peak: f32,
    hold_left: f32,
}

/// Smoothing state for [`AudioMeters::render_vu`].
pub struct AudioMeters {
    meters: Vec<MeterState>,
    last_frame: Option<Instant>,
}

impl Default for AudioMeters {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioMeters {
    pub fn new() -> Self {
        Self {
            meters: vec![MeterState::default(); MAX_METERS],
            last_frame: None,
        }
    }

    /// Draw one horizontal bar per registered meter.
    pub fn render_vu(&mut self, levels: &LevelStore) -> Framebuffer {
        let mut fb = Framebuffer::new();

        // Frame-rate independent smoothing: the caller's cadence is not
        // guaranteed (a slow HID write stretches a frame), so derive the
        // coefficients from the real elapsed time. Clamped to keep a stalled
        // loop from snapping every envelope to zero.
        let now = Instant::now();
        let dt = self
            .last_frame
            .map(|t| now.duration_since(t).as_secs_f32())
            .unwrap_or(0.04)
            .clamp(0.001, 0.25);
        self.last_frame = Some(now);
        let attack = 1.0 - (-dt / ATTACK_TAU).exp();
        let release = 1.0 - (-dt / RELEASE_TAU).exp();

        // `names()` walks a HashMap, so sort by slot: slots are stable per
        // name and keep each bar in the same row between frames.
        let mut names = levels.names();
        names.sort_by_key(|(_, slot)| *slot);
        names.retain(|(_, slot)| *slot < self.meters.len());

        if names.is_empty() {
            fb.draw_text_centered((HEIGHT as isize - 14) / 2, "NO AUDIO", 2);
            return fb;
        }

        let n = names.len();
        let row_h = (HEIGHT / n).clamp(3, 13) as isize;
        let top = (HEIGHT as isize - row_h * n as isize) / 2;
        let bar_h = (row_h - 2).max(3) as usize;
        // Labels only earn their space when a 7px glyph actually fits.
        let labelled = row_h >= 9;
        let bar_x: isize = if labelled { 34 } else { 2 };
        let bar_w = (WIDTH as isize - bar_x - 2).max(4) as usize;

        for (row, (name, slot)) in names.iter().enumerate() {
            let level = self.update(*slot, name, levels, attack, release, dt);
            let y = top + row as isize * row_h;

            if labelled {
                fb.draw_text(1, y + (bar_h as isize - 7) / 2, &short_name(name), 1);
            }

            fb.stroke_rect(bar_x, y, bar_w, bar_h);
            let inner_w = bar_w.saturating_sub(2);
            let inner_h = bar_h.saturating_sub(2);
            let filled = (level * inner_w as f32).round().clamp(0.0, inner_w as f32) as usize;
            fb.fill_rect(bar_x + 1, y + 1, filled, inner_h, true);

            // Peak tick. It never lands inside the filled part (peak >= env),
            // so it stays visible against the solid bar.
            let peak = self.meters[*slot].peak;
            if peak > 0.0 && inner_w > 0 {
                let px = (peak * (inner_w - 1) as f32).round() as isize;
                fb.fill_rect(bar_x + 1 + px, y + 1, 1, inner_h, true);
            }
        }
        fb
    }

    /// Drain a meter and fold it into its envelope; returns the 0..1 level.
    fn update(
        &mut self,
        slot: usize,
        name: &str,
        levels: &LevelStore,
        attack: f32,
        release: f32,
        dt: f32,
    ) -> f32 {
        // Both channels are drained every frame: peaks are consumed on read,
        // so anything left unread would show up late on the next poll.
        let raw = levels.drain(slot, 0).max(levels.drain(slot, 1));
        let target = db_norm(raw);

        let Some(state) = self.meters.get_mut(slot) else {
            return 0.0;
        };
        if state.owner != name {
            *state = MeterState {
                owner: name.to_string(),
                ..MeterState::default()
            };
        }

        let coeff = if target > state.env { attack } else { release };
        state.env += (target - state.env) * coeff;
        state.env = state.env.clamp(0.0, 1.0);

        if state.env >= state.peak {
            state.peak = state.env;
            state.hold_left = PEAK_HOLD_S;
        } else if state.hold_left > 0.0 {
            state.hold_left -= dt;
        } else {
            state.peak = (state.peak - PEAK_FALL_PER_S * dt).max(state.env);
        }
        state.env
    }
}

/// Map a linear amplitude to a 0..1 bar position on a dB scale.
fn db_norm(amp: f32) -> f32 {
    if !amp.is_finite() || amp <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * amp.max(1e-6).log10();
    ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0)
}

/// Label for a meter: `sink_game` -> `GAME`, capped at 5 glyphs so it fits the
/// 30px label gutter. Non-ASCII is dropped because the font has no glyphs.
fn short_name(name: &str) -> String {
    let base = name.strip_prefix("sink_").unwrap_or(name);
    base.chars()
        .filter(char::is_ascii_graphic)
        .take(5)
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// ChatMix balance: game volume on the left, chat on the right, with a marker
/// showing where the mix currently sits.
pub fn render_chatmix(game: Option<u8>, chat: Option<u8>) -> Framebuffer {
    let mut fb = Framebuffer::new();

    let (Some(game), Some(chat)) = (game, chat) else {
        // No dial reading yet (headset asleep, or the HID poll hasn't landed).
        fb.draw_text_centered(16, "CHATMIX", 2);
        fb.draw_text_centered(38, "no data", 1);
        return fb;
    };
    let (game, chat) = (game.min(100), chat.min(100));

    fb.draw_text_centered(2, "CHATMIX", 1);
    fb.draw_text(4, 14, "GAME", 1);
    let chat_w = Framebuffer::text_width("CHAT", 1) as isize;
    fb.draw_text(WIDTH as isize - chat_w - 4, 14, "CHAT", 1);

    const TRACK_X: isize = 4;
    const TRACK_Y: isize = 24;
    const TRACK_W: usize = 120;
    const TRACK_H: usize = 12;
    fb.stroke_rect(TRACK_X, TRACK_Y, TRACK_W, TRACK_H);

    // Centre reference so an even mix is readable at a glance.
    for y in (TRACK_Y + 2..TRACK_Y + TRACK_H as isize - 2).step_by(2) {
        fb.set(TRACK_X + TRACK_W as isize / 2, y, true);
    }

    // Balance is the chat share of the two legs; equal (or both silent) sits
    // dead centre.
    let total = game as f32 + chat as f32;
    let balance = if total > 0.0 { chat as f32 / total } else { 0.5 };
    const MARKER_W: usize = 7;
    let travel = (TRACK_W - 2 - MARKER_W) as f32;
    let mx = TRACK_X + 1 + (balance * travel).round() as isize;
    fb.fill_rect(mx, TRACK_Y + 1, MARKER_W, TRACK_H - 2, true);

    let left = format!("{game}%");
    let right = format!("{chat}%");
    fb.draw_text(4, 42, &left, 2);
    let rw = Framebuffer::text_width(&right, 2) as isize;
    fb.draw_text(WIDTH as isize - rw - 4, 42, &right, 2);
    fb
}

/// Transient volume popup: speaker glyph, big percentage and a progress bar.
pub fn render_volume_overlay(volume: u8, muted: bool) -> Framebuffer {
    let mut fb = Framebuffer::new();
    let volume = volume.min(100);

    speaker_glyph(&mut fb, 4, 24, volume, muted);

    let label = format!("{volume}%");
    let lw = Framebuffer::text_width(&label, 3) as isize;
    fb.draw_text(WIDTH as isize - lw - 4, 13, &label, 3);

    const BAR_X: isize = 4;
    const BAR_Y: isize = 46;
    const BAR_W: usize = 120;
    const BAR_H: usize = 14;
    fb.stroke_rect(BAR_X, BAR_Y, BAR_W, BAR_H);
    let inner_w = BAR_W - 2;
    let filled = (volume as usize * inner_w) / 100;
    if muted {
        // Hatched rather than solid: the level is still meaningful while
        // muted, but it must not read as "audible".
        for dx in 0..filled as isize {
            for dy in 0..BAR_H as isize - 2 {
                if (dx + dy) % 2 == 0 {
                    fb.set(BAR_X + 1 + dx, BAR_Y + 1 + dy, true);
                }
            }
        }
    } else {
        fb.fill_rect(BAR_X + 1, BAR_Y + 1, filled, BAR_H - 2, true);
    }
    fb
}

/// A speaker built from rectangles, centred vertically on `cy`. The number of
/// wave bars tracks the level; muted swaps them for a cross.
fn speaker_glyph(fb: &mut Framebuffer, x: isize, cy: isize, volume: u8, muted: bool) {
    // Driver box plus a cone that widens away from it.
    fb.fill_rect(x, cy - 4, 5, 8, true);
    for i in 0..9isize {
        fb.fill_rect(x + 5 + i, cy - (i + 1), 1, (i as usize + 1) * 2, true);
    }

    if muted {
        // Cross over the waves' area.
        for k in 0..9isize {
            fb.set(x + 18 + k, cy - 4 + k, true);
            fb.set(x + 19 + k, cy - 4 + k, true);
            fb.set(x + 18 + k, cy + 4 - k, true);
            fb.set(x + 19 + k, cy + 4 - k, true);
        }
        return;
    }

    // Three arcs, lit progressively: quiet still shows one so the glyph never
    // looks broken at 1%.
    for (i, threshold) in [1u8, 34, 67].into_iter().enumerate() {
        if volume < threshold {
            break;
        }
        let h = 6 + i * 6;
        fb.fill_rect(x + 17 + i as isize * 4, cy - (h as isize) / 2, 2, h, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_level_store_renders_placeholder() {
        let mut meters = AudioMeters::new();
        let store = LevelStore::new();
        // Must not panic and must produce a non-blank frame.
        let fb = meters.render_vu(&store);
        assert!(!fb.frame_packets().is_empty());
    }

    #[test]
    fn short_name_strips_prefix_and_caps_length() {
        assert_eq!(short_name("sink_game"), "GAME");
        assert_eq!(short_name("sink_headphones"), "HEADP");
        assert_eq!(short_name("mic"), "MIC");
    }

    #[test]
    fn db_norm_spans_the_floor() {
        assert_eq!(db_norm(0.0), 0.0);
        assert_eq!(db_norm(f32::NAN), 0.0);
        assert!((db_norm(1.0) - 1.0).abs() < 1e-6);
        assert!(db_norm(0.5) > 0.8, "-6 dB should sit near the top");
    }

    #[test]
    fn envelope_decays_after_a_transient() {
        let mut meters = AudioMeters::new();
        let store = LevelStore::new();
        let slot = match store.slot_for("sink_game") {
            Some(s) => s,
            None => return,
        };
        store.raise(slot, 0, 1.0);
        meters.render_vu(&store);
        let loud = meters.meters[slot].env;
        // Nothing raised since: the next frames must fall, not hold.
        for _ in 0..5 {
            meters.render_vu(&store);
        }
        assert!(meters.meters[slot].env < loud);
        assert!(meters.meters[slot].peak >= meters.meters[slot].env);
    }

    #[test]
    fn chatmix_and_volume_handle_missing_or_extreme_input() {
        render_chatmix(None, None);
        render_chatmix(Some(200), Some(0));
        render_volume_overlay(200, true);
        render_volume_overlay(0, false);
    }
}
