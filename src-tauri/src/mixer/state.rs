use std::collections::HashSet;

use crate::audio::types::VirtualSink;
use crate::persistence::aliases::Aliases;
use crate::persistence::assignments::Assignments;
use crate::persistence::channels::Channels;

/// In-memory mixer state: the source of truth for channel volume/mute as
/// set through the UI, plus the persistent app→channel assignments.
#[derive(Debug, Default)]
pub struct MixerState {
    pub channels: Vec<VirtualSink>,
    /// User-defined channel set (persisted to disk).
    pub channel_defs: Channels,
    /// True once `init_virtual_devices` has created the sinks.
    pub initialized: bool,
    /// Saved app→channel assignments (persisted to disk + WirePlumber conf).
    pub assignments: Assignments,
    /// User-chosen display names for discovered apps (persisted to disk).
    pub aliases: Aliases,
    /// Per-channel output device choices (persisted to disk).
    pub outputs: crate::persistence::outputs::ChannelOutputs,
    /// Per-channel parametric EQ configs (persisted to disk).
    pub eq: crate::persistence::eq::ChannelEq,
    /// Mic chain configuration (persisted to disk).
    pub mic: crate::audio::types::MicConfig,
    /// Every app identity ever observed (history + ignore list).
    pub seen: crate::persistence::seen::SeenApps,
    /// Unix seconds of the last `seen` write. The poll only saves on
    /// structural changes, so this drives a slow flush that bounds how stale
    /// on-disk `last_seen` timestamps can get if Sink dies without a clean
    /// quit - the age-based prune trusts them.
    pub seen_saved_at: u64,
    /// Profile changes autosave into this profile (live-bound, not a
    /// snapshot). None = unmanaged state.
    pub active_profile: Option<String>,
    /// Cached trigger device of `active_profile`, so autosave preserves it
    /// without re-reading the profile file on every mutation. Kept in step
    /// whenever the active profile or its trigger changes.
    pub active_trigger: Option<String>,
    /// User-defined mixes (record buses), persisted to disk.
    pub buses: crate::persistence::buses::Buses,
    /// App preferences (device naming etc.), persisted to disk.
    pub prefs: crate::persistence::prefs::Prefs,
    /// Streams already considered for auto-routing this session, by
    /// `object.serial`. Each stream is enforced once, on first sight, so a
    /// user moving a stream elsewhere (here or in pavucontrol) isn't fought
    /// every poll. Serials, not node ids: PipeWire recycles node ids fast
    /// enough that an app closing one stream and opening another between two
    /// polls would inherit the old entry and never get routed.
    pub auto_routed: HashSet<u64>,
}

impl MixerState {
    /// Populate the channel strips from the user's channel definitions,
    /// each restored to its persisted volume/mute (100%/unmuted for a
    /// channel that has never been touched).
    pub fn init_defaults(&mut self) {
        self.channels = self
            .channel_defs
            .channels
            .iter()
            .map(|def| VirtualSink {
                name: def.name.clone(),
                label: def.label.clone(),
                icon: def.icon.clone(),
                volume_percent: def.volume_percent,
                muted: def.muted,
                stream_mix: def.stream_mix,
            })
            .collect();
        self.initialized = true;
    }

    pub fn channel_mut(&mut self, sink_name: &str) -> Option<&mut VirtualSink> {
        self.channels.iter_mut().find(|c| c.name == sink_name)
    }

    /// Forget history entries the user never acted on and hasn't seen in a
    /// week, so the "not running" list stays about apps they actually use.
    /// Returns true when the history changed and should be saved.
    pub fn prune_stale_apps(&mut self, now: u64) -> bool {
        // Disjoint field borrows: `prune` needs `seen` mutably while the
        // intent test reads the other two.
        let Self {
            seen,
            assignments,
            aliases,
            ..
        } = self;
        seen.prune(
            now,
            crate::persistence::seen::MAX_SEEN_AGE_SECS,
            |prop, value| {
                assignments.sink_for(prop, value).is_some() || aliases.get(prop, value).is_some()
            },
        )
    }

    pub fn reset(&mut self) {
        self.channels.clear();
        self.initialized = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_defaults_creates_four_channels() {
        let mut state = MixerState::default();
        state.init_defaults();
        assert_eq!(state.channels.len(), 4);
        assert!(state.initialized);
        assert_eq!(state.channels[0].name, "sink_game");
        assert_eq!(state.channels[0].label, "Game");
        assert!(state.channels.iter().all(|c| c.volume_percent == 100 && !c.muted));
    }

    #[test]
    fn prune_stale_apps_exempts_assigned_and_aliased() {
        const DAY: u64 = 24 * 60 * 60;
        let now = 100 * DAY;
        let old = now - 30 * DAY;
        let mut state = MixerState::default();
        for value in ["plain", "assigned", "aliased"] {
            state.seen.upsert("application.name", value, value, None, old);
        }
        state
            .assignments
            .set("application.name", "assigned", "sink_game");
        state.aliases.set("application.name", "aliased", "My App");

        assert!(state.prune_stale_apps(now));
        assert!(state.seen.get("application.name", "plain").is_none());
        assert!(state.seen.get("application.name", "assigned").is_some());
        assert!(state.seen.get("application.name", "aliased").is_some());
    }

    #[test]
    fn channel_mut_finds_by_name() {
        let mut state = MixerState::default();
        state.init_defaults();
        let chat = state.channel_mut("sink_chat").expect("chat channel exists");
        chat.volume_percent = 85;
        assert_eq!(state.channels[1].volume_percent, 85);
        assert!(state.channel_mut("sink_nope").is_none());
    }
}
