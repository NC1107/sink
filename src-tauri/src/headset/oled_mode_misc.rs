//! Three self-contained OLED screens that don't belong to the headset itself:
//! local weather (wttr.in), the wireless mouse battery, and the list of apps
//! currently making noise.
//!
//! The weather screen is the only one with an expensive data source, so it owns
//! a cache and refreshes on a background thread — every `render()` here has to
//! stay cheap enough to be called at the panel's ~24 fps.

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::oled::{Framebuffer, HEIGHT, WIDTH};

/// Normal spacing between successful wttr.in polls.
const WEATHER_REFRESH: Duration = Duration::from_secs(15 * 60);
/// Shorter spacing while we still have nothing to show (offline at boot).
const WEATHER_RETRY: Duration = Duration::from_secs(2 * 60);
/// The panel font only covers the 256-glyph GLCD table; 0x7f reads as a degree
/// sign, which is the closest thing we have to `°`.
const DEG: char = '\u{7f}';

// ===================== shared drawing helpers =====================

/// Bresenham line (same routine as `clips.rs`, kept local so the two screens
/// stay independent of the animation module).
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

/// Filled circle.
fn disc(fb: &mut Framebuffer, cx: isize, cy: isize, r: isize) {
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy <= r * r {
                fb.set(cx + dx, cy + dy, true);
            }
        }
    }
}

/// 1px circle outline.
fn ring(fb: &mut Framebuffer, cx: isize, cy: isize, r: isize) {
    for dy in -r..=r {
        for dx in -r..=r {
            if (dx * dx + dy * dy - r * r).abs() <= r {
                fb.set(cx + dx, cy + dy, true);
            }
        }
    }
}

/// Strip anything the 5x7 font can't draw. External strings (weather
/// descriptions, app names, device models) routinely carry UTF-8 that would
/// otherwise render as holes in the text.
fn ascii(s: &str) -> String {
    s.chars()
        .map(|c| if (' '..='~').contains(&c) { c } else { '?' })
        .collect()
}

/// Clamp a string to `max` characters, marking the cut with an ASCII ellipsis.
fn ellipsize(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return chars.into_iter().collect();
    }
    if max <= 3 {
        return chars.into_iter().take(max).collect();
    }
    let mut out: String = chars.into_iter().take(max - 3).collect();
    out.push_str("...");
    out
}

/// Draw `text` so its right edge sits at `right`.
fn draw_text_right(fb: &mut Framebuffer, right: isize, y: isize, text: &str, scale: usize) {
    let w = Framebuffer::text_width(text, scale) as isize;
    fb.draw_text(right - w, y, text, scale);
}

/// Recover a poisoned mutex instead of panicking: a torn weather reading is
/// never worth taking the render thread down for.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ===================== 1. weather =====================

/// One decoded wttr.in reading. Everything the screen can draw.
#[derive(Clone)]
struct WeatherData {
    temp_c: i32,
    feels_c: i32,
    desc: String,
    humidity: u8,
    wind_kph: u32,
}

#[derive(Default)]
struct WeatherState {
    data: Option<WeatherData>,
    /// When we last *started* a fetch — set up-front so a slow curl can't
    /// trigger a second one.
    last_attempt: Option<Instant>,
}

/// The weather screen. Holds its own cache; `render()` never blocks on IO.
pub struct Weather {
    state: Arc<Mutex<WeatherState>>,
    fetching: Arc<AtomicBool>,
}

impl Default for Weather {
    fn default() -> Self {
        Self::new()
    }
}

impl Weather {
    pub fn new() -> Self {
        let w = Weather {
            state: Arc::new(Mutex::new(WeatherState::default())),
            fetching: Arc::new(AtomicBool::new(false)),
        };
        // Kick off the first poll immediately so the screen has data by the
        // time the user actually cycles to it.
        w.maybe_refresh();
        w
    }

    /// Spawn a fetch if the cache is stale and none is already in flight.
    fn maybe_refresh(&self) {
        let mut st = lock(&self.state);
        let interval = if st.data.is_some() { WEATHER_REFRESH } else { WEATHER_RETRY };
        let due = match st.last_attempt {
            None => true,
            Some(t) => t.elapsed() >= interval,
        };
        if !due {
            return;
        }
        // compare_exchange rather than load/store: two render threads must not
        // both win the race and launch curl twice.
        if self
            .fetching
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        st.last_attempt = Some(Instant::now());
        drop(st);

        let state = Arc::clone(&self.state);
        let fetching = Arc::clone(&self.fetching);
        // Detached on purpose: the panel keeps drawing the previous reading
        // (or the placeholder) while this runs.
        std::thread::spawn(move || {
            let fetched = fetch_weather();
            {
                let mut st = lock(&state);
                // Keep the last good reading on failure — stale is friendlier
                // than blanking the screen on a flaky link.
                if fetched.is_some() {
                    st.data = fetched;
                }
            }
            fetching.store(false, Ordering::Release);
        });
    }

    pub fn render(&mut self) -> Framebuffer {
        self.maybe_refresh();
        let data = lock(&self.state).data.clone();
        let mut fb = Framebuffer::new();

        let Some(w) = data else {
            fb.draw_text_centered(16, "WEATHER", 2);
            fb.draw_text_centered(38, "unavailable", 1);
            return fb;
        };

        // Pictogram on the left, the numbers to its right.
        draw_weather_glyph(&mut fb, Sky::from_desc(&w.desc), 18, 17);
        fb.draw_text(38, 6, &format!("{}{DEG}", w.temp_c), 3);
        fb.draw_text(38, 30, &format!("FEELS {}{DEG}", w.feels_c), 1);

        fb.draw_text_centered(42, &ellipsize(&w.desc, 21), 1);

        // Footer: humidity left, wind right, under a rule.
        let footer = HEIGHT as isize - 9;
        for x in 0..WIDTH as isize {
            fb.set(x, footer - 4, true);
        }
        fb.draw_text(2, footer, &format!("HUM {}%", w.humidity), 1);
        draw_text_right(&mut fb, WIDTH as isize - 2, footer, &format!("{} KPH", w.wind_kph), 1);
        fb
    }
}

/// The four pictograms the description can map to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sky {
    Sun,
    Cloud,
    Rain,
    Snow,
}

impl Sky {
    /// wttr.in descriptions are free text ("Patchy light rain with thunder"),
    /// so keyword-match from the most specific condition downward.
    fn from_desc(desc: &str) -> Sky {
        let d = desc.to_ascii_lowercase();
        let has = |needles: &[&str]| needles.iter().any(|n| d.contains(n));
        if has(&["snow", "sleet", "blizzard", "ice", "icy", "freezing"]) {
            Sky::Snow
        } else if has(&["rain", "drizzle", "shower", "thunder", "storm"]) {
            Sky::Rain
        } else if has(&["cloud", "overcast", "fog", "mist", "haze", "smoke"]) {
            Sky::Cloud
        } else {
            Sky::Sun
        }
    }
}

/// A puffy cloud roughly 24x16, centred on (cx, cy).
fn cloud(fb: &mut Framebuffer, cx: isize, cy: isize) {
    disc(fb, cx - 7, cy + 1, 5);
    disc(fb, cx + 6, cy + 1, 5);
    disc(fb, cx - 1, cy - 3, 7);
    fb.fill_rect(cx - 11, cy + 1, 22, 6, true);
}

/// Draw the weather pictogram inside a ~30x30 box centred on (cx, cy).
fn draw_weather_glyph(fb: &mut Framebuffer, sky: Sky, cx: isize, cy: isize) {
    match sky {
        Sky::Sun => {
            disc(fb, cx, cy, 7);
            // Eight rays, drawn as short spokes outside the disc.
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                let (sx, sy) = (cx + (a.cos() * 10.0) as isize, cy + (a.sin() * 10.0) as isize);
                let (ex, ey) = (cx + (a.cos() * 13.0) as isize, cy + (a.sin() * 13.0) as isize);
                line(fb, sx, sy, ex, ey);
            }
        }
        Sky::Cloud => cloud(fb, cx, cy),
        Sky::Rain => {
            cloud(fb, cx, cy - 4);
            // Three slanted streaks falling out of the cloud base.
            for k in -1..=1isize {
                let x = cx + k * 8;
                line(fb, x + 2, cy + 6, x - 1, cy + 13);
            }
        }
        Sky::Snow => {
            cloud(fb, cx, cy - 4);
            // Three six-point flakes.
            for k in -1..=1isize {
                let (x, y) = (cx + k * 8, cy + 10);
                line(fb, x - 2, y, x + 2, y);
                line(fb, x, y - 2, x, y + 2);
                line(fb, x - 2, y - 2, x + 2, y + 2);
                line(fb, x - 2, y + 2, x + 2, y - 2);
            }
        }
    }
}

/// One-shot wttr.in poll. Runs on the background thread only.
fn fetch_weather() -> Option<WeatherData> {
    // curl rather than a HTTP crate: no new dependency, and `--max-time` caps
    // the worst case at 6s even when the network black-holes us.
    let out = Command::new("curl")
        .args(["-s", "--max-time", "6", "https://wttr.in/?format=j1"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_weather(&String::from_utf8_lossy(&out.stdout))
}

/// Decode the handful of fields we need out of wttr.in's `j1` payload. Every
/// value in that format is a *string*, including the numbers.
fn parse_weather(json: &str) -> Option<WeatherData> {
    let root: Value = serde_json::from_str(json).ok()?;
    let cur = root.get("current_condition")?.get(0)?;
    let field = |key: &str| cur.get(key).and_then(Value::as_str).unwrap_or_default();

    let desc = cur
        .get("weatherDesc")
        .and_then(|d| d.get(0))
        .and_then(|d| d.get("value"))
        .and_then(Value::as_str)
        .map(ascii)
        .unwrap_or_else(|| "?".to_string());

    let temp_c = field("temp_C").trim().parse::<i32>().ok()?;
    Some(WeatherData {
        temp_c,
        // Feels-like is cosmetic; fall back to the real temperature.
        feels_c: field("FeelsLikeC").trim().parse().unwrap_or(temp_c),
        desc: desc.trim().to_string(),
        humidity: field("humidity").trim().parse().unwrap_or(0),
        wind_kph: field("windspeedKmph").trim().parse().unwrap_or(0),
    })
}

// ===================== 2. mouse battery =====================

/// Mouse battery screen: a big battery glyph, the level in large text and the
/// model underneath. `percent == None` means no mouse is paired/awake.
pub fn render_mouse_battery(percent: Option<u8>, charging: bool, model: &str) -> Framebuffer {
    let mut fb = Framebuffer::new();
    fb.draw_text(2, 0, "MOUSE", 1);
    if charging {
        draw_text_right(&mut fb, WIDTH as isize - 2, 0, "CHG", 1);
    }

    let name = ellipsize(ascii(model).trim(), 21);

    let Some(pct) = percent else {
        fb.draw_text_centered(26, "NO MOUSE", 2);
        return fb;
    };
    let pct = pct.min(100);

    // Battery body + terminal nub, leaving the right third for the number.
    let (bx, by, bw, bh) = (4isize, 12isize, 64usize, 32usize);
    fb.stroke_rect(bx, by, bw, bh);
    fb.fill_rect(bx + bw as isize, by + 11, 4, 10, true);
    let inner_w = bw - 6;
    let fill = (inner_w as u32 * pct as u32 / 100) as usize;
    fb.fill_rect(bx + 3, by + 3, fill, bh - 6, true);

    // Bolt sits in a punched-out well so it stays legible whether or not the
    // fill has reached it.
    if charging {
        let (cx, cy) = (bx + bw as isize / 2, by + bh as isize / 2);
        fb.fill_rect(cx - 7, cy - 10, 15, 20, false);
        bolt(&mut fb, cx, cy);
    }

    draw_text_right(&mut fb, WIDTH as isize - 2, 20, &format!("{pct}%"), 2);
    fb.draw_text_centered(52, &name, 1);
    fb
}

/// Lightning-bolt pixel art, 11x16. Hand-placed rather than computed: two
/// slanted bands joined by a jog is what makes the shape read as a bolt at
/// this size, and that kink is fiddly to get right from geometry.
const BOLT: [&str; 16] = [
    "      #####",
    "     #####",
    "    #####",
    "   #####",
    "  #####",
    " #####",
    "#####",
    "###########",
    " ##########",
    "      #####",
    "     #####",
    "    #####",
    "   #####",
    "  #####",
    " #####",
    "#####",
];

/// Stamp [`BOLT`] centred on (cx, cy).
fn bolt(fb: &mut Framebuffer, cx: isize, cy: isize) {
    let top = cy - BOLT.len() as isize / 2;
    for (row, art) in BOLT.iter().enumerate() {
        for (col, ch) in art.bytes().enumerate() {
            if ch != b' ' {
                fb.set(cx - 5 + col as isize, top + row as isize, true);
            }
        }
    }
}

// ===================== 3. active apps =====================

/// Up to five audio clients, each with a dot showing whether it is currently
/// producing sound (`true`) or merely holding a stream open (`false`).
pub fn render_active_apps(apps: &[(String, bool)]) -> Framebuffer {
    let mut fb = Framebuffer::new();
    fb.draw_text(2, 0, "PLAYING", 1);

    if apps.is_empty() {
        fb.draw_text_centered(26, "NO APPS", 2);
        return fb;
    }

    let active = apps.iter().filter(|(_, playing)| *playing).count();
    draw_text_right(&mut fb, WIDTH as isize - 2, 0, &format!("{active}/{}", apps.len()), 1);
    for x in 0..WIDTH as isize {
        fb.set(x, 9, true);
    }

    // Five rows of 10px; names start past the 12px dot gutter, which leaves
    // 114px == 19 glyphs at scale 1.
    for (i, (name, playing)) in apps.iter().take(5).enumerate() {
        let y = 13 + i as isize * 10;
        let (cx, cy) = (6isize, y + 3);
        if *playing {
            disc(&mut fb, cx, cy, 3);
        } else {
            ring(&mut fb, cx, cy, 3);
        }
        fb.draw_text(14, y, &ellipsize(ascii(name).trim(), 19), 1);
    }
    fb
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ellipsize_marks_the_cut() {
        assert_eq!(ellipsize("short", 10), "short");
        assert_eq!(ellipsize("abcdefghij", 6), "abc...");
        assert_eq!(ellipsize("abcdef", 6), "abcdef");
    }

    #[test]
    fn ascii_replaces_unrenderable_chars() {
        assert_eq!(ascii("caf\u{e9} \u{1f600}"), "caf? ?");
    }

    #[test]
    fn sky_keywords_pick_the_right_glyph() {
        assert!(Sky::from_desc("Patchy light rain with thunder") == Sky::Rain);
        assert!(Sky::from_desc("Light sleet showers") == Sky::Snow);
        assert!(Sky::from_desc("Partly cloudy") == Sky::Cloud);
        assert!(Sky::from_desc("Sunny") == Sky::Sun);
    }

    #[test]
    fn parses_a_wttr_payload() {
        let json = r#"{"current_condition":[{"FeelsLikeC":"19","humidity":"55",
            "temp_C":"21","windspeedKmph":"12",
            "weatherDesc":[{"value":"Partly cloudy"}]}]}"#;
        let w = parse_weather(json).expect("payload should decode");
        assert_eq!(w.temp_c, 21);
        assert_eq!(w.feels_c, 19);
        assert_eq!(w.humidity, 55);
        assert_eq!(w.wind_kph, 12);
        assert_eq!(w.desc, "Partly cloudy");
    }

    #[test]
    fn garbage_json_yields_no_reading() {
        assert!(parse_weather("not json").is_none());
        assert!(parse_weather("{}").is_none());
    }

    #[test]
    fn placeholder_screens_do_not_panic() {
        let _ = render_mouse_battery(None, false, "");
        let _ = render_mouse_battery(Some(200), true, "Logitech G Pro X Superlight 2");
        let _ = render_active_apps(&[]);
        let _ = render_active_apps(&[("a-very-long-application-name".into(), true)]);
    }

    #[test]
    fn rows_stay_inside_the_panel() {
        // Five rows at 10px starting at y=13 must leave the last baseline
        // (7px tall) on-screen.
        assert!(13 + 4 * 10 + 7 <= HEIGHT as isize);
    }
}
