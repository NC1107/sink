//! Real-time audio spectrum analyser for the OLED.
//!
//! Audio comes from `parec` (PulseAudio / pipewire-pulse), which is always
//! present wherever Sink runs, recording the monitor of the default sink so we
//! visualise what the user actually hears. The subprocess is spawned once on a
//! background thread; that thread does all the expensive work (FFT, band
//! folding, smoothing) and publishes a tiny array of band levels behind a
//! mutex. [`Spectrum::render`] only ever reads that array, so the 24 fps OLED
//! draw loop never blocks on IO.
//!
//! The FFT is a hand-rolled in-place radix-2 Cooley–Tukey — Sink ships no
//! third-party DSP crate and 1024 points at ~23 blocks/s is nothing.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use super::oled::{Framebuffer, HEIGHT, WIDTH};

/// FFT window length in samples. Power of two (radix-2) and ~23 ms at 44.1 kHz,
/// which is short enough to feel instant and long enough for ~43 Hz bins.
const FFT_SIZE: usize = 1024;
const SAMPLE_RATE: f32 = 44_100.0;
/// Number of logarithmically-spaced bars.
const BANDS: usize = 24;
/// Lowest / highest frequency the display covers.
const F_LO: f32 = 45.0;
const F_HI: f32 = 16_000.0;
/// Noise floor of the dB mapping: anything quieter than this reads as silence.
const FLOOR_DB: f32 = 58.0;

// --- Layout -----------------------------------------------------------------

const BAR_W: usize = 4;
const BAR_GAP: isize = 1;
const BAR_PITCH: isize = BAR_W as isize + BAR_GAP;
/// Baseline row; bars grow upward from just above it.
const FLOOR_Y: isize = HEIGHT as isize - 1;
/// Tallest a bar may be, leaving a few rows at the top for the peak tick.
const MAX_BAR: isize = 54;

// ============================ capture state =================================

/// What the capture thread publishes for the renderer.
struct Bands {
    level: [f32; BANDS],
    peak: [f32; BANDS],
    /// False until the first block has actually been analysed.
    valid: bool,
}

impl Bands {
    fn new() -> Self {
        Self {
            level: [0.0; BANDS],
            peak: [0.0; BANDS],
            valid: false,
        }
    }

    fn reset(&mut self) {
        self.level = [0.0; BANDS];
        self.peak = [0.0; BANDS];
        self.valid = false;
    }
}

/// Live audio spectrum analyser. Cheap to construct; capture starts on
/// [`Spectrum::start`] and is torn down by [`Spectrum::stop`].
pub struct Spectrum {
    bands: Arc<Mutex<Bands>>,
    running: Arc<AtomicBool>,
    /// The `parec` process, tagged with the generation that spawned it so a
    /// restart never has an old thread reaping the new child — or clearing
    /// the new generation's `running` flag on its way out.
    child: Arc<Mutex<Option<(u64, Child)>>>,
    generation: Arc<AtomicU64>,
}

impl Default for Spectrum {
    fn default() -> Self {
        Self::new()
    }
}

impl Spectrum {
    pub fn new() -> Self {
        Self {
            bands: Arc::new(Mutex::new(Bands::new())),
            running: Arc::new(AtomicBool::new(false)),
            child: Arc::new(Mutex::new(None)),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Start capturing. Idempotent: the atomic swap means only the first caller
    /// of a stopped analyser spawns a thread.
    pub fn start(&self) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Ok(mut b) = self.bands.lock() {
            b.reset();
        }
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let bands = Arc::clone(&self.bands);
        let running = Arc::clone(&self.running);
        let child = Arc::clone(&self.child);
        let gen_flag = Arc::clone(&self.generation);
        // A detached thread: it owns the pipe and exits when `running` clears
        // or when the pipe dies (which is what killing the child does).
        let spawned = thread::Builder::new()
            .name("sink-spectrum".into())
            .spawn(move || capture_loop(generation, bands, running, child, gen_flag));
        if let Err(e) = spawned {
            eprintln!("sink: could not spawn spectrum thread: {e}");
            self.running.store(false, Ordering::SeqCst);
        }
    }

    /// Stop capturing and kill `parec`. Safe to call when already stopped.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        // Killing the child closes the pipe, which unblocks the reader thread.
        if let Ok(mut slot) = self.child.lock() {
            if let Some((_, mut c)) = slot.take() {
                let _ = c.kill();
                let _ = c.wait(); // reap it so we never leave a zombie behind
            }
        }
        if let Ok(mut b) = self.bands.lock() {
            b.reset();
        }
    }

    /// Draw the current spectrum. Pure reads — never touches IO.
    pub fn render(&self) -> Framebuffer {
        let mut fb = Framebuffer::new();

        let Some((level, peak)) = self.snapshot() else {
            // No capture yet (or it failed): say so instead of drawing a lie.
            fb.draw_text_centered(20, "SPECTRUM", 2);
            fb.draw_text_centered(40, "starting...", 1);
            return fb;
        };

        // Centre the bar block: 24 bars of 4px with 1px gaps span 119px.
        let span = BAR_PITCH * BANDS as isize - BAR_GAP;
        let x0 = (WIDTH as isize - span) / 2;

        fb.fill_rect(x0, FLOOR_Y, span as usize, 1, true);

        for b in 0..BANDS {
            let x = x0 + b as isize * BAR_PITCH;
            let bar_h = (level[b].clamp(0.0, 1.0) * MAX_BAR as f32).round() as isize;
            if bar_h > 0 {
                fb.fill_rect(x, FLOOR_Y - bar_h, BAR_W, bar_h as usize, true);
            } else {
                // A 1px stub keeps all 24 columns legible during silence.
                fb.fill_rect(x, FLOOR_Y - 1, BAR_W, 1, true);
            }

            // Peak tick, held at least two rows clear of the bar so it reads as
            // a separate marker even while the peak is being pushed up.
            let peak_h = (peak[b].clamp(0.0, 1.0) * MAX_BAR as f32).round() as isize;
            let tick_y = (FLOOR_Y - peak_h - 2).min(FLOOR_Y - bar_h - 2).max(0);
            fb.fill_rect(x, tick_y, BAR_W, 1, true);
        }
        fb
    }

    /// Copy the published bands out from under the lock, or `None` if capture
    /// is stopped, poisoned, or has not produced a block yet.
    fn snapshot(&self) -> Option<([f32; BANDS], [f32; BANDS])> {
        if !self.running.load(Ordering::SeqCst) {
            return None;
        }
        let b = self.bands.lock().ok()?;
        if !b.valid {
            return None;
        }
        Some((b.level, b.peak))
    }
}

impl Drop for Spectrum {
    fn drop(&mut self) {
        // Never outlive the analyser with a stray recorder attached to the sink.
        self.stop();
    }
}

// ============================= capture thread ===============================

/// Clear `running` on the way out, but only if we are still the live
/// generation. A `stop()` immediately followed by a `start()` leaves the
/// outgoing thread blocked in `read_exact` until its killed pipe reports EOF;
/// without this guard it would then switch off the capture that the new
/// generation just switched on, leaving the display stuck on "starting..."
/// with a healthy `parec` running behind it.
fn retire(generation: u64, running: &AtomicBool, current_gen: &AtomicU64) {
    if current_gen.load(Ordering::SeqCst) == generation {
        running.store(false, Ordering::SeqCst);
    }
}

fn capture_loop(
    generation: u64,
    bands: Arc<Mutex<Bands>>,
    running: Arc<AtomicBool>,
    child_slot: Arc<Mutex<Option<(u64, Child)>>>,
    current_gen: Arc<AtomicU64>,
) {
    // `@DEFAULT_MONITOR@` is the pulse token for "monitor of the default sink",
    // i.e. the mix the user is hearing, not the microphone.
    let spawned = Command::new("parec")
        .args([
            "--format=s16le",
            "--rate=44100",
            "--channels=1",
            "--latency-msec=30",
            "--device=@DEFAULT_MONITOR@",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();

    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            eprintln!("sink: spectrum capture unavailable (parec: {e})");
            retire(generation, &running, &current_gen);
            return;
        }
    };
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        retire(generation, &running, &current_gen);
        return;
    };

    // Publish the handle so `stop()` can kill it; kill anything stale first.
    if let Ok(mut slot) = child_slot.lock() {
        if let Some((_, mut old)) = slot.replace((generation, child)) {
            let _ = old.kill();
            let _ = old.wait();
        }
    }

    let window = hann_window(FFT_SIZE);
    let twiddles = twiddle_table(FFT_SIZE);
    let bins = band_bins();
    let mut raw = vec![0u8; FFT_SIZE * 2]; // s16le
    let mut spec: Vec<(f32, f32)> = vec![(0.0, 0.0); FFT_SIZE];
    let mut level = [0.0f32; BANDS];
    let mut peak = [0.0f32; BANDS];

    while running.load(Ordering::SeqCst) && current_gen.load(Ordering::SeqCst) == generation {
        // Blocks until a full analysis window has arrived (~23 ms of audio).
        if stdout.read_exact(&mut raw).is_err() {
            break;
        }

        for (i, s) in spec.iter_mut().enumerate() {
            let lo = raw[i * 2] as u16;
            let hi = raw[i * 2 + 1] as u16;
            let sample = ((hi << 8) | lo) as i16 as f32 / 32_768.0;
            *s = (sample * window[i], 0.0);
        }
        fft_in_place(&mut spec, &twiddles);

        for (b, &(lo, hi)) in bins.iter().enumerate() {
            // Peak-in-band, not average: a single strong partial should show up
            // even when it shares a wide high band with quiet neighbours.
            let mut mag = 0.0f32;
            for s in &spec[lo..hi] {
                let m = (s.0 * s.0 + s.1 * s.1).sqrt();
                if m > mag {
                    mag = m;
                }
            }
            // Hann coherent gain is 0.5, so a full-scale sine peaks at N/4.
            let norm = mag * 4.0 / FFT_SIZE as f32;
            // Tilt the top of the spectrum up; music rolls off steeply there
            // and untilted bars would sit flat on the floor.
            let tilt = b as f32 * 0.55;
            let db = 20.0 * norm.max(1e-6).log10() + tilt;
            let v = ((db + FLOOR_DB) / FLOOR_DB).clamp(0.0, 1.0);

            // Fast attack / slow release so transients pop but decay smoothly.
            let coeff = if v > level[b] { 0.65 } else { 0.22 };
            level[b] += (v - level[b]) * coeff;

            if level[b] >= peak[b] {
                peak[b] = level[b];
            } else {
                peak[b] = (peak[b] - 0.02).max(0.0);
            }
        }

        // Same guard on the publish: a retiring thread must not resurrect
        // bands that `stop()` has already reset, nor stamp its stale block
        // over a newer generation's.
        if current_gen.load(Ordering::SeqCst) == generation {
            if let Ok(mut published) = bands.lock() {
                published.level = level;
                published.peak = peak;
                published.valid = true;
            }
        }
    }

    retire(generation, &running, &current_gen);
    // Only clear the slot if it still holds *our* child; a newer generation may
    // already have taken over.
    if let Ok(mut slot) = child_slot.lock() {
        if slot.as_ref().is_some_and(|(g, _)| *g == generation) {
            if let Some((_, mut c)) = slot.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }
}

// ================================== DSP =====================================

/// Hann window — cheap, and its wide-ish main lobe suits a 24-bar display far
/// better than a rectangular window's spectral leakage.
fn hann_window(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| {
            let x = std::f32::consts::TAU * i as f32 / n as f32;
            0.5 - 0.5 * x.cos()
        })
        .collect()
}

/// Precomputed twiddles `e^(-2*pi*i*k/n)` for k in 0..n/2. Computing them once
/// keeps the butterflies free of trig and avoids the error accumulation of a
/// recurrence.
fn twiddle_table(n: usize) -> Vec<(f32, f32)> {
    (0..n / 2)
        .map(|k| {
            let a = -std::f32::consts::TAU * k as f32 / n as f32;
            (a.cos(), a.sin())
        })
        .collect()
}

/// In-place radix-2 Cooley–Tukey FFT over interleaved (re, im) pairs.
/// `buf.len()` must be a power of two and `tw.len()` must be `buf.len() / 2`;
/// anything else is a no-op rather than a panic.
fn fft_in_place(buf: &mut [(f32, f32)], tw: &[(f32, f32)]) {
    let n = buf.len();
    if n < 2 || !n.is_power_of_two() || tw.len() < n / 2 {
        return;
    }

    // Decimation in time: reorder into bit-reversed index order first.
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            buf.swap(i, j);
        }
    }

    let mut len = 2;
    while len <= n {
        let half = len / 2;
        let step = n / len; // stride into the full-length twiddle table
        let mut base = 0;
        while base < n {
            for k in 0..half {
                let (wr, wi) = tw[k * step];
                let (ar, ai) = buf[base + k];
                let (br, bi) = buf[base + k + half];
                let tr = br * wr - bi * wi;
                let ti = br * wi + bi * wr;
                buf[base + k] = (ar + tr, ai + ti);
                buf[base + k + half] = (ar - tr, ai - ti);
            }
            base += len;
        }
        len <<= 1;
    }
}

/// Half-open `[lo, hi)` FFT bin ranges for each bar, spaced logarithmically so
/// the bass end doesn't collapse into a single bar.
fn band_bins() -> [(usize, usize); BANDS] {
    let mut out = [(0usize, 0usize); BANDS];
    let nyquist_bins = FFT_SIZE / 2;
    let hz_per_bin = SAMPLE_RATE / FFT_SIZE as f32;
    let bin_of = |f: f32| ((f / hz_per_bin).round() as usize).clamp(1, nyquist_bins - 1);

    let ratio = F_HI / F_LO;
    let mut lo = bin_of(F_LO);
    for (b, slot) in out.iter_mut().enumerate() {
        let edge = F_LO * ratio.powf((b + 1) as f32 / BANDS as f32);
        // Every band must own at least one bin, otherwise the low bars (whose
        // edges round to the same bin) would always read zero.
        let hi = bin_of(edge).max(lo + 1).min(nyquist_bins);
        *slot = (lo.min(hi - 1), hi);
        lo = hi;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_finds_a_pure_tone() {
        // A sine at exactly bin 16 should put all its energy in bin 16.
        let n = 256;
        let tw = twiddle_table(n);
        let mut buf: Vec<(f32, f32)> = (0..n)
            .map(|i| {
                let p = std::f32::consts::TAU * 16.0 * i as f32 / n as f32;
                (p.sin(), 0.0)
            })
            .collect();
        fft_in_place(&mut buf, &tw);
        let mag = |k: usize| (buf[k].0 * buf[k].0 + buf[k].1 * buf[k].1).sqrt();
        let loudest = (1..n / 2).max_by(|a, b| mag(*a).total_cmp(&mag(*b)));
        assert_eq!(loudest, Some(16));
    }

    #[test]
    fn fft_rejects_non_power_of_two() {
        let mut buf = vec![(1.0f32, 0.0f32); 6];
        fft_in_place(&mut buf, &twiddle_table(8));
        assert_eq!(buf[0], (1.0, 0.0)); // untouched, not panicking
    }

    #[test]
    fn bands_are_monotonic_and_non_empty() {
        let bins = band_bins();
        for (lo, hi) in bins {
            assert!(lo < hi, "empty band {lo}..{hi}");
            assert!(hi <= FFT_SIZE / 2);
        }
        for w in bins.windows(2) {
            assert!(w[0].0 <= w[1].0);
        }
    }

    #[test]
    fn renders_placeholder_before_capture() {
        // Must not panic and must not need a running parec.
        let s = Spectrum::new();
        let _ = s.render();
    }
}
