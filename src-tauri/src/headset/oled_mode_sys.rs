//! History-based monitoring screens for the OLED: scrolling CPU/GPU graphs,
//! network throughput, uptime/load and temperatures.
//!
//! The dashboard in [`super::oled_controller`] only ever shows *now*, so it can
//! re-read its numbers whenever it likes. These screens need a time series
//! instead, which is why [`SysMonitor`] owns its own sampler: it advances a set
//! of ring buffers at most once a second (the expensive part — `nvidia-smi` —
//! must not run per frame) while the render methods stay cheap enough for the
//! ~24 fps OLED loop.
//!
//! Data sources come from [`super::sysinfo`]; only what it does not provide
//! (network byte counters, uptime, load average, core count) is read here.

use std::fs;
use std::time::{Duration, Instant};

use super::oled::{Framebuffer, HEIGHT, WIDTH};
use super::sysinfo::{self, CpuSampler};

/// Samples kept per series — one per second, so ~2 minutes of history, and
/// conveniently one per horizontal pixel of the panel.
const HIST: usize = 128;
/// Minimum spacing between samples. Renders in between reuse the last values.
const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);
/// Sentinel for "this source had nothing to report at that sample".
const NO_DATA: u8 = u8::MAX;
/// Fixed vertical range of the temperature graph, in °C.
const TEMP_LO: i32 = 20;
const TEMP_HI: i32 = 100;

/// A fixed-capacity FIFO of samples. No allocation, no growth, oldest values
/// silently fall off the back.
#[derive(Clone, Copy)]
struct Ring<T: Copy> {
    data: [T; HIST],
    head: usize,
    len: usize,
}

impl<T: Copy + Default> Ring<T> {
    fn new() -> Self {
        Self {
            data: [T::default(); HIST],
            head: 0,
            len: 0,
        }
    }

    fn push(&mut self, v: T) {
        self.data[self.head] = v;
        self.head = (self.head + 1) % HIST;
        if self.len < HIST {
            self.len += 1;
        }
    }

    fn last(&self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            Some(self.data[(self.head + HIST - 1) % HIST])
        }
    }

    /// The newest `n` samples, oldest first (fewer if the ring is still cold).
    fn recent(&self, n: usize) -> Vec<T> {
        let n = n.min(self.len);
        let start = (self.head + HIST - n) % HIST;
        (0..n).map(|i| self.data[(start + i) % HIST]).collect()
    }
}

/// Sampler + renderer for the monitoring screens.
pub struct SysMonitor {
    cpu_sampler: CpuSampler,
    last_sample: Option<Instant>,
    /// Previous `/proc/net/dev` totals: (rx bytes, tx bytes, when read).
    net_prev: Option<(u64, u64, Instant)>,
    cpu: Ring<u8>,
    gpu: Ring<u8>,
    cpu_temp: Ring<u8>,
    gpu_temp: Ring<u8>,
    rx: Ring<u32>,
    tx: Ring<u32>,
    uptime: Option<u64>,
    load: Option<[f32; 3]>,
    /// Core count never changes at runtime, so it is read exactly once.
    cores: usize,
}

impl Default for SysMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl SysMonitor {
    pub fn new() -> Self {
        let mut cpu_sampler = CpuSampler::default();
        // Prime both delta-based sources so the first real sample, one second
        // from now, is a true rate instead of a garbage spike.
        cpu_sampler.sample();
        let now = Instant::now();
        Self {
            cpu_sampler,
            last_sample: Some(now),
            net_prev: net_totals().map(|(rx, tx)| (rx, tx, now)),
            cpu: Ring::new(),
            gpu: Ring::new(),
            cpu_temp: Ring::new(),
            gpu_temp: Ring::new(),
            rx: Ring::new(),
            tx: Ring::new(),
            uptime: uptime_secs(),
            load: loadavg(),
            cores: cpu_cores(),
        }
    }

    /// Advance every series, but never more than once per [`SAMPLE_INTERVAL`].
    fn sample(&mut self) {
        let now = Instant::now();
        if let Some(prev) = self.last_sample {
            if now.duration_since(prev) < SAMPLE_INTERVAL {
                return;
            }
        }
        self.last_sample = Some(now);

        self.cpu.push(self.cpu_sampler.sample().min(100));
        self.cpu_temp.push(sysinfo::cpu_temp().unwrap_or(NO_DATA));

        let g = sysinfo::gpu();
        self.gpu
            .push(g.as_ref().map(|g| g.util.min(100)).unwrap_or(NO_DATA));
        self.gpu_temp
            .push(g.as_ref().and_then(|g| g.temp).unwrap_or(NO_DATA));

        let (mut rx_rate, mut tx_rate) = (0u32, 0u32);
        if let Some((rx, tx)) = net_totals() {
            if let Some((prx, ptx, pt)) = self.net_prev {
                let dt = now.duration_since(pt).as_secs_f64();
                // Guard against a division blow-up if two reads land together.
                if dt > 0.05 {
                    let rate = |d: u64| (d as f64 / dt).min(u32::MAX as f64) as u32;
                    // Counters wrap/reset on interface churn; saturating_sub
                    // turns that into a harmless zero instead of a huge spike.
                    rx_rate = rate(rx.saturating_sub(prx));
                    tx_rate = rate(tx.saturating_sub(ptx));
                }
            }
            self.net_prev = Some((rx, tx, now));
        }
        self.rx.push(rx_rate);
        self.tx.push(tx_rate);

        self.uptime = uptime_secs();
        self.load = loadavg();
    }

    /// Stacked CPU and GPU utilisation graphs, newest sample at the right.
    pub fn render_graphs(&mut self) -> Framebuffer {
        self.sample();
        let mut fb = Framebuffer::new();
        graph_panel(
            &mut fb,
            0,
            "CPU",
            &pct_label(self.cpu.last().and_then(opt)),
            &series(&self.cpu, PLOT_W),
        );
        graph_panel(
            &mut fb,
            HEIGHT as isize / 2,
            "GPU",
            &pct_label(self.gpu.last().and_then(opt)),
            &series(&self.gpu, PLOT_W),
        );
        fb
    }

    /// Download/upload rates plus a shared-scale sparkline of both.
    pub fn render_network(&mut self) -> Framebuffer {
        self.sample();
        let mut fb = Framebuffer::new();
        let rx = self.rx.recent(PLOT_W);
        let tx = self.tx.recent(PLOT_W);
        // Both series share one scale so the sparkline compares them honestly.
        let peak = rx.iter().chain(tx.iter()).copied().max().unwrap_or(0).max(1);

        fb.draw_text(1, 0, "NETWORK", 1);
        let pk = format!("PK {}", human_rate(peak as f64));
        let w = Framebuffer::text_width(&pk, 1) as isize;
        fb.draw_text(WIDTH as isize - w - 1, 0, &pk, 1);

        for (i, (label, v)) in [("DN", self.rx.last()), ("UP", self.tx.last())]
            .iter()
            .enumerate()
        {
            let y = 9 + i as isize * 15;
            fb.draw_text(1, y + 4, label, 1);
            let text = match v {
                Some(v) => human_rate(*v as f64),
                None => "--".to_string(),
            };
            let w = Framebuffer::text_width(&text, 2) as isize;
            fb.draw_text((WIDTH as isize - w - 2).max(14), y, &text, 2);
        }

        // Sparkline: download as filled bars, upload as a dotted overlay.
        let (bx, by, bw, bh) = (0isize, 40isize, WIDTH, 24usize);
        fb.stroke_rect(bx, by, bw, bh);
        let (ix, iy, iw, ih) = (bx + 1, by + 1, bw - 2, bh - 2);
        let x0 = ix + (iw - rx.len()) as isize;
        for (i, v) in rx.iter().enumerate() {
            let h = (*v as u64 * ih as u64 / peak as u64) as isize;
            if h > 0 {
                fb.fill_rect(x0 + i as isize, iy + ih as isize - h, 1, h as usize, true);
            }
        }
        let pts: Vec<Option<isize>> = tx
            .iter()
            .map(|v| Some(map_range(*v as u64, peak as u64, iy, ih)))
            .collect();
        trace(&mut fb, ix, iw, &pts, true);
        fb
    }

    /// Uptime, the three load averages, and 1-minute load against core count.
    pub fn render_uptime(&mut self) -> Framebuffer {
        self.sample();
        let mut fb = Framebuffer::new();
        fb.draw_text(1, 0, "UPTIME", 1);

        let up = match self.uptime {
            Some(s) => format_uptime(s),
            None => "--:--".to_string(),
        };
        fb.draw_text_centered(9, &up, 2);

        let load = self.load.unwrap_or([0.0; 3]);
        let has_load = self.load.is_some();
        fb.draw_text(1, 26, "LOAD", 1);
        let text = if has_load {
            format!("{:.2} {:.2} {:.2}", load[0], load[1], load[2])
        } else {
            "-- -- --".to_string()
        };
        let w = Framebuffer::text_width(&text, 1) as isize;
        fb.draw_text(WIDTH as isize - w - 1, 26, &text, 1);

        // Load bar: full width == one runnable task per core (i.e. saturated).
        let (bx, by, bw, bh) = (1isize, 36isize, WIDTH - 2, 11usize);
        fb.stroke_rect(bx, by, bw, bh);
        let inner = bw - 2;
        let ratio = (load[0] / self.cores as f32).clamp(0.0, 1.0);
        let fill = if has_load {
            (inner as f32 * ratio).round() as usize
        } else {
            0
        };
        fb.fill_rect(bx + 1, by + 1, fill.min(inner), bh - 2, true);
        // Half-load tick, drawn on top so it stays visible inside the fill.
        let mid = bx + 1 + inner as isize / 2;
        for dy in 0..bh as isize - 2 {
            fb.set(mid, by + 1 + dy, (dy % 2 == 0) != (fill > inner / 2));
        }

        let foot = if has_load {
            format!("1m {:.2} of {} CPU", load[0], self.cores)
        } else {
            "load unavailable".to_string()
        };
        fb.draw_text_centered(51, &foot, 1);
        fb
    }

    /// CPU and GPU temperature read-outs over a shared history graph.
    pub fn render_temps(&mut self) -> Framebuffer {
        self.sample();
        let mut fb = Framebuffer::new();
        fb.draw_text(1, 0, "CPU", 1);
        let gpu_hdr = "GPU";
        let w = Framebuffer::text_width(gpu_hdr, 1) as isize;
        fb.draw_text(WIDTH as isize - w - 1, 0, gpu_hdr, 1);

        let cpu_t = self.cpu_temp.last().and_then(opt);
        let gpu_t = self.gpu_temp.last().and_then(opt);
        fb.draw_text(1, 8, &temp_label(cpu_t), 2);
        let right = temp_label(gpu_t);
        let w = Framebuffer::text_width(&right, 2) as isize;
        fb.draw_text(WIDTH as isize - w - 1, 8, &right, 2);

        // Fixed 20..100 °C scale: an autoscaled axis would make idle jitter
        // look like a thermal event.
        let (gx, gy, gw, gh) = (0isize, 24isize, WIDTH, 40usize);
        fb.stroke_rect(gx, gy, gw, gh);
        let (ix, iy, iw, ih) = (gx + 1, gy + 1, gw - 2, gh - 2);
        for deg in [40, 60, 80] {
            let y = temp_y(deg, iy, ih);
            for x in (ix..ix + iw as isize).step_by(4) {
                fb.set(x, y, true);
            }
        }
        let to_pts = |r: &Ring<u8>| -> Vec<Option<isize>> {
            series(r, iw)
                .iter()
                .map(|v| v.map(|v| temp_y(v as i32, iy, ih)))
                .collect()
        };
        trace(&mut fb, ix, iw, &to_pts(&self.cpu_temp), false);
        trace(&mut fb, ix, iw, &to_pts(&self.gpu_temp), true);
        fb
    }
}

// ===================== drawing helpers =====================

/// Usable plot width: the panel minus its 1px frame on each side.
const PLOT_W: usize = WIDTH - 2;

/// One monitoring panel: a title row (`LABEL … value`) over a framed plot.
fn graph_panel(fb: &mut Framebuffer, top: isize, label: &str, value: &str, samples: &[Option<u8>]) {
    fb.draw_text(1, top, label, 1);
    let w = Framebuffer::text_width(value, 1) as isize;
    fb.draw_text(WIDTH as isize - w - 1, top, value, 1);

    let (gx, gy, gw, gh) = (0isize, top + 8, WIDTH, 24usize);
    fb.stroke_rect(gx, gy, gw, gh);
    let (ix, iy, iw, ih) = (gx + 1, gy + 1, gw - 2, gh - 2);
    // 50% reference line, dotted so it never reads as data.
    let mid = map_range(50, 100, iy, ih);
    for x in (ix..ix + iw as isize).step_by(4) {
        fb.set(x, mid, true);
    }
    let pts: Vec<Option<isize>> = samples
        .iter()
        .map(|v| v.map(|v| map_range(v as u64, 100, iy, ih)))
        .collect();
    trace(fb, ix, iw, &pts, false);
}

/// Map `v` in `0..=max` to a pixel row inside a plot of height `h` at `y`
/// (0 at the bottom row, `max` at the top).
fn map_range(v: u64, max: u64, y: isize, h: usize) -> isize {
    let max = max.max(1);
    let span = (h as isize - 1).max(0);
    y + span - (v.min(max) as isize * span / max as isize)
}

/// Pixel row for a temperature on the fixed [`TEMP_LO`]..[`TEMP_HI`] axis.
fn temp_y(t: i32, y: isize, h: usize) -> isize {
    let clamped = t.clamp(TEMP_LO, TEMP_HI) - TEMP_LO;
    map_range(clamped as u64, (TEMP_HI - TEMP_LO) as u64, y, h)
}

/// Connect a series of pixel rows, newest sample flush to the right edge.
/// `None` entries break the line so gaps in the data stay visible as gaps.
fn trace(fb: &mut Framebuffer, x0: isize, w: usize, pts: &[Option<isize>], dashed: bool) {
    let n = pts.len().min(w);
    let pts = &pts[pts.len() - n..];
    let start = x0 + (w - n) as isize;
    let mut prev: Option<(isize, isize)> = None;
    for (i, p) in pts.iter().enumerate() {
        let x = start + i as isize;
        match p {
            Some(y) => {
                match prev {
                    Some((px, py)) => line(fb, px, py, x, *y, dashed),
                    // A lone sample still deserves a pixel.
                    None => plot(fb, x, *y, dashed),
                }
                prev = Some((x, *y));
            }
            None => prev = None,
        }
    }
}

/// Set one pixel; dashed traces use a checkerboard so they stay legible
/// against solid traces and filled bars at any slope.
#[inline]
fn plot(fb: &mut Framebuffer, x: isize, y: isize, dashed: bool) {
    if !dashed || (x + y) % 2 == 0 {
        fb.set(x, y, true);
    }
}

/// Bresenham line (clips.rs has its own private copy; this one dashes).
fn line(fb: &mut Framebuffer, x0: isize, y0: isize, x1: isize, y1: isize, dashed: bool) {
    let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
    let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
    let (mut x, mut y, mut err) = (x0, y0, dx + dy);
    loop {
        plot(fb, x, y, dashed);
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

/// The newest `n` samples with the [`NO_DATA`] sentinel turned back into `None`.
fn series(r: &Ring<u8>, n: usize) -> Vec<Option<u8>> {
    r.recent(n).into_iter().map(opt).collect()
}

fn opt(v: u8) -> Option<u8> {
    (v != NO_DATA).then_some(v)
}

fn pct_label(v: Option<u8>) -> String {
    match v {
        Some(v) => format!("{v}%"),
        None => "--".to_string(),
    }
}

/// 0x7f renders as a degree-ish glyph in the GLCD font (same trick as the
/// System dashboard).
fn temp_label(v: Option<u8>) -> String {
    match v {
        Some(v) => format!("{v}\u{7f}C"),
        None => "--".to_string(),
    }
}

/// Byte rate in the largest unit that keeps the string short: at most one
/// decimal, and never more than 8 characters so it fits at scale 2.
fn human_rate(bps: f64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * KIB;
    let (v, unit) = if bps >= MIB {
        (bps / MIB, "MB/s")
    } else if bps >= KIB {
        (bps / KIB, "KB/s")
    } else {
        (bps, "B/s")
    };
    if v < 10.0 && unit != "B/s" {
        format!("{v:.1} {unit}")
    } else {
        format!("{v:.0} {unit}")
    }
}

/// `3d 04:21`, dropping the day part while it would read `0d`.
fn format_uptime(secs: u64) -> String {
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3600;
    let mins = (secs % 3600) / 60;
    if days > 0 {
        format!("{days}d {hours:02}:{mins:02}")
    } else {
        format!("{hours:02}:{mins:02}")
    }
}

// ===================== /proc readers =====================

/// True for interfaces whose traffic is not real link traffic: loopback and
/// the usual container/VM bridge and veth clutter, which would otherwise be
/// counted twice alongside the physical NIC.
fn is_virtual_iface(name: &str) -> bool {
    name == "lo"
        || name.starts_with("veth")
        || name.starts_with("docker")
        || name.starts_with("br-")
        || name.starts_with("virbr")
}

/// Summed (rx, tx) byte counters across all physical interfaces.
fn net_totals() -> Option<(u64, u64)> {
    let data = fs::read_to_string("/proc/net/dev").ok()?;
    let (mut rx, mut tx) = (0u64, 0u64);
    for line in data.lines() {
        // "  eth0: <8 receive fields> <8 transmit fields>"; the two header
        // rows have no colon and drop out here.
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || is_virtual_iface(name) {
            continue;
        }
        let f: Vec<u64> = rest
            .split_whitespace()
            .map(|v| v.parse().unwrap_or(0))
            .collect();
        if f.len() < 9 {
            continue;
        }
        rx += f[0];
        tx += f[8];
    }
    Some((rx, tx))
}

fn uptime_secs() -> Option<u64> {
    let data = fs::read_to_string("/proc/uptime").ok()?;
    let secs: f64 = data.split_whitespace().next()?.parse().ok()?;
    (secs >= 0.0).then_some(secs as u64)
}

fn loadavg() -> Option<[f32; 3]> {
    let data = fs::read_to_string("/proc/loadavg").ok()?;
    let mut it = data.split_whitespace();
    let mut out = [0.0f32; 3];
    for slot in &mut out {
        *slot = it.next()?.parse().ok()?;
    }
    Some(out)
}

/// Logical core count, from the `processor` lines of `/proc/cpuinfo`. Never
/// zero — it is used as a divisor for the load bar.
fn cpu_cores() -> usize {
    fs::read_to_string("/proc/cpuinfo")
        .map(|s| s.lines().filter(|l| l.starts_with("processor")).count())
        .unwrap_or(0)
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_keeps_the_newest_samples_in_order() {
        let mut r: Ring<u8> = Ring::new();
        for i in 0..(HIST as u32 + 5) {
            r.push(i as u8);
        }
        let recent = r.recent(3);
        assert_eq!(recent, vec![(HIST + 2) as u8, (HIST + 3) as u8, (HIST + 4) as u8]);
        assert_eq!(r.last(), Some((HIST + 4) as u8));
    }

    #[test]
    fn rates_use_short_human_units() {
        assert_eq!(human_rate(512.0), "512 B/s");
        assert_eq!(human_rate(2048.0), "2.0 KB/s");
        assert_eq!(human_rate(1024.0 * 1024.0 * 3.5), "3.5 MB/s");
        assert!(human_rate(999.0 * 1024.0).len() <= 8);
    }

    #[test]
    fn uptime_drops_a_zero_day_count() {
        assert_eq!(format_uptime(3 * 86_400 + 4 * 3600 + 21 * 60), "3d 04:21");
        assert_eq!(format_uptime(4 * 3600 + 21 * 60), "04:21");
    }

    #[test]
    fn plot_mapping_hits_both_rails() {
        // 0 sits on the bottom row, the maximum on the top row.
        assert_eq!(map_range(0, 100, 10, 22), 31);
        assert_eq!(map_range(100, 100, 10, 22), 10);
        assert_eq!(temp_y(TEMP_LO - 50, 10, 22), 31);
        assert_eq!(temp_y(TEMP_HI + 50, 10, 22), 10);
    }

    #[test]
    fn virtual_interfaces_are_excluded() {
        assert!(is_virtual_iface("lo"));
        assert!(is_virtual_iface("docker0"));
        assert!(is_virtual_iface("br-1a2b3c"));
        assert!(!is_virtual_iface("enp5s0"));
        assert!(!is_virtual_iface("wlan0"));
    }

    #[test]
    fn renders_without_panicking_on_a_cold_monitor() {
        let mut m = SysMonitor::new();
        // No samples yet: every screen must still produce a frame.
        for fb in [
            m.render_graphs(),
            m.render_network(),
            m.render_uptime(),
            m.render_temps(),
        ] {
            assert_eq!(fb.frame_packets().len(), 2);
        }
    }
}
