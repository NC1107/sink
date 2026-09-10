use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

/// Maximum concurrent meters (channels + mic, with headroom).
pub const MAX_METERS: usize = 12;

/// Lock-free per-meter peak store with a dynamic name→slot registry
/// (channels are user-defined since the dynamic-channels work). Peaks are
/// written by realtime meter/DSP callbacks and drained by the level
/// emitter; values are f32 amplitudes bit-cast into AtomicU32.
pub struct LevelStore {
    peaks: [[AtomicU32; 2]; MAX_METERS],
    slots: Mutex<SlotRegistry>,
}

#[derive(Default)]
struct SlotRegistry {
    by_name: HashMap<String, usize>,
    free: Vec<usize>,
}

impl LevelStore {
    pub fn new() -> Self {
        Self {
            peaks: Default::default(),
            slots: Mutex::new(SlotRegistry::default()),
        }
    }

    /// Slot for `name`, registering it on first use. None when the meter
    /// budget is exhausted.
    pub fn slot_for(&self, name: &str) -> Option<usize> {
        let mut registry = self.slots.lock().ok()?;
        if let Some(slot) = registry.by_name.get(name) {
            return Some(*slot);
        }
        let slot = registry.free.pop().or_else(|| {
            let next = registry.by_name.len() + registry.free.len();
            (next < MAX_METERS).then_some(next)
        })?;
        registry.by_name.insert(name.to_string(), slot);
        Some(slot)
    }

    /// Free a name's slot for reuse (channel deleted).
    pub fn release(&self, name: &str) {
        if let Ok(mut registry) = self.slots.lock() {
            if let Some(slot) = registry.by_name.remove(name) {
                self.peaks[slot][0].store(0, Ordering::Relaxed);
                self.peaks[slot][1].store(0, Ordering::Relaxed);
                registry.free.push(slot);
            }
        }
    }

    /// Snapshot of registered meter names and their slots.
    pub fn names(&self) -> Vec<(String, usize)> {
        self.slots
            .lock()
            .map(|r| r.by_name.iter().map(|(n, s)| (n.clone(), *s)).collect())
            .unwrap_or_default()
    }

    /// Raise the stored peak for a meter channel (kept until drained).
    pub fn raise(&self, slot: usize, channel: usize, amplitude: f32) {
        let Some(cell) = self.peaks.get(slot).map(|p| &p[channel.min(1)]) else {
            return;
        };
        let new = amplitude.to_bits();
        let mut current = cell.load(Ordering::Relaxed);
        while f32::from_bits(current) < amplitude {
            match cell.compare_exchange_weak(current, new, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    /// Read and reset the peak for a meter channel.
    pub fn drain(&self, slot: usize, channel: usize) -> f32 {
        self.peaks
            .get(slot)
            .map(|p| f32::from_bits(p[channel.min(1)].swap(0, Ordering::Relaxed)))
            .unwrap_or(0.0)
    }
}

impl Default for LevelStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raise_keeps_maximum_and_drain_resets() {
        let store = LevelStore::new();
        let slot = store.slot_for("sink_game").expect("slot");
        store.raise(slot, 0, 0.5);
        store.raise(slot, 0, 0.3); // lower - ignored
        assert!((store.drain(slot, 0) - 0.5).abs() < f32::EPSILON);
        assert_eq!(store.drain(slot, 0), 0.0); // drained
    }

    #[test]
    fn slots_are_stable_and_reusable() {
        let store = LevelStore::new();
        let a = store.slot_for("sink_game").expect("slot");
        assert_eq!(store.slot_for("sink_game"), Some(a), "stable per name");
        let b = store.slot_for("sink_chat").expect("slot");
        assert_ne!(a, b);
        store.release("sink_game");
        let c = store.slot_for("sink_voice").expect("slot");
        assert_eq!(c, a, "freed slot is reused");
    }

    #[test]
    fn budget_is_enforced() {
        let store = LevelStore::new();
        for i in 0..MAX_METERS {
            assert!(store.slot_for(&format!("m{i}")).is_some());
        }
        assert!(store.slot_for("overflow").is_none());
    }
}
