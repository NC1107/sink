use tauri::State;

use crate::audio::types::is_virtual_sink;
use crate::persistence::wireplumber;
use crate::state::AppState;

pub(crate) const MAX_VOLUME: u8 = 150;

/// An app's other live streams: same identity, different stream. Routing is
/// per app, so these move with the one the user clicked.
fn siblings_of<'a>(
    streams: &'a [crate::audio::types::AppStream],
    stream: &crate::audio::types::AppStream,
) -> Vec<&'a crate::audio::types::AppStream> {
    streams
        .iter()
        .filter(|s| {
            s.index != stream.index
                && s.match_prop == stream.match_prop
                && s.match_value == stream.match_value
        })
        .collect()
}

/// Move an app stream onto a channel. An empty `sink_name` unassigns the
/// stream (returns it to the system default sink).
///
/// The choice is also recorded as a persistent assignment (Phase 2): saved
/// to `$XDG_CONFIG_HOME/sink/assignments.json`, mirrored to a WirePlumber
/// conf fragment, and re-applied by the stream poll when the app restarts.
#[tauri::command]
pub fn route_app_to_channel(
    state: State<'_, AppState>,
    stream_index: u32,
    sink_name: String,
) -> Result<(), String> {
    if !sink_name.is_empty() && !is_virtual_sink(&sink_name) {
        return Err(format!("unknown channel: {sink_name}"));
    }

    // Assignments are per app, not per stream: siblings move too.
    let streams = state.backend.list_app_streams().map_err(|e| e.to_string())?;
    let Some(stream) = streams.iter().find(|s| s.index == stream_index) else {
        // Vanished between the click and now; move the raw index anyway in
        // case the listing raced, with nothing to record against it.
        return state
            .backend
            .move_stream_to_sink(stream_index, &sink_name)
            .map_err(|e| e.to_string());
    };
    let siblings: Vec<&crate::audio::types::AppStream> = siblings_of(&streams, stream);

    // Record intent before moving, or a concurrent tick undoes the move.
    let assignments = {
        let mut mixer = state.lock_mixer()?;
        if sink_name.is_empty() {
            mixer
                .assignments
                .remove(&stream.match_prop, &stream.match_value);
        } else {
            mixer
                .assignments
                .set(&stream.match_prop, &stream.match_value, &sink_name);
        }
        // The user explicitly placed these streams; don't auto-route them again.
        mixer.auto_routed.insert(stream.serial);
        mixer.auto_routed.extend(siblings.iter().map(|s| s.serial));
        crate::commands::profiles::autosave_active(&mixer);
        mixer.assignments.clone()
    };

    state
        .backend
        .move_stream_to_sink(stream_index, &sink_name)
        .map_err(|e| e.to_string())?;
    for sibling in &siblings {
        if let Err(e) = state.backend.move_stream_to_sink(sibling.index, &sink_name) {
            eprintln!(
                "sink: moving {} (#{}) failed: {e}",
                stream.app_name, sibling.index
            );
        }
    }

    assignments.save().map_err(|e| e.to_string())?;
    wireplumber::write(&assignments).map_err(|e| e.to_string())?;
    Ok(())
}

/// Set a channel's volume (0-150%).
#[tauri::command]
pub fn set_channel_volume(
    state: State<'_, AppState>,
    sink_name: String,
    volume: u8,
) -> Result<(), String> {
    // Only our own channels, so a compromised webview can't touch arbitrary
    // session sinks (TD-050).
    if !is_virtual_sink(&sink_name) {
        return Err(format!("unknown channel: {sink_name}"));
    }
    let volume = volume.min(MAX_VOLUME);
    state
        .backend
        .set_sink_volume(&sink_name, volume)
        .map_err(|e| e.to_string())?;

    let defs = {
        let mut mixer = state.lock_mixer()?;
        if let Some(channel) = mixer.channel_mut(&sink_name) {
            channel.volume_percent = volume;
        }
        // Persist the level itself, not just into an active profile - a
        // channel must come back at the volume you left it at even when
        // no profile is bound.
        mixer.channel_defs.set_volume(&sink_name, volume);
        crate::commands::profiles::autosave_active(&mixer);
        mixer.channel_defs.clone()
    };
    defs.save().map_err(|e| e.to_string())
}

/// Mute or unmute a channel.
#[tauri::command]
pub fn toggle_channel_mute(
    state: State<'_, AppState>,
    sink_name: String,
    muted: bool,
) -> Result<(), String> {
    if !is_virtual_sink(&sink_name) {
        return Err(format!("unknown channel: {sink_name}"));
    }
    state
        .backend
        .set_sink_mute(&sink_name, muted)
        .map_err(|e| e.to_string())?;

    let defs = {
        let mut mixer = state.lock_mixer()?;
        if let Some(channel) = mixer.channel_mut(&sink_name) {
            channel.muted = muted;
        }
        mixer.channel_defs.set_muted(&sink_name, muted);
        crate::commands::profiles::autosave_active(&mixer);
        mixer.channel_defs.clone()
    };
    defs.save().map_err(|e| e.to_string())
}

/// Listen to a channel/mix/mic on the default output (session scoped -
/// not persisted, cleared on restart).
#[tauri::command]
pub fn set_monitor(
    state: State<'_, AppState>,
    sink_name: String,
    enabled: bool,
) -> Result<(), String> {
    // Monitoring is scoped to our own nodes: a channel, a mix bus, or the mic
    // (TD-050) - not any arbitrary session sink.
    {
        let mixer = state.lock_mixer()?;
        let known = sink_name == "sink_mic"
            || mixer.channel_defs.channels.iter().any(|c| c.name == sink_name)
            || mixer.buses.buses.iter().any(|b| b.name == sink_name);
        if !known {
            return Err(format!("unknown monitor target: {sink_name}"));
        }
    }
    state
        .backend
        .set_monitor(&sink_name, enabled)
        .map_err(|e| e.to_string())
}

/// Set or clear a persistent display name for an app, keyed by its stream
/// identity. An empty `alias` reverts to the discovered name.
#[tauri::command]
pub fn rename_app(
    state: State<'_, AppState>,
    match_prop: String,
    match_value: String,
    alias: String,
) -> Result<(), String> {
    let aliases = {
        let mut mixer = state.lock_mixer()?;
        mixer.aliases.set(&match_prop, &match_value, &alias);
        mixer.aliases.clone()
    };
    aliases.save().map_err(|e| e.to_string())
}

/// Set the volume of a single app stream (0-150%).
#[tauri::command]
pub fn set_app_volume(
    state: State<'_, AppState>,
    stream_index: u32,
    volume: u8,
) -> Result<(), String> {
    state
        .backend
        .set_app_volume(stream_index, volume.min(MAX_VOLUME))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::types::AppStream;

    fn stream(index: u32, prop: &str, value: &str) -> AppStream {
        AppStream {
            index,
            serial: u64::from(index) + 1000,
            app_name: value.to_string(),
            match_prop: prop.into(),
            match_value: value.into(),
            alias: None,
            icon_name: None,
            icon_path: None,
            pid: None,
            assigned_sink: None,
            volume_percent: 100,
            muted: false,
            active: true,
        }
    }

    #[test]
    fn siblings_are_the_apps_other_streams() {
        let streams = vec![
            stream(1, "application.name", "Firefox"),
            stream(2, "application.name", "Firefox"),
            stream(3, "application.name", "Spotify"),
            // Same value under a different property is a different identity.
            stream(4, "application.process.binary", "Firefox"),
        ];
        let picked = siblings_of(&streams, &streams[0]);
        let indices: Vec<u32> = picked.iter().map(|s| s.index).collect();
        assert_eq!(indices, vec![2], "only same-identity streams, never itself");

        assert!(siblings_of(&streams, &streams[2]).is_empty());
    }
}
