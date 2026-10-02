//! Built-in noise suppression: RNNoise (pure-Rust nnnoiseless) ahead of the
//! gate. It only works on fixed 10 ms frames at 48 kHz, so samples are
//! re-blocked through one frame of delay - that frame is all the latency it
//! adds. Per frame it neither allocates nor locks; its FFT plans are cached
//! thread-locally, built once on the RT thread's first frame.

use nnnoiseless::DenoiseState;

const FRAME: usize = DenoiseState::FRAME_SIZE;
/// RNNoise was trained on 16-bit sample values, not unit floats.
const PCM_SCALE: f32 = 32768.0;

pub struct Denoiser {
    state: Box<DenoiseState<'static>>,
    input: [f32; FRAME],
    output: [f32; FRAME],
    pos: usize,
}

impl Denoiser {
    pub fn new() -> Self {
        Self {
            state: DenoiseState::new(),
            input: [0.0; FRAME],
            output: [0.0; FRAME],
            pos: 0,
        }
    }

    /// Drop the buffered frame, so re-enabling never replays audio captured
    /// before the user switched suppression off.
    pub fn clear(&mut self) {
        self.input = [0.0; FRAME];
        self.output = [0.0; FRAME];
        self.pos = 0;
    }

    /// Denoise in place; the output trails the input by exactly one frame.
    pub fn process(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            self.input[self.pos] = *s * PCM_SCALE;
            *s = self.output[self.pos] / PCM_SCALE;
            self.pos += 1;
            if self.pos == FRAME {
                self.state.process_frame(&mut self.output, &self.input);
                self.pos = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
    }

    /// Deterministic white noise (xorshift), no rand dependency.
    fn noise(n: usize, amp: f32) -> Vec<f32> {
        let mut x = 0x2545_f491_u32;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x as f32 / u32::MAX as f32 * 2.0 - 1.0) * amp
            })
            .collect()
    }

    #[test]
    fn output_trails_input_by_one_frame() {
        let mut d = Denoiser::new();
        let mut buf = vec![0.5f32; FRAME - 1];
        d.process(&mut buf);
        assert!(
            buf.iter().all(|&s| s == 0.0),
            "nothing out before a full frame"
        );
    }

    #[test]
    fn odd_block_sizes_match_whole_frames() {
        let input = noise(FRAME * 8, 0.1);
        let mut whole = input.clone();
        Denoiser::new().process(&mut whole);

        let mut chunked = input.clone();
        let mut d = Denoiser::new();
        for chunk in chunked.chunks_mut(37) {
            d.process(chunk);
        }
        assert_eq!(whole, chunked);
    }

    #[test]
    fn suppresses_steady_noise() {
        let mut buf = noise(48_000 * 2, 0.05);
        let before = rms(&buf[48_000..]);
        Denoiser::new().process(&mut buf);
        // Judge the second half, after the model has settled on the noise.
        let after = rms(&buf[48_000..]);
        // At least 12 dB down (measured ~15 dB on white noise).
        assert!(after < before * 0.25, "{before} -> {after}");
    }

    #[test]
    fn clear_drops_the_buffered_frame() {
        let mut d = Denoiser::new();
        let mut loud = vec![0.5f32; FRAME + FRAME / 2];
        d.process(&mut loud);
        d.clear();
        let mut silence = vec![0.0f32; FRAME];
        d.process(&mut silence);
        assert!(silence.iter().all(|&s| s == 0.0));
    }
}
