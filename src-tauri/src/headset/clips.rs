//! Built-in, procedurally-generated OLED animations.
//!
//! These are original and generated at runtime, so the repo ships no
//! third-party media. Copyrighted clips (Bad Apple, Rickroll, DOOM footage,
//! …) are loaded from the user's own files via [`super::media`] instead.

use super::media::OledFrame;
use super::oled::{Framebuffer, HEIGHT, WIDTH};

/// Tiny deterministic PRNG (xorshift32) so clips look the same every run
/// without pulling in the `rand` crate.
struct Rng(u32);
impl Rng {
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    fn range(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next() % n }
    }
    fn frac(&mut self) -> f32 {
        (self.next() % 10_000) as f32 / 10_000.0
    }
}

/// A selectable clip: stable id, display label, and category for grouping.
pub struct ClipInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub category: &'static str,
}

pub const BUILTIN_CLIPS: &[ClipInfo] = &[
    // --- Japan ---
    ClipInfo { id: "torii",     label: "Torii Gate",    category: "Japan" },
    ClipInfo { id: "fuji",      label: "Mt. Fuji",      category: "Japan" },
    ClipInfo { id: "sakura",    label: "Sakura Petals", category: "Japan" },
    ClipInfo { id: "koi",       label: "Koi Pond",      category: "Japan" },
    ClipInfo { id: "asahi",     label: "Rising Sun",    category: "Japan" },
    ClipInfo { id: "wave",      label: "Great Wave",    category: "Japan" },
    ClipInfo { id: "kitsune",   label: "Kitsune Mask",  category: "Japan" },
    ClipInfo { id: "lantern",   label: "Lanterns",      category: "Japan" },
    ClipInfo { id: "kana",      label: "Kana Rain",     category: "Japan" },
    ClipInfo { id: "pagoda",    label: "Pagoda",        category: "Japan" },
    ClipInfo { id: "shuriken",  label: "Shuriken",      category: "Japan" },
    ClipInfo { id: "neon",      label: "Neon City",     category: "Japan" },
    // --- Effects ---
    ClipInfo { id: "doom",      label: "DOOM Fire",     category: "Effects" },
    ClipInfo { id: "matrix",    label: "Matrix Rain",   category: "Effects" },
    ClipInfo { id: "starfield", label: "Starfield",     category: "Effects" },
    ClipInfo { id: "plasma",    label: "Plasma",        category: "Effects" },
    ClipInfo { id: "tunnel",    label: "Tunnel",        category: "Effects" },
    ClipInfo { id: "metaballs", label: "Metaballs",     category: "Effects" },
    ClipInfo { id: "fireworks", label: "Fireworks",     category: "Effects" },
    ClipInfo { id: "rain",      label: "Rain",          category: "Effects" },
    ClipInfo { id: "snow",      label: "Snow",          category: "Effects" },
    // --- Demos ---
    ClipInfo { id: "logo",      label: "Bouncing SINK", category: "Demos" },
    ClipInfo { id: "cube",      label: "Wireframe Cube",category: "Demos" },
    ClipInfo { id: "life",      label: "Game of Life",  category: "Demos" },
    ClipInfo { id: "boids",     label: "Flocking",      category: "Demos" },
    ClipInfo { id: "scope",     label: "Oscilloscope",  category: "Demos" },
    ClipInfo { id: "ekg",       label: "Heartbeat",     category: "Demos" },
    ClipInfo { id: "pong",      label: "Pong",          category: "Demos" },
];

/// Generate a built-in clip by id, or `None` if unknown.
pub fn generate(name: &str) -> Option<Vec<OledFrame>> {
    match name {
        // Japan
        "torii" => Some(torii(120)),
        "fuji" => Some(fuji(140)),
        "sakura" => Some(sakura(140)),
        "koi" => Some(koi(140)),
        "asahi" => Some(asahi(120)),
        "wave" => Some(great_wave(140)),
        "kitsune" => Some(kitsune(120)),
        "lantern" => Some(lanterns(140)),
        "kana" => Some(kana_rain(130)),
        "pagoda" => Some(pagoda(120)),
        "shuriken" => Some(shuriken(96)),
        "neon" => Some(neon_city(140)),
        // Effects
        "doom" | "fire" => Some(doom_fire(90)),
        "matrix" => Some(matrix_rain(120)),
        "starfield" | "stars" => Some(starfield(120)),
        "plasma" => Some(plasma(90)),
        "tunnel" => Some(tunnel(90)),
        "metaballs" => Some(metaballs(100)),
        "fireworks" => Some(fireworks(150)),
        "rain" => Some(rain(120)),
        "snow" => Some(snow(140)),
        // Demos
        "logo" | "bounce" => Some(bouncing_logo(160)),
        "cube" => Some(cube(120)),
        "life" => Some(game_of_life(150)),
        "boids" => Some(boids(150)),
        "scope" => Some(oscilloscope(120)),
        "ekg" => Some(ekg(120)),
        "pong" => Some(pong(180)),
        _ => None,
    }
}

fn frame_from_gray(gray: &[u8]) -> OledFrame {
    let mut fb = Framebuffer::new();
    fb.blit_gray_dithered(gray);
    OledFrame { fb, delay_ms: 40 }
}

/// Bresenham line, used by the wireframe/vector clips.
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

// ===================== Japan =====================

/// A torii gate silhouetted against a rising sun with drifting clouds.
fn torii(frames: usize) -> Vec<OledFrame> {
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut fb = Framebuffer::new();
        // Sun rises over the run of the clip.
        let sun_y = 46 - (f as isize * 22 / frames as isize);
        circle(&mut fb, 64, sun_y, 16, true);
        // Punch horizontal cloud bands out of the sun.
        for band in 0..3 {
            let y = sun_y - 6 + band * 6;
            let off = ((f as isize * 2 + band * 17) % 40) - 20;
            fb.fill_rect(40 + off, y, 26, 2, false);
        }
        // Torii: two pillars, curved top rail (kasagi) and lower rail (nuki).
        fb.fill_rect(34, 20, 5, 44, true);
        fb.fill_rect(89, 20, 5, 44, true);
        fb.fill_rect(24, 16, 80, 4, true); // kasagi
        fb.fill_rect(28, 22, 72, 2, true); // shimaki
        fb.fill_rect(38, 32, 52, 4, true); // nuki
        fb.fill_rect(62, 20, 4, 14, true); // gakuzuka
        // Upturned ends of the top rail.
        line(&mut fb, 24, 16, 18, 12);
        line(&mut fb, 104, 16, 110, 12);
        out.push(OledFrame { fb, delay_ms: 50 });
    }
    out
}

/// Mount Fuji with a snow cap, sun and drifting cloud layers.
fn fuji(frames: usize) -> Vec<OledFrame> {
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut gray = vec![0u8; WIDTH * HEIGHT];
        let peak_x = 64.0f32;
        let peak_y = 18.0f32;
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let fx = x as f32;
                let fy = y as f32;
                // Mountain slope: |x - peak| * gradient below the summit.
                let slope = (fx - peak_x).abs() * 0.62 + peak_y;
                let v = if fy > slope {
                    // Snow cap near the top, darker rock lower down.
                    if fy < slope + 9.0 { 235 } else { 60 }
                } else {
                    0
                };
                gray[y * WIDTH + x] = v;
            }
        }
        // Sun.
        let sx = 100i32;
        let sy = 14i32;
        for dy in -7i32..=7 {
            for dx in -7i32..=7 {
                if dx * dx + dy * dy <= 49 {
                    let (px, py) = (sx + dx, sy + dy);
                    if px >= 0 && py >= 0 && (px as usize) < WIDTH && (py as usize) < HEIGHT {
                        gray[py as usize * WIDTH + px as usize] = 255;
                    }
                }
            }
        }
        // Cloud bands drifting across the slopes.
        for band in 0..2 {
            let y = 40 + band * 8;
            let off = ((f * 2 + band * 30) % (WIDTH + 40)) as isize - 40;
            for dx in 0..38 {
                let x = off + dx;
                if x >= 0 && (x as usize) < WIDTH {
                    for dy in 0..3 {
                        gray[(y + dy) * WIDTH + x as usize] = 0;
                    }
                }
            }
        }
        out.push(frame_from_gray(&gray));
    }
    out
}

/// Cherry blossom petals drifting down with a sideways sway.
fn sakura(frames: usize) -> Vec<OledFrame> {
    const N: usize = 26;
    let mut rng = Rng(0x5A_4B_1C_2D);
    let mut petals: Vec<(f32, f32, f32, f32)> = (0..N)
        .map(|_| {
            (
                rng.frac() * WIDTH as f32,
                rng.frac() * HEIGHT as f32,
                0.35 + rng.frac() * 0.7,      // fall speed
                rng.frac() * std::f32::consts::TAU, // sway phase
            )
        })
        .collect();
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut fb = Framebuffer::new();
        for p in &mut petals {
            p.1 += p.2;
            if p.1 > HEIGHT as f32 {
                p.1 = -2.0;
                p.0 = rng.frac() * WIDTH as f32;
            }
            let sway = ((f as f32 * 0.08) + p.3).sin() * 5.0;
            let x = (p.0 + sway) as isize;
            let y = p.1 as isize;
            // A petal: a 2px core with two soft edges.
            fb.set(x, y, true);
            fb.set(x + 1, y, true);
            fb.set(x, y + 1, true);
        }
        // A branch across the top-left corner.
        line(&mut fb, 0, 6, 34, 12);
        line(&mut fb, 12, 8, 18, 3);
        line(&mut fb, 26, 11, 32, 5);
        out.push(OledFrame { fb, delay_ms: 55 });
    }
    out
}

/// Koi circling a pond, with expanding ripple rings.
fn koi(frames: usize) -> Vec<OledFrame> {
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut fb = Framebuffer::new();
        let t = f as f32 * 0.08;
        // Ripples.
        for k in 0..3 {
            let r = ((t * 6.0 + k as f32 * 9.0) as isize % 30) + 4;
            circle(&mut fb, 64, 32, r, false);
        }
        // Two koi on opposite phases of an ellipse.
        for (i, phase) in [0.0f32, std::f32::consts::PI].iter().enumerate() {
            let a = t + phase;
            let cx = 64.0 + a.cos() * 38.0;
            let cy = 32.0 + a.sin() * 18.0;
            // Body.
            for s in 0..7 {
                let bx = cx - a.cos() * s as f32 * 1.6;
                let by = cy - a.sin() * s as f32 * 1.6;
                let w = if s < 4 { 2 } else { 1 };
                fb.fill_rect(bx as isize, by as isize, w, w, true);
            }
            // Forked tail that waves.
            let wag = (t * 4.0 + i as f32).sin() * 2.0;
            let tx = cx - a.cos() * 11.0;
            let ty = cy - a.sin() * 11.0;
            line(&mut fb, tx as isize, ty as isize,
                 (tx - a.cos() * 4.0 + wag) as isize, (ty - a.sin() * 4.0 + wag) as isize);
        }
        out.push(OledFrame { fb, delay_ms: 50 });
    }
    out
}

/// The rising-sun ray pattern, slowly rotating.
fn asahi(frames: usize) -> Vec<OledFrame> {
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut gray = vec![0u8; WIDTH * HEIGHT];
        let t = f as f32 * 0.03;
        let (cx, cy) = (64.0f32, 34.0f32);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                let ang = dy.atan2(dx) + t;
                let dist = (dx * dx + dy * dy).sqrt();
                // 16 alternating wedges, plus a solid disc in the middle.
                let wedge = ((ang / std::f32::consts::TAU * 16.0).floor() as i32).rem_euclid(2);
                let v = if dist < 13.0 {
                    255
                } else if wedge == 0 {
                    220
                } else {
                    0
                };
                gray[y * WIDTH + x] = v;
            }
        }
        out.push(frame_from_gray(&gray));
    }
    out
}

/// A stylised great wave with a curling crest and foam.
fn great_wave(frames: usize) -> Vec<OledFrame> {
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut fb = Framebuffer::new();
        let t = f as f32 * 0.12;
        // Layered swells: each is a sine the water is filled below.
        for layer in 0..3 {
            let amp = 6.0 - layer as f32 * 1.5;
            let base = 34.0 + layer as f32 * 9.0;
            let speed = t * (1.0 + layer as f32 * 0.4);
            for x in 0..WIDTH {
                let fx = x as f32 * 0.09;
                let y = base + (fx + speed).sin() * amp + (fx * 2.3 - speed).sin() * amp * 0.35;
                fb.set(x as isize, y as isize, true);
                if layer == 2 {
                    // Fill the deepest layer so the sea reads solid.
                    for yy in (y as isize)..HEIGHT as isize {
                        fb.set(x as isize, yy, true);
                    }
                }
            }
        }
        // Curling crest that travels left to right.
        let cx = ((t * 9.0) as isize % (WIDTH as isize + 40)) - 20;
        circle(&mut fb, cx, 26, 11, false);
        for k in 0..6 {
            let a = -1.2 + k as f32 * 0.35;
            let fx = cx as f32 + a.cos() * 14.0;
            let fy = 26.0 + a.sin() * 14.0;
            fb.set(fx as isize, fy as isize, true);
            fb.set(fx as isize + 1, fy as isize, true);
        }
        out.push(OledFrame { fb, delay_ms: 55 });
    }
    out
}

/// A kitsune (fox) mask — nods to the user's own branding.
fn kitsune(frames: usize) -> Vec<OledFrame> {
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut fb = Framebuffer::new();
        // Gentle bob so it isn't a static image.
        let bob = ((f as f32 * 0.10).sin() * 2.0) as isize;
        let cy = 34 + bob;
        // Face outline: a tapered muzzle formed from two slanted sides.
        for y in 0..34isize {
            // Half-width narrows toward the snout.
            let hw = 26 - (y * 17 / 34);
            fb.set(64 - hw, cy - 16 + y, true);
            fb.set(64 + hw, cy - 16 + y, true);
        }
        line(&mut fb, 38, cy - 16, 90, cy - 16); // brow line
        line(&mut fb, 55, cy + 18, 73, cy + 18); // chin
        // Ears.
        line(&mut fb, 40, cy - 16, 30, cy - 32);
        line(&mut fb, 30, cy - 32, 52, cy - 22);
        line(&mut fb, 88, cy - 16, 98, cy - 32);
        line(&mut fb, 98, cy - 32, 76, cy - 22);
        // Slanted eyes; they blink briefly on a cycle.
        let blink = (f % 45) < 3;
        if blink {
            line(&mut fb, 46, cy - 4, 58, cy - 4);
            line(&mut fb, 70, cy - 4, 82, cy - 4);
        } else {
            for (ex, dir) in [(46isize, 1isize), (82, -1)] {
                line(&mut fb, ex, cy - 2, ex + dir * 12, cy - 7);
                line(&mut fb, ex, cy - 2, ex + dir * 12, cy + 1);
                fb.fill_rect(ex + dir * 5, cy - 4, 3, 3, true);
            }
        }
        // Snout markings.
        fb.fill_rect(62, cy + 8, 5, 3, true);
        line(&mut fb, 58, cy + 13, 64, cy + 11);
        line(&mut fb, 70, cy + 13, 64, cy + 11);
        out.push(OledFrame { fb, delay_ms: 60 });
    }
    out
}

/// Hanging paper lanterns swaying on a wire.
fn lanterns(frames: usize) -> Vec<OledFrame> {
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut fb = Framebuffer::new();
        line(&mut fb, 0, 6, WIDTH as isize - 1, 6);
        for i in 0..4isize {
            let base_x = 18 + i * 31;
            // Each lantern swings on its own phase.
            let sway = ((f as f32 * 0.07) + i as f32 * 1.1).sin() * 4.0;
            let x = base_x + sway as isize;
            line(&mut fb, base_x, 6, x, 20);
            // Body: rounded rectangle with horizontal ribs.
            let (w, h) = (15isize, 22isize);
            for yy in 0..h {
                // Taper the ends so it reads as a paper lantern.
                let inset = if yy < 3 || yy >= h - 3 { 3 } else { 0 };
                fb.set(x - w / 2 + inset, 20 + yy, true);
                fb.set(x + w / 2 - inset, 20 + yy, true);
            }
            line(&mut fb, x - w / 2 + 3, 20, x + w / 2 - 3, 20);
            line(&mut fb, x - w / 2 + 3, 20 + h - 1, x + w / 2 - 3, 20 + h - 1);
            for rib in 1..4 {
                let ry = 20 + rib * 5;
                line(&mut fb, x - w / 2 + 1, ry, x + w / 2 - 1, ry);
            }
            // Tassel.
            line(&mut fb, x, 20 + h, x, 20 + h + 4);
        }
        out.push(OledFrame { fb, delay_ms: 55 });
    }
    out
}

/// Matrix-style rain, but the glyphs are blocky kana-like shapes.
fn kana_rain(frames: usize) -> Vec<OledFrame> {
    // Small 5x7 kana-ish glyph masks (column bitmaps, bit0 = top row).
    const GLYPHS: [[u8; 5]; 10] = [
        [0x04, 0x3f, 0x44, 0x44, 0x38], // ka-ish
        [0x7f, 0x08, 0x08, 0x08, 0x7f], // ki-ish
        [0x02, 0x01, 0x7f, 0x01, 0x02], // ta-ish
        [0x22, 0x14, 0x7f, 0x14, 0x22], // me-ish
        [0x41, 0x22, 0x1c, 0x22, 0x41], // no-ish
        [0x7c, 0x04, 0x04, 0x04, 0x7c], // ro-ish
        [0x08, 0x08, 0x7f, 0x08, 0x08], // shi-ish
        [0x3e, 0x2a, 0x2a, 0x2a, 0x22], // e-ish
        [0x40, 0x3e, 0x02, 0x02, 0x7e], // u-ish
        [0x1c, 0x22, 0x22, 0x22, 0x1c], // wa-ish
    ];
    let cols = WIDTH / 7;
    let mut rng = Rng(0x4B_A9_1E_37);
    let mut heads: Vec<i32> = (0..cols).map(|_| -(rng.range(HEIGHT as u32) as i32)).collect();
    let mut speed: Vec<u32> = (0..cols).map(|_| 1 + rng.range(2)).collect();
    let mut out = Vec::with_capacity(frames);
    for _ in 0..frames {
        let mut fb = Framebuffer::new();
        for (c, head) in heads.iter_mut().enumerate() {
            let x = (c * 7 + 1) as isize;
            for t in 0..5i32 {
                let gy = *head - t * 8;
                if gy < -7 || gy >= HEIGHT as i32 {
                    continue;
                }
                // Head glyph solid; trail thins with distance.
                if t > 0 && rng.range(3) == 0 {
                    continue;
                }
                let g = GLYPHS[(rng.range(GLYPHS.len() as u32)) as usize];
                for (col, bits) in g.iter().enumerate() {
                    for row in 0..7 {
                        if (bits >> row) & 1 != 0 {
                            fb.set(x + col as isize, gy as isize + row, true);
                        }
                    }
                }
            }
            *head += speed[c] as i32 * 2;
            if *head - 40 > HEIGHT as i32 {
                *head = -(rng.range(40) as i32);
                speed[c] = 1 + rng.range(2);
            }
        }
        out.push(OledFrame { fb, delay_ms: 60 });
    }
    out
}

/// A five-tiered pagoda with a drifting moon.
fn pagoda(frames: usize) -> Vec<OledFrame> {
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut fb = Framebuffer::new();
        // Moon crosses behind the tower.
        let mx = 6 + (f as isize * 2 % (WIDTH as isize + 20)) - 10;
        circle(&mut fb, mx, 12, 7, false);
        // Five tiers, widening toward the base.
        for tier in 0..5isize {
            let y = 14 + tier * 10;
            let hw = 14 + tier * 5;
            // Roof line with upturned eaves.
            line(&mut fb, 64 - hw, y, 64 + hw, y);
            line(&mut fb, 64 - hw, y, 64 - hw + 5, y - 4);
            line(&mut fb, 64 + hw, y, 64 + hw - 5, y - 4);
            // Body below the roof.
            let bw = hw - 6;
            fb.set(64 - bw, y + 1, true);
            fb.set(64 + bw, y + 1, true);
            for yy in 1..9 {
                fb.set(64 - bw, y + yy, true);
                fb.set(64 + bw, y + yy, true);
            }
        }
        // Finial.
        line(&mut fb, 64, 6, 64, 14);
        circle(&mut fb, 64, 5, 2, true);
        out.push(OledFrame { fb, delay_ms: 55 });
    }
    out
}

/// A spinning four-point shuriken.
fn shuriken(frames: usize) -> Vec<OledFrame> {
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut fb = Framebuffer::new();
        let t = f as f32 * 0.16;
        let (cx, cy) = (64.0f32, 32.0f32);
        // Four blades, each a triangle from hub to tip.
        for k in 0..4 {
            let a = t + k as f32 * std::f32::consts::FRAC_PI_2;
            let tip = (cx + a.cos() * 26.0, cy + a.sin() * 26.0);
            let l = (cx + (a - 0.55).cos() * 9.0, cy + (a - 0.55).sin() * 9.0);
            let r = (cx + (a + 0.55).cos() * 9.0, cy + (a + 0.55).sin() * 9.0);
            line(&mut fb, l.0 as isize, l.1 as isize, tip.0 as isize, tip.1 as isize);
            line(&mut fb, r.0 as isize, r.1 as isize, tip.0 as isize, tip.1 as isize);
            line(&mut fb, l.0 as isize, l.1 as isize, r.0 as isize, r.1 as isize);
        }
        circle(&mut fb, cx as isize, cy as isize, 9, false);
        circle(&mut fb, cx as isize, cy as isize, 3, false);
        out.push(OledFrame { fb, delay_ms: 40 });
    }
    out
}

/// A neon-lit skyline with flickering windows and a scanning searchlight.
fn neon_city(frames: usize) -> Vec<OledFrame> {
    let mut rng = Rng(0xC1_7B_EE_F3);
    // Fixed skyline so it doesn't jitter between frames.
    let towers: Vec<(isize, isize, isize)> = (0..11)
        .map(|i| {
            let x = i * 12;
            let h = 16 + rng.range(34) as isize;
            (x, h, 11)
        })
        .collect();
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut fb = Framebuffer::new();
        for (x, h, w) in &towers {
            let top = HEIGHT as isize - h;
            // Outline.
            line(&mut fb, *x, top, *x + w - 1, top);
            for yy in top..HEIGHT as isize {
                fb.set(*x, yy, true);
                fb.set(*x + w - 1, yy, true);
            }
            // Windows flicker.
            let mut wy = top + 3;
            while wy < HEIGHT as isize - 2 {
                for wx in (*x + 2..*x + w - 2).step_by(3) {
                    if (rng.range(100) as usize + f) % 17 < 9 {
                        fb.set(wx, wy, true);
                    }
                }
                wy += 4;
            }
        }
        // Searchlight beam sweeping the sky.
        let a = -2.2 + ((f as f32 * 0.05).sin() * 0.8);
        let (bx, by) = (20.0f32, HEIGHT as f32 - 6.0);
        line(&mut fb, bx as isize, by as isize,
             (bx + a.cos() * 90.0) as isize, (by + a.sin() * 90.0) as isize);
        // A blinking aircraft light.
        if (f / 8) % 2 == 0 {
            fb.set(100, 8, true);
            fb.set(101, 8, true);
        }
        out.push(OledFrame { fb, delay_ms: 60 });
    }
    out
}

// ===================== Effects =====================

/// The classic PSX DOOM fire: a hot bottom row propagating upward with random
/// decay and drift. Rendered as grayscale, then dithered to 1-bit.
fn doom_fire(frames: usize) -> Vec<OledFrame> {
    let mut rng = Rng(0x1234_5678);
    let mut buf = vec![0u8; WIDTH * HEIGHT];
    for x in 0..WIDTH {
        buf[(HEIGHT - 1) * WIDTH + x] = 255;
    }
    let mut out = Vec::with_capacity(frames);
    for _ in 0..frames {
        for y in 1..HEIGHT {
            for x in 0..WIDTH {
                let src = y * WIDTH + x;
                let decay = rng.range(24) as u8;
                let drift = (rng.range(3) as isize) - 1;
                let dst_x = (x as isize + drift).clamp(0, WIDTH as isize - 1) as usize;
                let dst = (y - 1) * WIDTH + dst_x;
                buf[dst] = buf[src].saturating_sub(decay);
            }
        }
        let mut gray = buf.clone();
        for v in &mut gray {
            *v = ((*v as u16 * *v as u16) / 255) as u8;
        }
        out.push(frame_from_gray(&gray));
    }
    out
}

/// Falling "digital rain" columns with a bright head and fading tail.
fn matrix_rain(frames: usize) -> Vec<OledFrame> {
    let cols = WIDTH / 4;
    let mut rng = Rng(0x9E37_79B9);
    let mut heads: Vec<i32> = (0..cols).map(|_| -(rng.range(HEIGHT as u32) as i32)).collect();
    let mut speed: Vec<u32> = (0..cols).map(|_| 1 + rng.range(2)).collect();
    let mut out = Vec::with_capacity(frames);
    for _ in 0..frames {
        let mut fb = Framebuffer::new();
        for (c, head) in heads.iter_mut().enumerate() {
            let x = (c * 4 + 1) as isize;
            let tail = 14;
            for t in 0..tail {
                let y = *head - t;
                if y < 0 || y >= HEIGHT as i32 {
                    continue;
                }
                let on = t == 0 || (t < 6 && (t + c as i32) % 2 == 0) || rng.range(6) == 0;
                if on {
                    fb.set(x, y as isize, true);
                    if t < 3 {
                        fb.set(x + 1, y as isize, true);
                    }
                }
            }
            *head += speed[c] as i32;
            if *head - tail > HEIGHT as i32 {
                *head = -(rng.range(HEIGHT as u32) as i32);
                speed[c] = 1 + rng.range(2);
            }
        }
        out.push(OledFrame { fb, delay_ms: 55 });
    }
    out
}

/// A warp-speed starfield: stars fly outward from the centre.
fn starfield(frames: usize) -> Vec<OledFrame> {
    const N: usize = 64;
    let mut rng = Rng(0xC0FF_EE01);
    let mut stars: Vec<(f32, f32, f32)> = (0..N)
        .map(|_| (rng.frac() * 2.0 - 1.0, rng.frac() * 2.0 - 1.0, rng.frac() + 0.1))
        .collect();
    let (cx, cy) = (WIDTH as f32 / 2.0, HEIGHT as f32 / 2.0);
    let mut out = Vec::with_capacity(frames);
    for _ in 0..frames {
        let mut fb = Framebuffer::new();
        for s in &mut stars {
            s.2 -= 0.02;
            if s.2 <= 0.02 {
                s.0 = rng.frac() * 2.0 - 1.0;
                s.1 = rng.frac() * 2.0 - 1.0;
                s.2 = 1.0;
            }
            let x = (cx + (s.0 / s.2) * cx) as isize;
            let y = (cy + (s.1 / s.2) * cy) as isize;
            fb.set(x, y, true);
            if s.2 < 0.4 {
                fb.set(x + 1, y, true);
                fb.set(x, y + 1, true);
            }
        }
        out.push(OledFrame { fb, delay_ms: 40 });
    }
    out
}

/// A classic sine plasma, dithered to monochrome.
fn plasma(frames: usize) -> Vec<OledFrame> {
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let t = f as f32 * 0.25;
        let mut gray = vec![0u8; WIDTH * HEIGHT];
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let (fx, fy) = (x as f32, y as f32);
                let v = (fx * 0.10 + t).sin()
                    + (fy * 0.12 - t).sin()
                    + ((fx + fy) * 0.08 + t).sin()
                    + (((fx * fx + fy * fy).sqrt()) * 0.12 - t).sin();
                gray[y * WIDTH + x] = (((v + 4.0) / 8.0) * 255.0) as u8;
            }
        }
        out.push(frame_from_gray(&gray));
    }
    out
}

/// A rotating/zooming tunnel built from polar texture coordinates.
fn tunnel(frames: usize) -> Vec<OledFrame> {
    let mut out = Vec::with_capacity(frames);
    let (cx, cy) = (WIDTH as f32 / 2.0, HEIGHT as f32 / 2.0);
    for f in 0..frames {
        let t = f as f32 * 0.06;
        let mut gray = vec![0u8; WIDTH * HEIGHT];
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                let dist = (dx * dx + dy * dy).sqrt().max(0.9);
                let ang = dy.atan2(dx);
                // Checkerboard in (depth, angle) space.
                let u = (16.0 / dist + t * 2.0).floor() as i32;
                let v = (ang / std::f32::consts::TAU * 12.0 + t).floor() as i32;
                let on = (u + v).rem_euclid(2) == 0;
                // Fade with depth so the mouth of the tunnel is dark.
                let shade = (dist * 5.0).min(255.0) as u8;
                gray[y * WIDTH + x] = if on { shade } else { 0 };
            }
        }
        out.push(frame_from_gray(&gray));
    }
    out
}

/// Blobby metaballs merging and splitting.
fn metaballs(frames: usize) -> Vec<OledFrame> {
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let t = f as f32 * 0.07;
        // Three orbiting attractors.
        let balls = [
            (64.0 + (t).cos() * 34.0, 32.0 + (t * 1.3).sin() * 18.0, 340.0),
            (64.0 + (t * 0.8 + 2.0).cos() * 28.0, 32.0 + (t * 1.1 + 1.0).sin() * 20.0, 300.0),
            (64.0 + (t * 1.4 + 4.0).cos() * 20.0, 32.0 + (t * 0.9 + 3.0).sin() * 14.0, 260.0),
        ];
        let mut gray = vec![0u8; WIDTH * HEIGHT];
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let mut sum = 0.0f32;
                for (bx, by, s) in &balls {
                    let dx = x as f32 - bx;
                    let dy = y as f32 - by;
                    sum += s / (dx * dx + dy * dy + 1.0);
                }
                gray[y * WIDTH + x] = (sum * 90.0).min(255.0) as u8;
            }
        }
        out.push(frame_from_gray(&gray));
    }
    out
}

/// Shells launching and bursting into expanding sparks.
fn fireworks(frames: usize) -> Vec<OledFrame> {
    struct Spark { x: f32, y: f32, vx: f32, vy: f32, life: i32 }
    let mut rng = Rng(0xF1_5E_00_01);
    let mut sparks: Vec<Spark> = Vec::new();
    let mut rocket: Option<(f32, f32, f32)> = None; // x, y, vy
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut fb = Framebuffer::new();
        // Launch a new shell periodically.
        if rocket.is_none() && f % 24 == 0 {
            rocket = Some((10.0 + rng.frac() * 108.0, HEIGHT as f32 - 1.0, -1.8 - rng.frac()));
        }
        if let Some((rx, ry, vy)) = rocket {
            let ny = ry + vy;
            fb.set(rx as isize, ny as isize, true);
            fb.set(rx as isize, ny as isize + 1, true);
            if ny < 12.0 + rng.frac() * 14.0 {
                // Burst.
                for k in 0..22 {
                    let a = k as f32 / 22.0 * std::f32::consts::TAU;
                    let sp = 0.7 + rng.frac() * 0.8;
                    sparks.push(Spark { x: rx, y: ny, vx: a.cos() * sp, vy: a.sin() * sp, life: 22 });
                }
                rocket = None;
            } else {
                rocket = Some((rx, ny, vy));
            }
        }
        // Advance sparks with a little gravity.
        for s in &mut sparks {
            s.x += s.vx;
            s.y += s.vy;
            s.vy += 0.05;
            s.life -= 1;
            if s.life > 0 {
                fb.set(s.x as isize, s.y as isize, true);
            }
        }
        sparks.retain(|s| s.life > 0);
        out.push(OledFrame { fb, delay_ms: 45 });
    }
    out
}

/// Slanted rain with splashes at the bottom.
fn rain(frames: usize) -> Vec<OledFrame> {
    const N: usize = 40;
    let mut rng = Rng(0x2A_1D_D0_0B);
    let mut drops: Vec<(f32, f32, f32)> = (0..N)
        .map(|_| (rng.frac() * WIDTH as f32, rng.frac() * HEIGHT as f32, 2.5 + rng.frac() * 2.5))
        .collect();
    let mut out = Vec::with_capacity(frames);
    for _ in 0..frames {
        let mut fb = Framebuffer::new();
        for d in &mut drops {
            d.1 += d.2;
            d.0 += 0.7; // wind shear
            if d.1 > HEIGHT as f32 {
                d.1 = -3.0;
                d.0 = rng.frac() * WIDTH as f32;
            }
            if d.0 > WIDTH as f32 {
                d.0 -= WIDTH as f32;
            }
            // A streak, slanted with the wind.
            line(&mut fb, d.0 as isize, d.1 as isize, d.0 as isize - 1, d.1 as isize - 3);
        }
        // Puddle line with occasional splash ticks.
        for x in (0..WIDTH as isize).step_by(2) {
            fb.set(x, HEIGHT as isize - 1, true);
        }
        out.push(OledFrame { fb, delay_ms: 40 });
    }
    out
}

/// Gently drifting snow that settles into a growing bank.
fn snow(frames: usize) -> Vec<OledFrame> {
    const N: usize = 34;
    let mut rng = Rng(0x50_1F_A1_17);
    let mut flakes: Vec<(f32, f32, f32, f32)> = (0..N)
        .map(|_| (rng.frac() * WIDTH as f32, rng.frac() * HEIGHT as f32,
                  0.3 + rng.frac() * 0.6, rng.frac() * 6.28))
        .collect();
    let mut bank = vec![0u8; WIDTH];
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut fb = Framebuffer::new();
        for fl in &mut flakes {
            fl.1 += fl.2;
            let x = (fl.0 + ((f as f32 * 0.05) + fl.3).sin() * 4.0) as isize;
            let xi = x.clamp(0, WIDTH as isize - 1) as usize;
            let floor = HEIGHT as f32 - 1.0 - bank[xi] as f32;
            if fl.1 >= floor {
                // Settle, then respawn at the top.
                if bank[xi] < 12 {
                    bank[xi] += 1;
                }
                fl.1 = -2.0;
                fl.0 = rng.frac() * WIDTH as f32;
                continue;
            }
            fb.set(x, fl.1 as isize, true);
        }
        // Draw the accumulated bank.
        for (x, h) in bank.iter().enumerate() {
            for k in 0..*h as isize {
                fb.set(x as isize, HEIGHT as isize - 1 - k, true);
            }
        }
        out.push(OledFrame { fb, delay_ms: 55 });
    }
    out
}

// ===================== Demos =====================

/// A DVD-style bouncing "SINK" wordmark.
fn bouncing_logo(frames: usize) -> Vec<OledFrame> {
    let text = "SINK";
    let scale = 3;
    let tw = Framebuffer::text_width(text, scale) as isize;
    let th = 7 * scale as isize;
    let (mut x, mut y) = (4isize, 6isize);
    let (mut dx, mut dy) = (2isize, 2isize);
    let mut out = Vec::with_capacity(frames);
    for _ in 0..frames {
        let mut fb = Framebuffer::new();
        fb.draw_text(x, y, text, scale);
        out.push(OledFrame { fb, delay_ms: 40 });
        x += dx;
        y += dy;
        if x <= 0 || x + tw >= WIDTH as isize {
            dx = -dx;
        }
        if y <= 0 || y + th >= HEIGHT as isize {
            dy = -dy;
        }
    }
    out
}

/// A rotating wireframe cube (classic demo-scene filler).
fn cube(frames: usize) -> Vec<OledFrame> {
    const V: [[f32; 3]; 8] = [
        [-1.0, -1.0, -1.0], [1.0, -1.0, -1.0], [1.0, 1.0, -1.0], [-1.0, 1.0, -1.0],
        [-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [1.0, 1.0, 1.0], [-1.0, 1.0, 1.0],
    ];
    const E: [(usize, usize); 12] = [
        (0, 1), (1, 2), (2, 3), (3, 0),
        (4, 5), (5, 6), (6, 7), (7, 4),
        (0, 4), (1, 5), (2, 6), (3, 7),
    ];
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut fb = Framebuffer::new();
        let (a, b) = (f as f32 * 0.05, f as f32 * 0.035);
        let proj: Vec<(isize, isize)> = V
            .iter()
            .map(|v| {
                // Rotate around Y then X, then project.
                let (x, y, z) = (v[0], v[1], v[2]);
                let (x, z) = (x * a.cos() - z * a.sin(), x * a.sin() + z * a.cos());
                let (y, z) = (y * b.cos() - z * b.sin(), y * b.sin() + z * b.cos());
                let d = 3.2 / (z + 4.0);
                ((64.0 + x * d * 44.0) as isize, (32.0 + y * d * 44.0) as isize)
            })
            .collect();
        for (i, j) in E {
            line(&mut fb, proj[i].0, proj[i].1, proj[j].0, proj[j].1);
        }
        out.push(OledFrame { fb, delay_ms: 40 });
    }
    out
}

/// Conway's Game of Life on a coarse grid, seeded randomly.
fn game_of_life(frames: usize) -> Vec<OledFrame> {
    const CELL: usize = 2;
    let (gw, gh) = (WIDTH / CELL, HEIGHT / CELL);
    let mut rng = Rng(0x11_FE_60_0D);
    let mut grid: Vec<bool> = (0..gw * gh).map(|_| rng.range(100) < 34).collect();
    let mut out = Vec::with_capacity(frames);
    for _ in 0..frames {
        let mut fb = Framebuffer::new();
        for y in 0..gh {
            for x in 0..gw {
                if grid[y * gw + x] {
                    fb.fill_rect((x * CELL) as isize, (y * CELL) as isize, CELL, CELL, true);
                }
            }
        }
        out.push(OledFrame { fb, delay_ms: 90 });
        // Step, with a toroidal wrap so gliders re-enter.
        let mut next = grid.clone();
        for y in 0..gh {
            for x in 0..gw {
                let mut n = 0;
                for dy in [gh - 1, 0, 1] {
                    for dx in [gw - 1, 0, 1] {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        if grid[((y + dy) % gh) * gw + (x + dx) % gw] {
                            n += 1;
                        }
                    }
                }
                let alive = grid[y * gw + x];
                next[y * gw + x] = matches!((alive, n), (true, 2) | (true, 3) | (false, 3));
            }
        }
        grid = next;
    }
    out
}

/// Boids-style flocking: separation, alignment and cohesion.
fn boids(frames: usize) -> Vec<OledFrame> {
    const N: usize = 22;
    let mut rng = Rng(0xB0_1D_5A_11);
    let mut b: Vec<(f32, f32, f32, f32)> = (0..N)
        .map(|_| (rng.frac() * WIDTH as f32, rng.frac() * HEIGHT as f32,
                  rng.frac() * 2.0 - 1.0, rng.frac() * 2.0 - 1.0))
        .collect();
    let mut out = Vec::with_capacity(frames);
    for _ in 0..frames {
        let snapshot = b.clone();
        for i in 0..N {
            let (mut cx, mut cy, mut ax, mut ay, mut sx, mut sy) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
            let mut count = 0.0f32;
            for (j, o) in snapshot.iter().enumerate() {
                if i == j {
                    continue;
                }
                let d = ((o.0 - snapshot[i].0).powi(2) + (o.1 - snapshot[i].1).powi(2)).sqrt();
                if d < 26.0 {
                    cx += o.0; cy += o.1;
                    ax += o.2; ay += o.3;
                    count += 1.0;
                    if d < 8.0 && d > 0.01 {
                        sx += (snapshot[i].0 - o.0) / d;
                        sy += (snapshot[i].1 - o.1) / d;
                    }
                }
            }
            if count > 0.0 {
                // Cohesion toward the local centre, alignment to local heading.
                b[i].2 += ((cx / count) - snapshot[i].0) * 0.0009 + ((ax / count) - snapshot[i].2) * 0.05;
                b[i].3 += ((cy / count) - snapshot[i].1) * 0.0009 + ((ay / count) - snapshot[i].3) * 0.05;
            }
            b[i].2 += sx * 0.04;
            b[i].3 += sy * 0.04;
            // Clamp speed so the flock stays coherent.
            let sp = (b[i].2 * b[i].2 + b[i].3 * b[i].3).sqrt().max(0.01);
            b[i].2 = b[i].2 / sp * 1.2;
            b[i].3 = b[i].3 / sp * 1.2;
            b[i].0 = (b[i].0 + b[i].2).rem_euclid(WIDTH as f32);
            b[i].1 = (b[i].1 + b[i].3).rem_euclid(HEIGHT as f32);
        }
        let mut fb = Framebuffer::new();
        for (x, y, vx, vy) in &b {
            // Draw each boid as a short line along its heading.
            line(&mut fb, *x as isize, *y as isize,
                 (*x - vx * 2.5) as isize, (*y - vy * 2.5) as isize);
        }
        out.push(OledFrame { fb, delay_ms: 45 });
    }
    out
}

/// A two-trace oscilloscope with a grid.
fn oscilloscope(frames: usize) -> Vec<OledFrame> {
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut fb = Framebuffer::new();
        // Graticule.
        for x in (0..WIDTH as isize).step_by(16) {
            for y in (0..HEIGHT as isize).step_by(4) {
                fb.set(x, y, true);
            }
        }
        for y in (0..HEIGHT as isize).step_by(16) {
            for x in (0..WIDTH as isize).step_by(4) {
                fb.set(x, y, true);
            }
        }
        let t = f as f32 * 0.16;
        let mut prev = (0isize, 0isize, 0isize);
        for x in 0..WIDTH as isize {
            let p = x as f32 * 0.11;
            let y1 = 20.0 + (p + t).sin() * 12.0;
            let y2 = 46.0 + (p * 2.0 - t * 1.4).sin() * 9.0 * (p * 0.3).cos();
            if x > 0 {
                line(&mut fb, x - 1, prev.1, x, y1 as isize);
                line(&mut fb, x - 1, prev.2, x, y2 as isize);
            }
            prev = (x, y1 as isize, y2 as isize);
        }
        out.push(OledFrame { fb, delay_ms: 40 });
    }
    out
}

/// A scrolling ECG trace with a BPM read-out.
fn ekg(frames: usize) -> Vec<OledFrame> {
    let mut trace: Vec<isize> = vec![44; WIDTH];
    let mut out = Vec::with_capacity(frames);
    let mut beat = 0usize;
    for f in 0..frames {
        // Shift left and append the next sample of the PQRST complex.
        trace.remove(0);
        let phase = beat % 30;
        let v: isize = match phase {
            2 => 40,           // P
            6 => 48,           // Q
            7 => 16,           // R (spike)
            8 => 54,           // S
            12 => 38,          // T
            13 => 41,
            _ => 44,
        };
        trace.push(v);
        beat += 1;

        let mut fb = Framebuffer::new();
        for x in 1..WIDTH {
            line(&mut fb, x as isize - 1, trace[x - 1], x as isize, trace[x]);
        }
        fb.draw_text(2, 2, "HEART", 1);
        // Roughly 30 samples per beat at ~22 fps.
        let bpm = 72 + ((f as f32 * 0.05).sin() * 6.0) as i32;
        let label = format!("{bpm} BPM");
        let w = Framebuffer::text_width(&label, 1) as isize;
        fb.draw_text(WIDTH as isize - w - 2, 2, &label, 1);
        out.push(OledFrame { fb, delay_ms: 45 });
    }
    out
}

/// Pong that plays itself, complete with score.
fn pong(frames: usize) -> Vec<OledFrame> {
    let (mut bx, mut by) = (64.0f32, 32.0f32);
    let (mut vx, mut vy) = (1.9f32, 1.1f32);
    let (mut p1, mut p2) = (26.0f32, 26.0f32);
    let (mut s1, mut s2) = (0u32, 0u32);
    let ph = 14.0f32;
    let mut out = Vec::with_capacity(frames);
    for _ in 0..frames {
        bx += vx;
        by += vy;
        if by <= 1.0 || by >= HEIGHT as f32 - 2.0 {
            vy = -vy;
        }
        // Paddles track the ball with a lag so rallies look natural.
        p1 += ((by - ph / 2.0) - p1).clamp(-1.7, 1.7);
        p2 += ((by - ph / 2.0) - p2).clamp(-1.5, 1.5);
        p1 = p1.clamp(0.0, HEIGHT as f32 - ph);
        p2 = p2.clamp(0.0, HEIGHT as f32 - ph);
        if bx <= 6.0 && by >= p1 && by <= p1 + ph {
            vx = vx.abs();
        }
        if bx >= WIDTH as f32 - 7.0 && by >= p2 && by <= p2 + ph {
            vx = -vx.abs();
        }
        if bx < 0.0 {
            s2 += 1;
            bx = 64.0;
            by = 32.0;
            vx = 1.9;
        }
        if bx > WIDTH as f32 {
            s1 += 1;
            bx = 64.0;
            by = 32.0;
            vx = -1.9;
        }

        let mut fb = Framebuffer::new();
        // Net.
        for y in (0..HEIGHT as isize).step_by(6) {
            fb.set(64, y, true);
            fb.set(64, y + 1, true);
        }
        fb.fill_rect(3, p1 as isize, 3, ph as usize, true);
        fb.fill_rect(WIDTH as isize - 6, p2 as isize, 3, ph as usize, true);
        fb.fill_rect(bx as isize, by as isize, 3, 3, true);
        fb.draw_text(46, 2, &s1.to_string(), 2);
        fb.draw_text(74, 2, &s2.to_string(), 2);
        out.push(OledFrame { fb, delay_ms: 35 });
    }
    out
}
