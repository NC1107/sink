use std::collections::HashSet;

use crate::audio::types::{AppStream, VirtualSink};
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
    /// Streams already auto-routed once, by `object.serial` (node ids
    /// recycle); manual re-routing isn't fought.
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

    /// Decide which live streams to move onto their saved channel, and
    /// record them as handled. Each stream is considered once, on first
    /// sight, so a manual re-route (here or in pavucontrol) isn't fought;
    /// streams that have gone away are forgotten so the ledger stays bounded.
    ///
    /// Returns `(stream index, target sink, app name)` for the caller to
    /// execute once it has released the lock.
    pub fn plan_auto_routes(&mut self, streams: &[AppStream]) -> Vec<(u32, String, String)> {
        // Enforce only once the virtual sinks exist, or streams would be
        // marked handled while their target can't be moved to yet.
        if !self.initialized {
            return Vec::new();
        }
        let mut planned = Vec::new();
        for stream in streams {
            if self.auto_routed.contains(&stream.serial) {
                continue;
            }
            if let Some(target) = self
                .assignments
                .sink_for(&stream.match_prop, &stream.match_value)
            {
                if stream.assigned_sink.as_deref() != Some(target) {
                    planned.push((stream.index, target.to_string(), stream.app_name.clone()));
                }
            }
            self.auto_routed.insert(stream.serial);
        }
        let live: HashSet<u64> = streams.iter().map(|s| s.serial).collect();
        self.auto_routed.retain(|serial| live.contains(serial));
        planned
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

    fn stream(index: u32, serial: u64, value: &str, on: Option<&str>) -> AppStream {
        AppStream {
            index,
            serial,
            app_name: value.to_string(),
            match_prop: "application.name".into(),
            match_value: value.into(),
            alias: None,
            icon_name: None,
            icon_path: None,
            pid: None,
            assigned_sink: on.map(str::to_string),
            volume_percent: 100,
            muted: false,
            active: true,
        }
    }

    #[test]
    fn auto_route_plans_once_and_respects_a_manual_move() {
        let mut state = MixerState::default();
        state.init_defaults();
        state
            .assignments
            .set("application.name", "Firefox", "sink_game");

        // First sight: planned, and marked handled.
        let planned = state.plan_auto_routes(&[stream(7, 100, "Firefox", None)]);
        assert_eq!(
            planned,
            vec![(7, "sink_game".to_string(), "Firefox".to_string())]
        );

        // Seen again, moved elsewhere by hand: not fought.
        let planned = state.plan_auto_routes(&[stream(7, 100, "Firefox", Some("sink_chat"))]);
        assert!(planned.is_empty());
    }

    #[test]
    fn auto_route_skips_a_stream_already_on_target() {
        let mut state = MixerState::default();
        state.init_defaults();
        state
            .assignments
            .set("application.name", "Firefox", "sink_game");
        assert!(state
            .plan_auto_routes(&[stream(7, 100, "Firefox", Some("sink_game"))])
            .is_empty());
    }

    #[test]
    fn auto_route_waits_for_the_sinks_to_exist() {
        let mut state = MixerState::default();
        state
            .assignments
            .set("application.name", "Firefox", "sink_game");
        // Nothing planned, and nothing marked handled - marking here would
        // strand the stream once the sinks arrive.
        assert!(state
            .plan_auto_routes(&[stream(7, 100, "Firefox", None)])
            .is_empty());
        assert!(state.auto_routed.is_empty());
    }

    #[test]
    fn auto_route_reroutes_a_restarted_stream_on_a_recycled_node_id() {
        let mut state = MixerState::default();
        state.init_defaults();
        state
            .assignments
            .set("application.name", "Firefox", "sink_game");
        assert_eq!(
            state.plan_auto_routes(&[stream(7, 100, "Firefox", None)]).len(),
            1
        );

        // The app reopened its stream and PipeWire reused the node id;
        // serials never repeat, so the new stream is still routed.
        let planned = state.plan_auto_routes(&[stream(7, 101, "Firefox", None)]);
        assert_eq!(planned.len(), 1);
    }

    #[test]
    fn auto_route_ledger_forgets_dead_streams() {
        let mut state = MixerState::default();
        state.init_defaults();
        state.plan_auto_routes(&[stream(1, 10, "A", None), stream(2, 11, "B", None)]);
        assert_eq!(state.auto_routed.len(), 2);
        state.plan_auto_routes(&[stream(1, 10, "A", None)]);
        assert_eq!(state.auto_routed, HashSet::from([10]));
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
