use std::sync::{Arc, Mutex};

use crate::audio::backend::AudioBackend;
use crate::headset::HeadsetMonitor;
use crate::mixer::state::MixerState;

/// Application state managed by Tauri and shared across commands and the tray.
pub struct AppState {
    pub backend: Arc<dyn AudioBackend>,
    /// True when the native PipeWire backend is driving (vs pactl fallback).
    pub backend_native: bool,
    pub mixer: Mutex<MixerState>,
    /// Opt-in SteelSeries headset-off detector (idle until the setting is on).
    pub headset: HeadsetMonitor,
}

impl AppState {
    /// Lock the mixer state, mapping poisoning to a command-friendly error.
    /// All command handlers go through this instead of hand-rolled map_errs.
    pub fn lock_mixer(&self) -> Result<std::sync::MutexGuard<'_, MixerState>, String> {
        self.mixer
            .lock()
            .map_err(|_| "mixer state lock poisoned".to_string())
    }

    pub fn new(backend: Arc<dyn AudioBackend>, backend_native: bool) -> Self {
        // Saved assignments are loaded eagerly so auto-routing can enforce
        // them as soon as the sinks exist.
        let channel_defs = crate::persistence::channels::Channels::load();
        let buses = crate::persistence::buses::Buses::load(&channel_defs);
        let active_profile = crate::persistence::active::load();
        // Cache the active profile's trigger once so autosave never has to
        // re-read the profile file to preserve it.
        let active_trigger = active_profile
            .as_deref()
            .and_then(|name| crate::persistence::profiles::load(name).ok())
            .and_then(|p| p.trigger_device);
        let mixer = MixerState {
            assignments: crate::persistence::assignments::Assignments::load(),
            aliases: crate::persistence::aliases::Aliases::load(),
            outputs: crate::persistence::outputs::ChannelOutputs::load(),
            mic: crate::persistence::mic::load(),
            channel_defs,
            buses,
            seen: crate::persistence::seen::SeenApps::load(),
            active_profile,
            active_trigger,
            prefs: crate::persistence::prefs::Prefs::load(),
            ..MixerState::default()
        };
        Self {
            backend,
            backend_native,
            mixer: Mutex::new(mixer),
            headset: HeadsetMonitor::new(),
        }
    }

    /// Best-effort teardown of all virtual sinks. Collects error messages
    /// instead of aborting on the first failure so a single bad unload
    /// doesn't leave the remaining sinks behind.
    pub fn teardown_virtual_sinks(&self) -> Vec<String> {
        let names: Vec<String> = self
            .mixer
            .lock()
            .map(|m| m.channel_defs.channels.iter().map(|c| c.name.clone()).collect())
            .unwrap_or_default();
        let mut errors = Vec::new();
        for name in names {
            if let Err(e) = self.backend.destroy_virtual_sink(&name) {
                errors.push(format!("{name}: {e}"));
            }
        }
        if let Ok(mut mixer) = self.mixer.lock() {
            // Persist freshest last-seen timestamps on the way out (the
            // poll only writes on structural changes).
            let _ = mixer.seen.save();
            mixer.reset();
        }
        errors
    }
}
