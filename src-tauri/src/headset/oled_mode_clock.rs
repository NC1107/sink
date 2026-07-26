//! Clock and timer screens for the OLED.
//!
//! Civil-time conversion is done here by hand (Howard Hinnant's days<->y/m/d
//! algorithm) instead of pulling in `chrono`: the only thing we cannot compute
//! from `SystemTime` alone is the local UTC offset, and that we ask libc for
//! once in a while. Everything else — including the per-frame seconds tick —
//! is arithmetic, so the render path stays cheap enough for the 24 fps
//! rotation loop.

// Not every screen here is wired into the rotation yet; keep the full set
// available without warning noise.
#![allow(dead_code)]

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::oled::{Framebuffer, HEIGHT, WIDTH};

const WEEKDAYS: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];
const MONTHS: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];

/// How often the UTC offset is re-probed. The offset only moves at DST
/// boundaries (or when `TZ` changes under us), so a slow poll is plenty.
const TZ_REFRESH: Duration = Duration::from_secs(30);

// ===================== civil time =====================

/// Local wall-clock time, already broken down into fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Civil {
    pub year: i32,
    pub month: u32, // 1..=12
    pub day: u32,   // 1..=31
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// 0 = Sunday .. 6 = Saturday.
    pub weekday: usize,
    /// Milliseconds within the current second — drives the colon blink.
    pub millis: u32,
}

impl Civil {
    /// Break a Unix timestamp (already shifted into local time) into fields.
    fn from_unix(secs: i64, millis: u32) -> Self {
        // Euclidean div/rem so pre-1970 timestamps stay sane.
        let days = secs.div_euclid(86_400);
        let tod = secs.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        Self {
            year,
            month,
            day,
            hour: (tod / 3600) as u32,
            minute: ((tod / 60) % 60) as u32,
            second: (tod % 60) as u32,
            // 1970-01-01 was a Thursday, hence the +4.
            weekday: (days + 4).rem_euclid(7) as usize,
            millis,
        }
    }

    fn weekday_name(&self) -> &'static str {
        WEEKDAYS.get(self.weekday).copied().unwrap_or("---")
    }

    fn month_name(&self) -> &'static str {
        self.month
            .checked_sub(1)
            .and_then(|i| MONTHS.get(i as usize))
            .copied()
            .unwrap_or("---")
    }

    /// "SAT 26 JUL 2026".
    fn long_date(&self) -> String {
        format!(
            "{} {:02} {} {}",
            self.weekday_name(),
            self.day,
            self.month_name(),
            self.year
        )
    }

    /// "HH:MM", with the colon blanked on the off half of the blink cycle.
    fn hh_mm(&self, colon: bool) -> String {
        format!(
            "{:02}{}{:02}",
            self.hour,
            if colon { ':' } else { ' ' },
            self.minute
        )
    }

    /// Colon lit for the first half of every second.
    fn blink(&self) -> bool {
        self.millis < 500
    }
}

/// Seconds and milliseconds since the Unix epoch, without unwrapping.
fn unix_now() -> (i64, u32) {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => (d.as_secs() as i64, d.subsec_millis()),
        // Clock set before 1970: negate the backwards offset rather than panic.
        Err(e) => (-(e.duration().as_secs() as i64), 0),
    }
}

/// Days since 1970-01-01 for a proleptic-Gregorian date (Hinnant).
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    // March-based year: the leap day lands at the end, killing all the
    // special-casing.
    let y = y as i64 - i64::from(m <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Inverse of [`days_from_civil`].
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((y + i64::from(m <= 2)) as i32, m, d)
}

/// Cached UTC offset. Probing touches libc's timezone state, so it is
/// amortised over [`TZ_REFRESH`]; the timestamp is bumped even when the probe
/// fails so a failing lookup isn't retried on every frame.
struct TzCache {
    offset_secs: i64,
    checked_at: Option<Instant>,
}

impl TzCache {
    fn new() -> Self {
        Self {
            offset_secs: 0,
            checked_at: None,
        }
    }

    fn offset(&mut self) -> i64 {
        let fresh = matches!(self.checked_at, Some(t) if t.elapsed() < TZ_REFRESH);
        if !fresh {
            if let Some(off) = probe_utc_offset() {
                self.offset_secs = off;
            }
            self.checked_at = Some(Instant::now());
        }
        self.offset_secs
    }
}

/// Local UTC offset, straight from libc. `localtime_r` honours `TZ`,
/// `/etc/localtime` and DST without a tzdata parser — and without forking a
/// `date`, which has no business running on a 24 fps render path. Falls back
/// to UTC (offset 0) if the lookup fails.
fn probe_utc_offset() -> Option<i64> {
    let (utc, _) = unix_now();
    let secs = utc as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: `localtime_r` writes through a pointer to a `tm` we own for the
    // whole call and never retains it; a null return means it failed. It reads
    // libc's timezone database itself (lazily, then cached), so DST changes are
    // still picked up by the periodic re-probe.
    let ok = unsafe { !libc::localtime_r(&secs, &mut tm).is_null() };
    if !ok {
        return None;
    }
    Some(tm.tm_gmtoff as i64)
}

// ===================== drawing helpers =====================

/// Bresenham line — the hands of the analog face and the tick marks.
fn line(fb: &mut Framebuffer, x0: isize, y0: isize, x1: isize, y1: isize) {
    let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
    let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
    let (mut x, mut y, mut err) = (x0, y0, dx + dy);
    loop {
        fb.set(x, y, true);
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

fn circle(fb: &mut Framebuffer, cx: isize, cy: isize, r: isize, fill: bool) {
    for dy in -r..=r {
        for dx in -r..=r {
            let d2 = dx * dx + dy * dy;
            let on = if fill { d2 <= r * r } else { (d2 - r * r).abs() <= r };
            if on {
                fb.set(cx + dx, cy + dy, true);
            }
        }
    }
}

/// Left edge that centres `text` by its *ink* width. `text_width` counts the
/// trailing inter-character gap, which pushes text a pixel or two left of true
/// centre at large scales.
fn centred_x(text: &str, scale: usize) -> isize {
    let scale = scale.max(1);
    let ink = Framebuffer::text_width(text, scale).saturating_sub(scale);
    (WIDTH as isize - ink as isize) / 2
}

/// Largest scale (capped at `max_scale`) whose ink fits in `max_w`.
fn fit_scale(text: &str, max_w: usize, max_scale: usize) -> usize {
    (1..=max_scale.max(1))
        .rev()
        .find(|s| Framebuffer::text_width(text, *s).saturating_sub(*s) <= max_w)
        .unwrap_or(1)
}

// ===================== Clock =====================

/// The clock screens. Holds only the timezone cache, so it is cheap to keep
/// around and cheap to render from.
pub struct Clock {
    tz: TzCache,
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock {
    pub fn new() -> Self {
        Self { tz: TzCache::new() }
    }

    /// Current local time. Only the offset lookup is cached; the fields
    /// themselves are recomputed per call so seconds tick smoothly.
    pub fn now(&mut self) -> Civil {
        let (utc, millis) = unix_now();
        Civil::from_unix(utc + self.tz.offset(), millis)
    }

    /// Big HH:MM with a blinking colon, small seconds beside it, weekday and
    /// date underneath.
    pub fn render_digital(&mut self) -> Framebuffer {
        let t = self.now();
        let mut fb = Framebuffer::new();

        let hm = t.hh_mm(t.blink());
        let ss = format!("{:02}", t.second);
        // Centre the (big time + small seconds) group as a whole.
        let hm_ink = Framebuffer::text_width(&hm, 3).saturating_sub(3) as isize;
        let ss_ink = Framebuffer::text_width(&ss, 2).saturating_sub(2) as isize;
        let x0 = (WIDTH as isize - (hm_ink + 4 + ss_ink)) / 2;
        fb.draw_text(x0, 11, &hm, 3);
        // Seconds sit on the same baseline as the big digits.
        fb.draw_text(x0 + hm_ink + 4, 11 + 21 - 14, &ss, 2);

        fb.fill_rect(8, 36, 112, 1, true);
        let date = t.long_date();
        fb.draw_text(centred_x(&date, 1), 43, &date, 1);
        fb
    }

    /// HH:MM as large as the panel allows (scale 4 is the widest that fits).
    pub fn render_big(&mut self) -> Framebuffer {
        let t = self.now();
        let mut fb = Framebuffer::new();
        let hm = t.hh_mm(true);
        let scale = fit_scale(&hm, WIDTH - 4, 4);
        let h = 7 * scale as isize;
        fb.draw_text(centred_x(&hm, scale), (HEIGHT as isize - h) / 2, &hm, scale);
        fb
    }

    /// Time on top, the date spelled out large underneath.
    pub fn render_with_date(&mut self) -> Framebuffer {
        let t = self.now();
        let mut fb = Framebuffer::new();

        let hm = t.hh_mm(t.blink());
        let ss = format!("{:02}", t.second);
        let hm_ink = Framebuffer::text_width(&hm, 3).saturating_sub(3) as isize;
        let ss_ink = Framebuffer::text_width(&ss, 1).saturating_sub(1) as isize;
        let x0 = (WIDTH as isize - (hm_ink + 3 + ss_ink)) / 2;
        fb.draw_text(x0, 3, &hm, 3);
        fb.draw_text(x0 + hm_ink + 3, 3 + 21 - 7, &ss, 1);

        fb.fill_rect(4, 27, 120, 1, true);
        // "SAT 26 JUL 2026" won't fit at scale 2, so the year gets its own row.
        let head = format!("{} {:02} {}", t.weekday_name(), t.day, t.month_name());
        let scale = fit_scale(&head, WIDTH - 4, 2);
        fb.draw_text(centred_x(&head, scale), 31, &head, scale);
        let year = t.year.to_string();
        fb.draw_text(centred_x(&year, 2), 48, &year, 2);
        fb
    }

    /// Analog face with hour ticks and three hands, flanked by the date and a
    /// small digital read-out in the space the circle leaves free.
    pub fn render_analog(&mut self) -> Framebuffer {
        let t = self.now();
        let mut fb = Framebuffer::new();
        let (cx, cy, r) = (64isize, 32isize, 30isize);
        circle(&mut fb, cx, cy, r, false);

        // Hour ticks; the quarters are drawn longer so the face reads at a
        // glance on a 1bpp panel.
        for k in 0..12 {
            let a = tick_angle(k as f32 * 30.0);
            let inner = if k % 3 == 0 { r - 6 } else { r - 3 };
            let (x0, y0) = polar(cx, cy, inner, a);
            let (x1, y1) = polar(cx, cy, r - 1, a);
            line(&mut fb, x0, y0, x1, y1);
        }

        let hour_a = tick_angle((t.hour % 12) as f32 * 30.0 + t.minute as f32 * 0.5);
        let min_a = tick_angle(t.minute as f32 * 6.0 + t.second as f32 * 0.1);
        let sec_a = tick_angle(t.second as f32 * 6.0);

        // Hour hand doubled with a 1px offset so it out-weighs the minute hand.
        let (hx, hy) = polar(cx, cy, 14, hour_a);
        line(&mut fb, cx, cy, hx, hy);
        line(&mut fb, cx + 1, cy, hx + 1, hy);
        line(&mut fb, cx, cy + 1, hx, hy + 1);

        let (mx, my) = polar(cx, cy, 21, min_a);
        line(&mut fb, cx, cy, mx, my);

        let (sx, sy) = polar(cx, cy, 26, sec_a);
        line(&mut fb, cx, cy, sx, sy);

        circle(&mut fb, cx, cy, 2, true);

        // The circle spans x 34..94, leaving two usable columns.
        fb.draw_text(3, 22, t.weekday_name(), 1);
        fb.draw_text(9, 34, &format!("{:02}", t.day), 1);
        fb.draw_text(WIDTH as isize - 21, 22, t.month_name(), 1);
        let hm = t.hh_mm(true);
        let hm_ink = Framebuffer::text_width(&hm, 1).saturating_sub(1) as isize;
        fb.draw_text(WIDTH as isize - hm_ink - 2, 34, &hm, 1);
        fb
    }
}

/// Clock angle in radians: 0 degrees points at 12 o'clock, growing clockwise.
fn tick_angle(deg: f32) -> f32 {
    (deg - 90.0).to_radians()
}

fn polar(cx: isize, cy: isize, r: isize, a: f32) -> (isize, isize) {
    (
        cx + (a.cos() * r as f32).round() as isize,
        cy + (a.sin() * r as f32).round() as isize,
    )
}

// ===================== Timer =====================

#[derive(Clone, Copy, PartialEq, Eq)]
enum TimerMode {
    Idle,
    /// Counting down from `total` seconds.
    Countdown { total: u64 },
    Stopwatch,
}

/// A countdown / stopwatch driven by [`Instant`], so it is immune to the wall
/// clock being stepped by NTP or a DST change.
pub struct Timer {
    mode: TimerMode,
    /// `Some` while running; `None` while paused or idle.
    running_since: Option<Instant>,
    /// Time banked by previous run segments.
    banked: Duration,
}

impl Default for Timer {
    fn default() -> Self {
        Self::new()
    }
}

impl Timer {
    pub fn new() -> Self {
        Self {
            mode: TimerMode::Idle,
            running_since: None,
            banked: Duration::ZERO,
        }
    }

    pub fn start_countdown(&mut self, secs: u64) {
        self.mode = TimerMode::Countdown { total: secs.max(1) };
        self.banked = Duration::ZERO;
        self.running_since = Some(Instant::now());
    }

    pub fn start_stopwatch(&mut self) {
        self.mode = TimerMode::Stopwatch;
        self.banked = Duration::ZERO;
        self.running_since = Some(Instant::now());
    }

    /// Pause a running timer, or resume a paused one. No-op while idle.
    pub fn toggle_pause(&mut self) {
        match self.running_since.take() {
            Some(start) => self.banked += start.elapsed(),
            None if self.mode != TimerMode::Idle => self.running_since = Some(Instant::now()),
            None => {}
        }
    }

    /// Zero the elapsed time and stop, keeping the configured mode so the same
    /// countdown can simply be re-started with [`Self::toggle_pause`].
    pub fn reset(&mut self) {
        self.banked = Duration::ZERO;
        self.running_since = None;
    }

    pub fn is_running(&self) -> bool {
        self.running_since.is_some()
    }

    /// True once a countdown has hit zero (never true for the stopwatch).
    pub fn is_done(&self) -> bool {
        match self.mode {
            TimerMode::Countdown { total } => self.elapsed().as_secs() >= total,
            _ => false,
        }
    }

    fn elapsed(&self) -> Duration {
        match self.running_since {
            Some(start) => self.banked + start.elapsed(),
            None => self.banked,
        }
    }

    pub fn render(&self) -> Framebuffer {
        let mut fb = Framebuffer::new();
        let elapsed = self.elapsed();

        let label = match (self.mode, self.running_since.is_some()) {
            (TimerMode::Idle, _) => "TIMER IDLE",
            (TimerMode::Countdown { .. }, true) => "COUNTDOWN",
            (TimerMode::Countdown { .. }, false) => "COUNTDOWN PAUSED",
            (TimerMode::Stopwatch, true) => "STOPWATCH",
            (TimerMode::Stopwatch, false) => "STOPWATCH PAUSED",
        };
        fb.draw_text(centred_x(label, 1), 3, label, 1);

        match self.mode {
            TimerMode::Idle => {
                let big = "00:00";
                let scale = fit_scale(big, WIDTH - 4, 4);
                fb.draw_text(centred_x(big, scale), 17, big, scale);
                let hint = "PRESS TO START";
                fb.draw_text(centred_x(hint, 1), 52, hint, 1);
            }
            TimerMode::Countdown { total } => {
                let done = elapsed.as_secs() >= total;
                let remaining = total.saturating_sub(elapsed.as_secs());
                if done {
                    // Blink so an expired timer is noticeable across the room.
                    let (_, millis) = unix_now();
                    if millis < 600 {
                        let big = "DONE";
                        let scale = fit_scale(big, WIDTH - 4, 4);
                        fb.draw_text(centred_x(big, scale), 17, big, scale);
                    }
                } else {
                    let big = clock_string(remaining);
                    let scale = fit_scale(&big, WIDTH - 4, 4);
                    fb.draw_text(centred_x(&big, scale), 17, &big, scale);
                }
                // Draining progress bar: full at the start, empty at zero.
                fb.stroke_rect(8, 50, 112, 11);
                let inner = 108u64;
                let filled = if done {
                    0
                } else {
                    (remaining.saturating_mul(inner) / total.max(1)).min(inner)
                };
                if filled > 0 {
                    fb.fill_rect(10, 52, filled as usize, 7, true);
                }
            }
            TimerMode::Stopwatch => {
                let big = clock_string(elapsed.as_secs());
                let scale = fit_scale(&big, WIDTH - 4, 4);
                fb.draw_text(centred_x(&big, scale), 17, &big, scale);
                // Hundredths make a paused stopwatch look alive and precise.
                let cs = format!(".{:02}", elapsed.subsec_millis() / 10);
                fb.draw_text(centred_x(&cs, 2), 49, &cs, 2);
            }
        }
        fb
    }
}

/// MM:SS, promoted to H:MM:SS past the hour so long runs stay readable.
fn clock_string(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_roundtrips_through_days() {
        for (y, m, d) in [(1970, 1, 1), (2000, 2, 29), (2026, 7, 26), (2100, 12, 31)] {
            let days = days_from_civil(y, m, d);
            assert_eq!(civil_from_days(days), (y, m, d));
        }
    }

    #[test]
    fn epoch_is_a_thursday_at_midnight() {
        let c = Civil::from_unix(0, 0);
        assert_eq!((c.year, c.month, c.day), (1970, 1, 1));
        assert_eq!((c.hour, c.minute, c.second), (0, 0, 0));
        assert_eq!(c.weekday_name(), "THU");
    }

    #[test]
    fn formats_local_fields() {
        // 2026-07-26 13:45:07 UTC, a Sunday.
        let c = Civil::from_unix(1_785_073_507, 0);
        assert_eq!(c.long_date(), "SUN 26 JUL 2026");
        assert_eq!(c.hh_mm(true), "13:45");
        assert_eq!(c.hh_mm(false), "13 45");
    }

    #[test]
    fn big_time_always_fits_the_panel() {
        for text in ["00:00", "59:59", "9:59:59"] {
            let scale = fit_scale(text, WIDTH - 4, 4);
            assert!(Framebuffer::text_width(text, scale) - scale <= WIDTH - 4);
        }
    }

    #[test]
    fn countdown_reports_done_only_after_expiry() {
        let mut t = Timer::new();
        assert!(!t.is_done());
        t.start_countdown(60);
        assert!(t.is_running() && !t.is_done());
        t.toggle_pause();
        assert!(!t.is_running());
        t.reset();
        assert_eq!(t.elapsed(), Duration::ZERO);
    }
}
