use tauri::State;

use crate::audio::types::{AppStream, OutputDevice, VirtualSink};
use crate::state::AppState;


/// Current channel state (volume/mute as tracked by MixerState).
#[tauri::command]
pub fn get_virtual_devices(state: State<'_, AppState>) -> Result<Vec<VirtualSink>, String> {
    let mixer = state.lock_mixer()?;
    Ok(mixer.channels.clone())
}

/// All running app audio streams.
///
/// Doubles as the auto-routing enforcement point (Phase 2): the frontend
/// polls this every 2s, and any stream seen for the first time whose app has
/// a saved assignment is moved onto its channel. Each stream is enforced
/// once, so manual re-routing (here or in pavucontrol) isn't fought.
#[tauri::command]
pub fn get_app_streams(state: State<'_, AppState>) -> Result<Vec<AppStream>, String> {
    let mut streams = state.backend.list_app_streams().map_err(|e| e.to_string())?;

    // Desktop-entry resolution: real icon files and polished display names
    // ("spotify" binary → Spotify with its actual icon). Cached per identity.
    for stream in &mut streams {
        let binary = (stream.match_prop == "application.process.binary")
            .then_some(stream.match_value.as_str());
        let resolved = crate::audio::icons::resolve(
            &stream.app_name,
            binary,
            stream.icon_name.as_deref(),
            stream.pid,
        );
        stream.icon_path = resolved.icon_path;
        if let Some(name) = resolved.display_name {
            stream.app_name = name;
        }
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Phase 1: under the lock, update history and *plan* auto-routing - but do
    // no blocking work. Holding the mixer mutex across the disk save or the
    // backend move calls (each up to the native backend's 3s request timeout)
    // would stall every other command - including tray-menu building - behind
    // this 2s poll, and slow-loop polls would stack up (TD-004). So we snapshot
    // the decisions here and release the guard before touching disk or PipeWire.
    let (seen_to_save, planned) = {
        let mut mixer = state.lock_mixer()?;
        let mut structural_change = false;
        for stream in &streams {
            structural_change |= mixer.seen.upsert(
                &stream.match_prop,
                &stream.match_value,
                &stream.app_name,
                stream.icon_name.as_deref(),
                now,
            );
        }
        // Hide ignored identities (also exempts them from auto-routing).
        streams.retain(|s| !mixer.seen.is_ignored(&s.match_prop, &s.match_value));

        // Only enforce once the virtual sinks exist; otherwise streams would be
        // marked handled while their target sink can't be moved to yet.
        let mut planned: Vec<(u32, String, String)> = Vec::new();
        if mixer.initialized {
            for stream in &streams {
                if mixer.auto_routed.contains(&stream.index) {
                    continue;
                }
                if let Some(target) = mixer
                    .assignments
                    .sink_for(&stream.match_prop, &stream.match_value)
                {
                    if stream.assigned_sink.as_deref() != Some(target) {
                        planned.push((stream.index, target.to_string(), stream.app_name.clone()));
                    }
                }
                // Marked handled once (before the move, so a concurrent poll
                // can't re-plan it); manual re-routing then isn't fought.
                mixer.auto_routed.insert(stream.index);
            }
            // Forget streams that have gone away, so the ledger can't grow
            // without bound and a recycled PipeWire index isn't mistaken for one
            // we already handled (which would skip auto-routing a new stream).
            let live: std::collections::HashSet<u32> = streams.iter().map(|s| s.index).collect();
            mixer.auto_routed.retain(|i| live.contains(i));
        }

        // User-chosen display names (in-memory read, cheap enough to keep here).
        for stream in &mut streams {
            stream.alias = mixer
                .aliases
                .get(&stream.match_prop, &stream.match_value)
                .map(str::to_string);
        }

        // Snapshot the history for an out-of-lock save, only when it changed.
        (structural_change.then(|| mixer.seen.clone()), planned)
    };

    // Phase 2: the blocking work, with the lock released.
    if let Some(seen) = seen_to_save {
        if let Err(e) = seen.save() {
            eprintln!("sink: saving app history failed: {e}");
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
        // Known starting point — adopted sinks from a previous run may carry
        // stale volume/mute.
        state
            .backend
            .set_sink_volume(&def.name, 100)
            .map_err(|e| e.to_string())?;
        state
            .backend
            .set_sink_mute(&def.name, false)
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
        if let Err(e) = state.backend.create_bus(&bus.name, &prefs.decorate(&bus.label)) {
            eprintln!("sink: creating mix {} failed: {e}", bus.name);
            continue;
        }
        if let Err(e) = state
            .backend
            .set_bus_members(&bus.name, &bus.effective_members(&names))
        {
            eprintln!("sink: members for mix {} failed: {e}", bus.name);
        }
    }

    // Bring the mic chain up if it was enabled last session.
    if mic.enabled {
        let mut applied = mic.clone();
        applied.output_label = prefs.decorate(&mic.output_label);
        if let Err(e) = state.backend.set_mic_config(&applied) {
            eprintln!("sink: mic chain init failed: {e}");
            // Keep the UI honest: no chain is running, so don't show the
            // mic as enabled. In-memory only — the on-disk config keeps
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
