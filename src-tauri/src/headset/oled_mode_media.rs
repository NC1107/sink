//! The OLED media screens: now-playing text, album art and a progress bar,
//! all driven by the system `playerctl`.
//!
//! Everything here is called from the OLED render loop at ~24 fps, so nothing
//! expensive may happen on that thread. The two costly operations — the
//! `playerctl` subprocess and decoding the cover image — run on a small
//! background poller thread that publishes ready-to-blit results behind a
//! mutex. Rendering only reads that snapshot and interpolates the playback
//! position from the wall clock, so a frame never waits on a fork+exec or on
//! a JPEG decode.

use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread;
use std::time::{Duration, Instant};

use image::{DynamicImage, GenericImageView};

use super::oled::{Framebuffer, HEIGHT, WIDTH};

/// One `playerctl metadata` call gets everything in a single fork+exec.
const FORMAT: &str =
    "{{title}}\t{{artist}}\t{{album}}\t{{status}}\t{{position}}\t{{mpris:length}}\t{{mpris:artUrl}}";

/// Full metadata poll interval — a subprocess per tick would be absurd.
const META_INTERVAL: Duration = Duration::from_secs(2);
/// Position-only poll: cheaper query that keeps the bar honest in between.
const POS_INTERVAL: Duration = Duration::from_secs(1);
/// How often the poller thread wakes to check whether anything is due. Also
/// bounds how long the thread outlives the `MediaMode` that owns it.
const POLL_TICK: Duration = Duration::from_millis(200);
/// Marquee advance: one character per step (~3 characters per second).
const SCROLL_STEP_MS: u128 = 300;
/// Side of the square album-art thumbnail on the split screen.
const ART_SIZE: usize = 60;
/// Upper bound on the intermediate resize width, so an absurd aspect ratio
/// (a 20000x20 cover) can't turn into a hundred-megabyte allocation.
const MAX_RESIZE_W: u32 = 4096;

/// A snapshot of what the active MPRIS player reports.
#[derive(Clone, Default)]
struct Track {
    title: String,
    artist: String,
    album: String,
    playing: bool,
    /// Position at the moment of the last poll (microseconds).
    position_us: u64,
    /// Track length in microseconds; 0 when the player doesn't report one.
    length_us: u64,
    art_url: String,
}

/// Decoded cover for one `artUrl`. A failed decode is cached as `None` so a
/// broken URL isn't retried until the track changes.
#[derive(Default)]
struct Art {
    url: String,
    /// ART_SIZE x ART_SIZE thumbnail for the split screen.
    thumb: Option<Vec<u8>>,
    /// WIDTH x HEIGHT height-filling rendition for the art-only screen.
    full: Option<Vec<u8>>,
}

/// Everything the poller thread produces and the render path consumes.
#[derive(Default)]
struct Shared {
    track: Option<Track>,
    /// When `track.position_us` was sampled, for interpolating between polls.
    pos_at: Option<Instant>,
    art: Art,
}

/// A poisoned mutex only means some other thread panicked mid-update; the data
/// is still a valid `Shared`, so recover rather than propagate the panic.
fn lock_shared(m: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Now-playing OLED screens. Polling and image decoding happen off-thread.
pub struct MediaMode {
    /// Fixed origin for the marquee so scrolling is wall-clock paced rather
    /// than frame paced (the render loop's rate isn't guaranteed).
    started: Instant,
    shared: Arc<Mutex<Shared>>,
}

impl Default for MediaMode {
    fn default() -> Self {
        Self::new()
    }
}

impl MediaMode {
    pub fn new() -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        // The thread holds only a Weak, so it exits within one POLL_TICK of
        // this MediaMode being dropped.
        let weak = Arc::downgrade(&shared);
        let _ = thread::Builder::new()
            .name("sink-oled-media".into())
            .spawn(move || poll_loop(weak));
        Self {
            started: Instant::now(),
            shared,
        }
    }

    fn state(&self) -> MutexGuard<'_, Shared> {
        lock_shared(&self.shared)
    }

    // ---------------------------------------------------------------- render

    /// Text-only screen: title (marquee), artist, elapsed/total and a bar.
    pub fn render_now_playing(&mut self) -> Framebuffer {
        let phase = self.scroll_phase();
        let mut fb = Framebuffer::new();
        let state = self.state();
        let Some(track) = state.track.as_ref() else {
            fb.draw_text_centered(24, "NO MEDIA", 2);
            return fb;
        };
        let pos = display_position(track, state.pos_at);

        // Title big, artist below it; both windowed to the panel width.
        draw_window(&mut fb, 2, 2, WIDTH - 4, &track.title, 2, phase);
        draw_window(&mut fb, 2, 20, WIDTH - 4, &track.artist, 1, phase);

        // Time row: "1:55 / 2:32" left, transport state right-aligned.
        let time = if track.length_us > 0 {
            format!("{} / {}", fmt_time(pos), fmt_time(track.length_us))
        } else {
            fmt_time(pos)
        };
        fb.draw_text(2, 34, &time, 1);
        let label = if track.playing { "PLAY" } else { "PAUSE" };
        let lw = Framebuffer::text_width(label, 1) as isize;
        fb.draw_text(WIDTH as isize - lw - 2, 34, label, 1);

        progress_bar(&mut fb, 2, 46, WIDTH - 4, 12, fraction(pos, track.length_us));
        fb
    }

    /// Split screen: dithered 60x60 cover on the left, text and bar on the right.
    pub fn render_with_art(&mut self) -> Framebuffer {
        let phase = self.scroll_phase();
        let state = self.state();
        let Some(track) = state.track.as_ref() else {
            let mut fb = Framebuffer::new();
            fb.draw_text_centered(24, "NO MEDIA", 2);
            return fb;
        };
        let pos = display_position(track, state.pos_at);

        // blit_gray_dithered wants a whole frame, so compose the art into a
        // full-size gray buffer (the text half stays black) and dither once.
        let mut gray = vec![0u8; WIDTH * HEIGHT];
        let thumb = state
            .art
            .thumb
            .as_deref()
            .filter(|t| t.len() >= ART_SIZE * ART_SIZE);
        let has_art = thumb.is_some();
        if let Some(thumb) = thumb {
            for y in 0..ART_SIZE {
                for x in 0..ART_SIZE {
                    let (dx, dy) = (x + 2, y + 2);
                    if dx < WIDTH && dy < HEIGHT {
                        gray[dy * WIDTH + dx] = thumb[y * ART_SIZE + x];
                    }
                }
            }
        }
        let mut fb = Framebuffer::new();
        fb.blit_gray_dithered(&gray);
        // Error diffusion can smear a pixel or two past the art; wipe the text
        // half so white-on-black stays clean.
        fb.fill_rect(64, 0, WIDTH - 64, HEIGHT, false);

        if !has_art {
            // Placeholder so a missing cover still reads as "this is the art".
            fb.stroke_rect(2, 2, ART_SIZE, ART_SIZE);
            let w = Framebuffer::text_width("NO ART", 1) as isize;
            fb.draw_text(2 + (ART_SIZE as isize - w) / 2, 28, "NO ART", 1);
        }

        // Right column: 60 px of usable width from x=66.
        let col_x = 66isize;
        let col_w = WIDTH - 68;
        draw_window(&mut fb, col_x, 3, col_w, &track.title, 1, phase);
        draw_window(&mut fb, col_x, 14, col_w, &track.artist, 1, phase);
        draw_window(&mut fb, col_x, 25, col_w, &track.album, 1, phase);
        // No spaces around the slash — 11 characters wouldn't fit the column.
        let time = if track.length_us > 0 {
            format!("{}/{}", fmt_time(pos), fmt_time(track.length_us))
        } else {
            fmt_time(pos)
        };
        draw_window(&mut fb, col_x, 36, col_w, &time, 1, 0);
        progress_bar(&mut fb, col_x, 46, col_w, 7, fraction(pos, track.length_us));
        let label = if track.playing { "PLAYING" } else { "PAUSED" };
        fb.draw_text(col_x, 56, label, 1);
        fb
    }

    /// The cover alone, scaled to fill the panel height and centred.
    pub fn render_art_only(&mut self) -> Framebuffer {
        let mut fb = Framebuffer::new();
        let state = self.state();
        match state
            .art
            .full
            .as_deref()
            .filter(|g| g.len() >= WIDTH * HEIGHT)
        {
            Some(gray) => fb.blit_gray_dithered(gray),
            None => fb.draw_text_centered(24, "NO ART", 2),
        }
        fb
    }

    /// Marquee step counter (one character per `SCROLL_STEP_MS`).
    fn scroll_phase(&self) -> usize {
        (self.started.elapsed().as_millis() / SCROLL_STEP_MS) as usize
    }
}

/// Last polled position advanced by wall-clock time, so the bar moves smoothly
/// at 24 fps instead of stepping once a second.
fn display_position(track: &Track, pos_at: Option<Instant>) -> u64 {
    let mut pos = track.position_us;
    if track.playing {
        if let Some(at) = pos_at {
            pos = pos.saturating_add(at.elapsed().as_micros() as u64);
        }
    }
    if track.length_us > 0 {
        pos.min(track.length_us)
    } else {
        pos
    }
}

// =============================== poller ===============================

/// Background poll loop: `playerctl` and cover decoding, never on the render
/// thread. Exits as soon as the owning `MediaMode` is dropped.
fn poll_loop(shared: Weak<Mutex<Shared>>) {
    let mut meta_at: Option<Instant> = None;
    let mut pos_at = Instant::now();
    loop {
        let Some(state) = shared.upgrade() else {
            return;
        };
        let now = Instant::now();
        let meta_due = meta_at.map_or(true, |at| now.duration_since(at) >= META_INTERVAL);
        if meta_due {
            let track = query_metadata();
            meta_at = Some(now);
            pos_at = now;
            let url = track
                .as_ref()
                .map(|t| t.art_url.clone())
                .unwrap_or_default();
            let art_due = {
                let mut s = lock_shared(&state);
                s.track = track;
                s.pos_at = Some(now);
                s.art.url != url
            };
            // Decode outside the lock: it can take hundreds of milliseconds.
            if art_due {
                let art = decode_art(&url);
                lock_shared(&state).art = art;
            }
        } else if now.duration_since(pos_at) >= POS_INTERVAL {
            let sampled = query_position_us();
            pos_at = now;
            if let Some(us) = sampled {
                let mut s = lock_shared(&state);
                if let Some(track) = s.track.as_mut() {
                    track.position_us = us;
                    s.pos_at = Some(now);
                }
            }
        }
        // Release the strong reference before sleeping so a dropped MediaMode
        // isn't kept alive for a whole tick.
        drop(state);
        thread::sleep(POLL_TICK);
    }
}

/// Decode one cover into both renditions. Failure yields an `Art` with the URL
/// recorded and no buffers, which negatively caches the bad URL.
fn decode_art(url: &str) -> Art {
    let mut art = Art {
        url: url.to_string(),
        thumb: None,
        full: None,
    };
    let Some(path) = art_path(url) else {
        return art;
    };
    let Ok(img) = image::open(path) else {
        return art;
    };
    art.thumb = Some(gray_fit(&img, ART_SIZE, ART_SIZE));
    art.full = Some(gray_fill_height(&img));
    art
}

// =============================== drawing ===============================

/// Draw `text` inside a `win_px`-wide window, marquee-scrolling it when it
/// doesn't fit. Stepping by whole characters (rather than pixels) keeps glyphs
/// inside the window — the framebuffer has no clipping rectangle. Returns the
/// x just past the last glyph drawn.
fn draw_window(
    fb: &mut Framebuffer,
    x: isize,
    y: isize,
    win_px: usize,
    text: &str,
    scale: usize,
    phase: usize,
) -> isize {
    let advance = Framebuffer::text_width(" ", scale).max(1);
    let cap = (win_px / advance).max(1);
    let chars: Vec<char> = ascii(text).chars().collect();
    if chars.is_empty() {
        return x;
    }
    if chars.len() <= cap {
        let line: String = chars.iter().collect();
        return fb.draw_text(x, y, &line, scale);
    }
    // Wrap with a gap so the loop point reads as a marquee, not a jump cut.
    let mut padded = chars;
    padded.extend("   ".chars());
    let start = phase % padded.len();
    let window: String = padded.iter().cycle().skip(start).take(cap).collect();
    fb.draw_text(x, y, &window, scale)
}

/// A hollow bar filled left-to-right by `frac` (0.0..=1.0).
fn progress_bar(fb: &mut Framebuffer, x: isize, y: isize, w: usize, h: usize, frac: f32) {
    fb.stroke_rect(x, y, w, h);
    if w <= 4 || h <= 4 {
        return;
    }
    let inner = w - 4;
    let fill = (inner as f32 * frac.clamp(0.0, 1.0)).round() as usize;
    fb.fill_rect(x + 2, y + 2, fill.min(inner), h - 4, true);
}

/// Elapsed fraction, guarding against players that report no length.
fn fraction(pos_us: u64, len_us: u64) -> f32 {
    if len_us == 0 {
        return 0.0;
    }
    (pos_us as f64 / len_us as f64) as f32
}

/// Microseconds as `m:ss` (or `h:mm:ss` for long tracks).
fn fmt_time(us: u64) -> String {
    let secs = us / 1_000_000;
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// The 5x7 font only has 256 glyphs and track titles are full of typographic
/// punctuation, so fold the common cases and drop everything else to '?'.
fn ascii(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' | '\u{02bc}' => '\'',
            '\u{201c}' | '\u{201d}' => '"',
            '\u{2010}'..='\u{2015}' => '-',
            '\u{00a0}' => ' ',
            c if c.is_ascii_graphic() || c == ' ' => c,
            _ => '?',
        })
        .collect()
}

// =============================== album art ===============================

/// Resolve an `mpris:artUrl` to a local path. Only `file://` (and bare paths)
/// are supported — Sink never fetches remote art.
fn art_path(url: &str) -> Option<PathBuf> {
    if url.is_empty() {
        return None;
    }
    if url.starts_with('/') {
        return Some(PathBuf::from(percent_decode(url)));
    }
    let rest = url.strip_prefix("file://")?;
    let decoded = percent_decode(rest);
    // "file:///path" leaves the leading slash; "file://host/path" is not local.
    if !decoded.starts_with('/') {
        return None;
    }
    Some(PathBuf::from(decoded))
}

/// Minimal percent-decoder (URL escapes only; no crate needed for this).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_nibble(bytes[i + 1]), hex_nibble(bytes[i + 2])) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    // Paths are bytes; lossy conversion keeps a mangled name usable.
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Aspect-preserving fit into `w` x `h`, centred on black.
fn gray_fit(img: &DynamicImage, w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h];
    let (iw, ih) = img.dimensions();
    if iw == 0 || ih == 0 || w == 0 || h == 0 {
        return out;
    }
    let scale = (w as f32 / iw as f32).min(h as f32 / ih as f32);
    let nw = ((iw as f32 * scale).round() as u32).clamp(1, w as u32);
    let nh = ((ih as f32 * scale).round() as u32).clamp(1, h as u32);
    let resized = img
        .resize_exact(nw, nh, image::imageops::FilterType::Triangle)
        .to_luma8();
    let ox = (w - nw as usize) / 2;
    let oy = (h - nh as usize) / 2;
    for y in 0..nh as usize {
        for x in 0..nw as usize {
            out[(oy + y) * w + ox + x] = resized.get_pixel(x as u32, y as u32).0[0];
        }
    }
    out
}

/// Scale to fill the full 64 px height and centre horizontally, cropping the
/// overhang when the cover is wider than the panel.
fn gray_fill_height(img: &DynamicImage) -> Vec<u8> {
    let mut out = vec![0u8; WIDTH * HEIGHT];
    let (iw, ih) = img.dimensions();
    if iw == 0 || ih == 0 {
        return out;
    }
    let nh = HEIGHT as u32;
    let nw = (((iw as f32 / ih as f32) * nh as f32).round() as u32).clamp(1, MAX_RESIZE_W);
    let resized = img
        .resize_exact(nw, nh, image::imageops::FilterType::Triangle)
        .to_luma8();
    let nw = nw as usize;
    // Wider than the panel: crop equally on both sides. Narrower: pillarbox.
    let src_ox = nw.saturating_sub(WIDTH) / 2;
    let dst_ox = WIDTH.saturating_sub(nw) / 2;
    let copy_w = nw.min(WIDTH);
    for y in 0..HEIGHT {
        for x in 0..copy_w {
            out[y * WIDTH + dst_ox + x] = resized.get_pixel((src_ox + x) as u32, y as u32).0[0];
        }
    }
    out
}

// =============================== playerctl ===============================

/// One `playerctl metadata` read. `None` when playerctl is missing, no player
/// is running, or the player reports nothing usable.
fn query_metadata() -> Option<Track> {
    let out = Command::new("playerctl")
        .args(["metadata", "--format", FORMAT])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&out.stdout);
    let line = raw.lines().next()?;
    if line.trim().is_empty() {
        return None;
    }
    let mut fields = line.splitn(7, '\t');
    let mut next = || fields.next().unwrap_or("").trim().to_string();
    let title = next();
    let artist = next();
    let album = next();
    let status = next();
    let position_us = parse_us(&next());
    let length_us = parse_us(&next());
    let art_url = next();
    if title.is_empty() && artist.is_empty() {
        return None;
    }
    Some(Track {
        title,
        artist,
        album,
        playing: status.eq_ignore_ascii_case("Playing"),
        position_us,
        length_us,
        art_url,
    })
}

/// Position-only poll. `playerctl position` prints seconds as a float.
fn query_position_us() -> Option<u64> {
    let out = Command::new("playerctl").arg("position").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&out.stdout);
    let secs: f64 = raw.trim().parse().ok()?;
    if !secs.is_finite() || secs < 0.0 {
        return None;
    }
    Some((secs * 1_000_000.0) as u64)
}

/// Microsecond fields arrive as integers, but some players emit a float or an
/// empty string; anything unparseable means "unknown", i.e. 0.
fn parse_us(s: &str) -> u64 {
    if let Ok(v) = s.parse::<u64>() {
        return v;
    }
    match s.parse::<f64>() {
        Ok(v) if v.is_finite() && v > 0.0 => v as u64,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(percent_decode("/tmp/a%20b%2Fc.png"), "/tmp/a b/c.png");
        // A stray '%' must survive rather than eat the rest of the path.
        assert_eq!(percent_decode("/tmp/100%"), "/tmp/100%");
        assert_eq!(percent_decode("/tmp/%zz"), "/tmp/%zz");
    }

    #[test]
    fn strips_the_file_scheme() {
        assert_eq!(
            art_path("file:///home/u/cover%20art.jpg"),
            Some(PathBuf::from("/home/u/cover art.jpg"))
        );
        assert_eq!(art_path("https://example.com/a.jpg"), None);
        assert_eq!(art_path(""), None);
    }

    #[test]
    fn formats_microseconds() {
        assert_eq!(fmt_time(115_000_000), "1:55");
        assert_eq!(fmt_time(0), "0:00");
        assert_eq!(fmt_time(3_723_000_000), "1:02:03");
    }

    #[test]
    fn windowed_text_never_overflows() {
        let mut fb = Framebuffer::new();
        // A long title in a 60 px window at scale 1, at every marquee phase.
        for phase in 0..64 {
            let end = draw_window(&mut fb, 66, 3, 60, "a very long track title", 1, phase);
            assert!(end <= 66 + 60, "phase {phase} drew past the window");
        }
        // Scale 2 in the full-width window must stay on the panel too.
        let end = draw_window(&mut fb, 2, 2, WIDTH - 4, "another overlong title", 2, 5);
        assert!(end <= WIDTH as isize - 2);
        // A short string is drawn as-is.
        assert_eq!(draw_window(&mut fb, 0, 0, 60, "ab", 1, 3), 12);
    }

    #[test]
    fn fraction_and_position_are_divide_safe() {
        assert_eq!(fraction(1_000_000, 0), 0.0);
        assert_eq!(fraction(1_000_000, 2_000_000), 0.5);
        let paused = Track {
            position_us: 500,
            length_us: 400,
            playing: false,
            ..Track::default()
        };
        // Clamped to the reported length, and no drift while paused.
        assert_eq!(display_position(&paused, Some(Instant::now())), 400);
        assert_eq!(display_position(&paused, None), 400);
    }

    #[test]
    fn parses_odd_microsecond_fields() {
        assert_eq!(parse_us("1500000"), 1_500_000);
        assert_eq!(parse_us("1.5e6"), 1_500_000);
        assert_eq!(parse_us(""), 0);
        assert_eq!(parse_us("-3"), 0);
        assert_eq!(parse_us("nan"), 0);
    }
}
