//! Minimal lock-free SPSC ring buffer for f32 samples, connecting the mic
//! capture callback (producer) to the virtual-source playback callback
//! (consumer). Both run on PipeWire data threads; no locks, no allocation.
//!
//! On overflow the oldest audio is dropped, so the producer may overwrite
//! slots the consumer is still copying. Each cursor has exactly one writer
//! and the consumer checks `claim` after copying (a seqlock), so a lapped
//! read is discarded instead of being played as a mix of old and new audio.

use std::sync::atomic::{fence, AtomicU32, AtomicUsize, Ordering};

pub struct Ring {
    buf: Box<[AtomicU32]>,
    /// End of the samples the producer has finished writing (producer-owned).
    write: AtomicUsize,
    /// End of the samples the producer is writing or has written; runs ahead
    /// of `write` during a push (producer-owned).
    claim: AtomicUsize,
    /// Next read position (consumer-owned - the producer never moves it).
    read: AtomicUsize,
}

impl Ring {
    /// Capacity is rounded up to a power of two; effective capacity is
    /// `capacity - 1` (one slot distinguishes full from empty).
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.next_power_of_two().max(2);
        let buf = (0..cap).map(|_| AtomicU32::new(0)).collect::<Vec<_>>();
        Self {
            buf: buf.into_boxed_slice(),
            write: AtomicUsize::new(0),
            claim: AtomicUsize::new(0),
            read: AtomicUsize::new(0),
        }
    }

    fn mask(&self) -> usize {
        self.buf.len() - 1
    }

    /// Push samples; drops the oldest unread data on overflow (live audio -
    /// stale samples are worthless).
    pub fn push(&self, samples: &[f32]) {
        let mask = self.mask();
        // More than a window's worth would only overwrite itself.
        let samples = &samples[samples.len().saturating_sub(self.buf.len())..];
        let mut w = self.write.load(Ordering::Relaxed);
        self.claim
            .store(w.wrapping_add(samples.len()), Ordering::Relaxed);
        // Readers that see any slot written below also see the claim.
        fence(Ordering::Release);
        for &s in samples {
            self.buf[w & mask].store(s.to_bits(), Ordering::Relaxed);
            w = w.wrapping_add(1);
        }
        self.write.store(w, Ordering::Release);
    }

    /// Pop up to `out.len()` samples; unfilled tail is zeroed (underrun).
    /// Returns the number of real samples written.
    pub fn pop(&self, out: &mut [f32]) -> usize {
        let mask = self.mask();
        let len = self.buf.len();
        let w = self.write.load(Ordering::Acquire);
        let mut r = self.read.load(Ordering::Relaxed);
        // Lapped while idle: only the freshest window still exists.
        if w.wrapping_sub(r) > len {
            r = w.wrapping_sub(len);
        }
        let avail = w.wrapping_sub(r).min(out.len());
        for (i, slot) in out.iter_mut().take(avail).enumerate() {
            *slot = f32::from_bits(self.buf[r.wrapping_add(i) & mask].load(Ordering::Relaxed));
        }
        // Anything the producer claimed more than a window past a slot may
        // have overwritten it while it was being copied: drop those.
        fence(Ordering::Acquire);
        let ahead = self.claim.load(Ordering::Relaxed).wrapping_sub(r);
        let lost = ahead.saturating_sub(len).min(avail);
        out.copy_within(lost..avail, 0);
        let valid = avail - lost;
        self.read.store(r.wrapping_add(avail), Ordering::Release);
        for slot in out.iter_mut().skip(valid) {
            *slot = 0.0;
        }
        valid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_underrun() {
        let ring = Ring::new(8);
        ring.push(&[1.0, 2.0, 3.0]);
        let mut out = [0.0f32; 5];
        let n = ring.pop(&mut out);
        assert_eq!(n, 3);
        assert_eq!(&out[..3], &[1.0, 2.0, 3.0]);
        assert_eq!(&out[3..], &[0.0, 0.0]); // underrun zero-fill
    }

    #[test]
    fn overflow_keeps_freshest_window(// producer overruns consumer
    ) {
        let ring = Ring::new(4); // effective window of 4
        let data: Vec<f32> = (0..10).map(|i| i as f32).collect();
        ring.push(&data);
        let mut out = [0.0f32; 4];
        let n = ring.pop(&mut out);
        assert_eq!(n, 4);
        assert_eq!(out, [6.0, 7.0, 8.0, 9.0]); // freshest 4 survive
    }

    /// The two callbacks run concurrently and the producer laps the consumer
    /// whenever the graphs drift. Whatever pop hands back must be real,
    /// in-order audio: never a slot rewritten mid-read, never a step back.
    #[test]
    fn concurrent_overruns_never_return_torn_or_stale_samples() {
        use std::sync::Arc;
        const TOTAL: u32 = 1 << 20; // exact as f32 (below 2^24)
        let ring = Arc::new(Ring::new(64));
        let producer = {
            let ring = ring.clone();
            std::thread::spawn(move || {
                let mut next = 0u32;
                let mut chunk = [0.0f32; 48];
                while next < TOTAL {
                    for s in chunk.iter_mut() {
                        *s = next as f32;
                        next += 1;
                    }
                    ring.push(&chunk);
                }
            })
        };
        let mut last: Option<u32> = None;
        let mut out = [0.0f32; 32];
        while !producer.is_finished() {
            let n = ring.pop(&mut out);
            for &s in &out[..n] {
                let v = s as u32;
                if let Some(prev) = last {
                    assert!(v > prev, "sample {v} after {prev}: torn or stale read");
                }
                last = Some(v);
            }
            let in_order = out[..n].windows(2).all(|w| w[1] as u32 == w[0] as u32 + 1);
            assert!(
                in_order,
                "one pop returned a non-contiguous run: {:?}",
                &out[..n]
            );
        }
        producer.join().unwrap();
        assert!(last.is_some(), "the consumer never saw a sample");
    }
}
