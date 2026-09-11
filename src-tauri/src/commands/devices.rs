use std::collections::{HashMap, HashSet};

use tauri::State;

use crate::audio::identity::{self, Identity};
use crate::audio::types::{AppStream, OutputDevice, VirtualSink};
use crate::audio::{icons, steam};
use crate::mixer::state::MixerState;
use crate::persistence::assignments::identity_key;
use crate::state::AppState;

/// How often the poll force-saves app history to refresh `last_seen` on disk.
const SEEN_FLUSH_SECS: u64 = 15 * 60;

/// Current channel state (volume/mute as tracked by MixerState).
#[tauri::command]
pub fn get_virtual_devices(state: State<'_, AppState>) -> Result<Vec<VirtualSink>, String> {
    let mixer = state.lock_mixer()?;
    Ok(mixer.channels.clone())
}

/// All running app audio streams.
#[tauri::command]
pub fn get_app_streams(state: State<'_, AppState>) -> Result<Vec<AppStream>, String> {
    state.note_ui_stream_poll();
    refresh_streams(state.inner())
}

/// List live streams, record history, and enforce saved assignments - each
/// stream once, on first sight. Idempotent; the ticker and the UI both call it.
pub fn refresh_streams(state: &AppState) -> Result<Vec<AppStream>, String> {
    let _gate = state
        .refresh_gate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut streams = live_streams(state)?;
    // A stream whose client facts are still arriving may yet resolve to a
    // different identity; recording, adopting or auto-routing it now would
    // stick (the ledger and history key on what it looked like first).
    streams.retain(|s| s.settled);

    let now = crate::persistence::unix_now();

    // Phase 1: under the lock, update history and *plan* auto-routing - but do
    // no blocking work. Holding the mixer mutex across the disk save or the
    // backend move calls (each up to the native backend's 3s request timeout)
    // would stall every other command - including tray-menu building - behind
    // this 2s poll, and slow-loop polls would stack up (TD-004). So we snapshot
    // the decisions here and release the guard before touching disk or PipeWire.
    let (seen_to_save, planned, rules_to_save) = {
        let mut mixer = state.lock_mixer()?;
        let mut structural_change = false;
        for stream in &streams {
            structural_change |= mixer.seen.upsert(
                &stream.match_prop,
                &stream.match_value,
                &stream.app_name,
                stream.icon_name.as_deref(),
                stream.icon_path.as_deref(),
                now,
            );
        }

        let (rules_changed, merged) = adopt_legacy(&mut mixer, &streams);
        structural_change |= merged;
        // A pure last_seen bump never reports a structural change, so without
        // this the freshest timestamps only reach disk on a clean tray-quit -
        // and an unclean exit would leave a daily-used app looking stale
        // enough for the prune to forget it. Flushing on a slow cadence
        // bounds that drift, and re-runs the prune for sessions that outlive
        // the window.
        if now.saturating_sub(mixer.seen_saved_at) >= SEEN_FLUSH_SECS {
            mixer.prune_stale_apps(now);
            mixer.seen_saved_at = now;
            structural_change = true;
        }

        // Hide ignored identities (also exempts them from auto-routing).
        streams.retain(|s| !mixer.seen.is_ignored(&s.match_prop, &s.match_value));

        let planned = mixer.plan_auto_routes(&streams);

        // User-chosen display names (in-memory read, cheap enough to keep here).
        for stream in &mut streams {
            stream.alias = mixer
                .aliases
                .get(&stream.match_prop, &stream.match_value)
                .map(str::to_string);
        }

        // Snapshot for out-of-lock saves, only when something changed.
        (
            structural_change.then(|| mixer.seen.clone()),
            planned,
            rules_changed.then(|| (mixer.assignments.clone(), mixer.aliases.clone())),
        )
    };

    // Phase 2: the blocking work, with the lock released.
    if let Some(seen) = seen_to_save {
        if let Err(e) = seen.save() {
            eprintln!("sink: saving app history failed: {e}");
        }
    }
    if let Some((assignments, aliases)) = rules_to_save {
        if let Err(e) = assignments.save() {
            eprintln!("sink: saving migrated assignments failed: {e}");
        }
        if let Err(e) = aliases.save() {
            eprintln!("sink: saving migrated aliases failed: {e}");
        }
    }
    for (index, target, app_name) in planned {
        match state.backend.move_stream_to_sink(index, &target) {
            // Reflect the successful move in the snapshot returned to the UI.
            Ok(()) => {
                if let Some(s) = streams.iter_mut().find(|s| s.index == index) {
                    s.assigned_sink = Some(target);
                }
            }
            Err(e) => eprintln!("sink: auto-route of {app_name} (#{index}) failed: {e}"),
        }
    }

    Ok(streams)
}

/// The live streams as the UI sees them: identities resolved, icons found.
pub fn live_streams(state: &AppState) -> Result<Vec<AppStream>, String> {
    let mut streams = state
        .backend
        .list_app_streams()
        .map_err(|e| e.to_string())?;
    resolve_identities(state, &mut streams);
    Ok(streams)
}

/// Ask the process before the stream (see `audio::identity`); cached per
/// serial so `/proc` is read once per stream, not per tick.
fn resolve_identities(state: &AppState, streams: &mut [AppStream]) {
    let live: HashSet<u64> = streams.iter().map(|s| s.serial).collect();
    // The cache lock is not held over the `/proc` and Steam-library walk:
    // a user command resolving a single stream would otherwise queue
    // behind the whole tick.
    let known: Vec<Option<Identity>> = {
        let mut cache = state
            .identity_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.retain(|serial, _| live.contains(serial));
        streams
            .iter()
            .map(|s| cache.get(&s.serial).cloned())
            .collect()
    };
    let mut ids: Vec<Identity> = streams
        .iter()
        .zip(known)
        .map(|(s, known)| {
            known.unwrap_or_else(|| {
                identity::resolve(
                    &s.props,
                    &identity::Proc,
                    &icons::Desktops,
                    &steam::SteamLibrary,
                )
            })
        })
        .collect();
    let props: Vec<&HashMap<String, String>> = streams.iter().map(|s| &s.props).collect();
    identity::adopt_from_siblings(&mut ids, &props);
    {
        let mut cache = state
            .identity_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (stream, id) in streams.iter().zip(&ids) {
            // Unsettled facts may still change; don't cache them.
            if stream.settled {
                cache.insert(stream.serial, id.clone());
            }
        }
    }
    for (stream, id) in streams.iter_mut().zip(ids) {
        stream.match_prop = id.prop;
        stream.match_value = id.value;
        stream.app_name = id.display;
        stream.pid = id.pid;
    }

    let mut icons_by_identity = state
        .icon_cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let live: HashSet<String> = streams
        .iter()
        .map(|s| identity_key(&s.match_prop, &s.match_value))
        .collect();
    icons_by_identity.retain(|key, _| live.contains(key));
    for stream in streams.iter_mut() {
        let key = identity_key(&stream.match_prop, &stream.match_value);
        // An unsettled stream's facts may still change; look them up but
        // don't keep them.
        let facts = match icons_by_identity.get(&key) {
            Some(facts) => facts.clone(),
            None if stream.settled => icons_by_identity
                .entry(key)
                .or_insert_with(|| icon_facts(stream))
                .clone(),
            None => icon_facts(stream),
        };
        stream.icon_path = facts.icon_path;
        if let Some(name) = facts.display_name {
            stream.app_name = name;
        }
    }
}

/// Icon and name for a freshly identified stream: the desktop entry's name
/// beats a bare exe or stream name, never a process identity's own.
fn icon_facts(stream: &AppStream) -> icons::IconFacts {
    let binary =
        (stream.match_prop == "application.process.binary").then_some(stream.match_value.as_str());
    let resolved = icons::resolve(
        &stream.app_name,
        binary,
        stream.icon_name.as_deref(),
        stream.pid,
    );
    let renames =
        !identity::is_process_prop(&stream.match_prop) || stream.match_prop == identity::PROP_EXE;
    icons::IconFacts {
        icon_path: icons::identity_icon(
            &stream.match_prop,
            &stream.match_value,
            resolved.icon_path,
        ),
        display_name: resolved.display_name.filter(|_| renames),
    }
}

/// Carry legacy stream-keyed rules, aliases and ignores over to the process
/// identity; the legacy rule stays, a generic one may route another game.
fn adopt_legacy(mixer: &mut MixerState, streams: &[AppStream]) -> (bool, bool) {
    let (mut rules, mut history) = (false, false);
    for stream in streams {
        if !identity::is_process_prop(&stream.match_prop) {
            continue;
        }
        let (prop, value) = (&stream.match_prop, &stream.match_value);
        let matchers = identity::legacy_matchers(&stream.props);
        if mixer
            .assignments
            .adopt_all(&matchers, prop, value)
            .is_some()
        {
            rules = true;
        }
        for (lprop, lvalue) in matchers {
            // The legacy alias stays: another app may share the stream name,
            // and an older version still reads it.
            if mixer.aliases.get(prop, value).is_none() {
                if let Some(alias) = mixer.aliases.get(&lprop, &lvalue).map(str::to_string) {
                    mixer.aliases.set(prop, value, &alias);
                    rules = true;
                }
            }
            if let Some(legacy) = mixer.seen.get(&lprop, &lvalue).cloned() {
                if legacy.ignored {
                    mixer.seen.set_ignored(prop, value, true);
                }
                mixer.seen.forget(&lprop, &lvalue);
                history = true;
            }
        }
    }
    (rules, history)
}

/// Physical output devices (everything that isn't one of our virtual sinks).
#[tauri::command]
pub fn get_output_devices(state: State<'_, AppState>) -> Result<Vec<OutputDevice>, String> {
    state
        .backend
        .list_output_devices()
        .map_err(|e| e.to_string())
}

/// Create the user's virtual sinks and reset them to 100%, unmuted.
/// Idempotent: safe to call again if the sinks already exist.
#[tauri::command]
pub fn init_virtual_devices(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let (defs, prefs) = {
        let mixer = state.lock_mixer()?;
        (mixer.channel_defs.clone(), mixer.prefs.clone())
    };

    for def in &defs.channels {
        state
            .backend
            .create_virtual_sink(&def.name, &prefs.decorate(&def.label))
            .map_err(|e| e.to_string())?;
        // Restore the saved level explicitly - an adopted sink from a
        // previous run may carry a stale volume/mute, so this is a set,
        // not a "leave it alone".
        state
            .backend
            .set_sink_volume(&def.name, def.volume_percent)
            .map_err(|e| e.to_string())?;
        state
            .backend
            .set_sink_mute(&def.name, def.muted)
            .map_err(|e| e.to_string())?;
    }

    let (outputs, eq, mic, buses) = {
        let mut mixer = state.lock_mixer()?;
        mixer.init_defaults();
        // The master mix always carries the full channel set.
        let names: Vec<String> = defs.channels.iter().map(|c| c.name.clone()).collect();
        mixer.buses.sync_master(&names);
        (
            mixer.outputs.clone(),
            mixer.eq.clone(),
            mixer.mic.clone(),
            mixer.buses.clone(),
        )
    };
    if let Err(e) = buses.save() {
        eprintln!("sink: saving mixes failed: {e}");
    }

    // Wire every channel to its saved output (or the system default) so
    // channels are audible from the start.
    for def in &defs.channels {
        if let Err(e) = state
            .backend
            .set_channel_output(&def.name, outputs.get(&def.name))
        {
            eprintln!("sink: output routing for {} failed: {e}", def.name);
        }
        // Restore per-channel failover (default on, so only push the ones off).
        if !outputs.failover(&def.name) {
            if let Err(e) = state.backend.set_channel_failover(&def.name, false) {
                eprintln!("sink: failover setting for {} failed: {e}", def.name);
            }
        }
        // Restore saved EQ (only channels that were ever configured; the
        // loop builds the insert when the sink node appears).
        if let Some(config) = eq.configs.get(&def.name) {
            if let Err(e) = state.backend.set_channel_eq(&def.name, config) {
                eprintln!("sink: eq restore for {} failed: {e}", def.name);
            }
        }
    }

    // Bring up the user's mixes and their memberships.
    let names: Vec<String> = defs.channels.iter().map(|c| c.name.clone()).collect();
    for bus in &buses.buses {
        if let Err(e) = state
            .backend
            .create_bus(&bus.name, &prefs.decorate(&bus.label), bus.role)
        {
            eprintln!("sink: creating mix {} failed: {e}", bus.name);
            continue;
        }
        if let Err(e) = state
            .backend
            .set_bus_members(&bus.name, &bus.effective_members(&names))
        {
            eprintln!("sink: members for mix {} failed: {e}", bus.name);
        }
        if bus.mic {
            if let Err(e) = state.backend.set_bus_mic(&bus.name, true) {
                eprintln!("sink: mic membership for mix {} failed: {e}", bus.name);
            }
        }
        crate::commands::buses::apply_bus_level(state.backend.as_ref(), bus);
        crate::commands::buses::apply_bus_member_gains(state.backend.as_ref(), bus);
    }

    // Bring the mic chain up if it was enabled last session.
    if mic.enabled {
        let mut applied = mic.clone();
        applied.output_label = prefs.decorate(&mic.output_label);
        if let Err(e) = state.backend.set_mic_config(&applied) {
            eprintln!("sink: mic chain init failed: {e}");
            // Keep the UI honest: no chain is running, so don't show the
            // mic as enabled. In-memory only - the on-disk config keeps
            // enabled=true so the next native-backend session restores it.
            if let Ok(mut mixer) = state.lock_mixer() {
                mixer.mic.enabled = false;
            }
        }
    }

    // First run: capture the current layout as the "Default" profile so
    // there's always a known-good state to come back to. It also becomes
    // the active (autosaving) profile.
    if matches!(crate::persistence::profiles::list(), Ok(list) if list.is_empty()) {
        let mut mixer = state.lock_mixer()?;
        let default = crate::persistence::profiles::Profile {
            name: "Default".to_string(),
            channels: mixer.channels.clone(),
            assignments: mixer.assignments.clone(),
            outputs: mixer.outputs.clone(),
            eq: mixer.eq.clone(),
            trigger_device: None,
            buses: mixer.buses.clone(),
        };
        match crate::persistence::profiles::save(&default) {
            Ok(()) => {
                mixer.active_profile = Some(default.name.clone());
                mixer.active_trigger = None; // the Default profile has no trigger
                let _ = crate::persistence::active::save(Some(&default.name));
            }
            Err(e) => eprintln!("sink: creating Default profile failed: {e}"),
        }
    }
    // Profiles/active state may have changed since the tray was built.
    crate::refresh_tray(&app);
    Ok(())
}

/// Current per-channel output choices (None = follow system default).
#[tauri::command]
pub fn get_channel_outputs(
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<String, Option<String>>, String> {
    let mixer = state.lock_mixer()?;
    Ok(mixer
        .channel_defs
        .channels
        .iter()
        .map(|def| {
            (
                def.name.clone(),
                mixer.outputs.get(&def.name).map(str::to_string),
            )
        })
        .collect())
}

/// Per-channel resolved output: the device node.name each channel is actually
/// routed to right now (after explicit/default/fallback resolution). The UI
/// shows this under "System default" so failover is visible. Empty on the
/// pactl fallback, which can't report it.
#[tauri::command]
pub fn get_resolved_outputs(
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<String, Option<String>>, String> {
    state
        .backend
        .resolved_channel_outputs()
        .map_err(|e| e.to_string())
}

/// Whether each channel fails over to another device when its chosen device
/// (or the default) is gone. On unless explicitly turned off.
#[tauri::command]
pub fn get_channel_failover(
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<String, bool>, String> {
    let mixer = state.lock_mixer()?;
    Ok(mixer
        .channel_defs
        .channels
        .iter()
        .map(|def| (def.name.clone(), mixer.outputs.failover(&def.name)))
        .collect())
}

/// Route a channel to an output device; empty `output_name` = follow the
/// system default. Persisted across restarts.
#[tauri::command]
pub fn set_channel_output(
    state: State<'_, AppState>,
    sink_name: String,
    output_name: String,
) -> Result<(), String> {
    let output = if output_name.is_empty() {
        None
    } else {
        Some(output_name)
    };
    state
        .backend
        .set_channel_output(&sink_name, output.as_deref())
        .map_err(|e| e.to_string())?;

    let outputs = {
        let mut mixer = state.lock_mixer()?;
        mixer.outputs.set(&sink_name, output);
        crate::commands::profiles::autosave_active(&mixer);
        mixer.outputs.clone()
    };
    outputs.save().map_err(|e| e.to_string())
}

/// Turn a channel's auto-failover on or off. Off = the channel plays only on
/// its chosen device (or exact default) and stays silent when that's gone.
/// Persisted across restarts.
#[tauri::command]
pub fn set_channel_failover(
    state: State<'_, AppState>,
    sink_name: String,
    enabled: bool,
) -> Result<(), String> {
    state
        .backend
        .set_channel_failover(&sink_name, enabled)
        .map_err(|e| e.to_string())?;

    let outputs = {
        let mut mixer = state.lock_mixer()?;
        mixer.outputs.set_failover(&sink_name, enabled);
        crate::commands::profiles::autosave_active(&mixer);
        mixer.outputs.clone()
    };
    outputs.save().map_err(|e| e.to_string())
}

/// Destroy all virtual sinks. Called before the app exits.
#[tauri::command]
pub fn teardown_virtual_devices(state: State<'_, AppState>) -> Result<(), String> {
    let errors = state.teardown_virtual_sinks();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::mock::{stream, MockBackend};
    use crate::persistence::testing::TempConfig;
    use std::sync::Arc;

    #[test]
    fn refresh_enforces_an_assignment_once() {
        let _cfg = TempConfig::new("refresh-once");
        let backend = Arc::new(MockBackend::with_streams(vec![stream(
            7, 100, "Firefox", None,
        )]));
        let state = AppState::new(backend.clone(), true);
        {
            let mut mixer = state.lock_mixer().expect("mixer");
            mixer.init_defaults();
            mixer
                .assignments
                .set("application.name", "Firefox", "sink_game");
        }

        refresh_streams(&state).expect("first pass");
        assert_eq!(backend.moves(), vec![(7, "sink_game".to_string())]);

        // The user drags it elsewhere in pavucontrol. The ticker runs five
        // times a second; it must leave that alone rather than dragging the
        // stream back on the next pass.
        backend.set_assigned(7, Some("sink_chat"));
        refresh_streams(&state).expect("second pass");
        assert_eq!(
            backend.moves().len(),
            1,
            "a manual re-route must not be fought"
        );
    }

    #[test]
    fn refresh_adopts_a_legacy_rule_into_the_process_identity() {
        let _cfg = TempConfig::new("refresh-migrate");
        // A sandboxed Spotify: identity comes from the portal app id (no
        // /proc involved), while the rule on disk is keyed the old way.
        let mut spotify = stream(7, 100, "Spotify", None);
        spotify.props.insert(
            "pipewire.access.portal.app_id".into(),
            "com.spotify.Client".into(),
        );
        spotify
            .props
            .insert("pipewire.access".into(), "flatpak".into());
        spotify
            .props
            .insert("application.process.id".into(), "2".into());
        let backend = Arc::new(MockBackend::with_streams(vec![spotify]));
        let state = AppState::new(backend.clone(), true);
        {
            let mut mixer = state.lock_mixer().expect("mixer");
            mixer.init_defaults();
            mixer
                .assignments
                .set("application.name", "Spotify", "sink_music");
            mixer.aliases.set("application.name", "Spotify", "Tunes");
            mixer
                .seen
                .upsert("application.name", "Spotify", "Spotify", None, None, 1);
        }

        let streams = refresh_streams(&state).expect("pass");

        assert_eq!(streams[0].match_prop, identity::PROP_FLATPAK);
        assert_eq!(streams[0].match_value, "com.spotify.Client");
        assert_eq!(streams[0].pid, None, "a sandbox pid is never trusted");
        assert_eq!(
            backend.moves(),
            vec![(7, "sink_music".to_string())],
            "the legacy rule routes the new identity"
        );
        let mixer = state.lock_mixer().expect("mixer");
        assert_eq!(
            mixer
                .assignments
                .sink_for(identity::PROP_FLATPAK, "com.spotify.Client"),
            Some("sink_music")
        );
        assert_eq!(
            mixer.assignments.sink_for("application.name", "Spotify"),
            Some("sink_music"),
            "the legacy rule is kept, never deleted"
        );
        assert_eq!(
            mixer
                .aliases
                .get(identity::PROP_FLATPAK, "com.spotify.Client"),
            Some("Tunes")
        );
        assert_eq!(
            mixer.aliases.get("application.name", "Spotify"),
            Some("Tunes"),
            "the legacy alias is kept for older versions and shared names"
        );
        assert!(
            mixer.seen.get("application.name", "Spotify").is_none(),
            "history merged"
        );
        assert!(mixer
            .seen
            .get(identity::PROP_FLATPAK, "com.spotify.Client")
            .is_some());
        drop(mixer);

        // The user unassigns the app. The legacy rule is still on disk and
        // the app is still playing: the next refresh must not bring it back.
        state
            .lock_mixer()
            .expect("mixer")
            .assignments
            .remove(identity::PROP_FLATPAK, "com.spotify.Client");
        refresh_streams(&state).expect("second pass");
        let mixer = state.lock_mixer().expect("mixer");
        assert!(
            mixer
                .assignments
                .sink_for(identity::PROP_FLATPAK, "com.spotify.Client")
                .is_none(),
            "an unassigned app must stay unassigned"
        );
        assert_eq!(backend.moves().len(), 1, "no second move");
    }

    #[test]
    fn refresh_leaves_unassigned_apps_alone() {
        let _cfg = TempConfig::new("refresh-unassigned");
        let backend = Arc::new(MockBackend::with_streams(vec![stream(
            9, 300, "Spotify", None,
        )]));
        let state = AppState::new(backend.clone(), true);
        state.lock_mixer().expect("mixer").init_defaults();

        let streams = refresh_streams(&state).expect("pass");
        assert!(backend.moves().is_empty());
        assert_eq!(streams.len(), 1);
    }
}
