use tauri::State;

use crate::persistence::channels::ChannelDef;
use crate::persistence::profiles::{self, Profile, ProfileInfo};
use crate::persistence::wireplumber;
use crate::state::AppState;


/// Persist the current mixer state into the active profile, if any.
/// Profiles are live-bound: every profile-relevant mutation calls this so
/// switching away and back never loses changes.
pub fn autosave_active(mixer: &crate::mixer::state::MixerState) {
    let Some(name) = &mixer.active_profile else {
        return;
    };
    let profile = Profile {
        name: name.clone(),
        channels: mixer.channels.clone(),
        assignments: mixer.assignments.clone(),
        outputs: mixer.outputs.clone(),
        eq: mixer.eq.clone(),
        // Preserved from the cache rather than re-read from disk each mutation.
        trigger_device: mixer.active_trigger.clone(),
        buses: mixer.buses.clone(),
    };
    if let Err(e) = profiles::save(&profile) {
        eprintln!("sink: autosave of profile {name} failed: {e}");
    }
}

fn set_active(state: &State<'_, AppState>, name: Option<String>) -> Result<(), String> {
    // Refresh the cached trigger from the profile we're binding to (a rare
    // profile switch, not the per-mutation autosave path).
    let trigger = name
        .as_deref()
        .and_then(|n| profiles::load(n).ok())
        .and_then(|p| p.trigger_device);
    let mut mixer = state.lock_mixer()?;
    mixer.active_profile = name.clone();
    mixer.active_trigger = trigger;
    crate::persistence::active::save(name.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_profiles() -> Result<Vec<ProfileInfo>, String> {
    profiles::list().map_err(|e| e.to_string())
}

/// The profile changes are currently autosaving into (restored at launch).
#[tauri::command]
pub fn get_active_profile(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let mixer = state.lock_mixer()?;
    Ok(mixer.active_profile.clone())
}

/// Bind (or clear, with empty string) an output device that auto-loads
/// this profile when it appears.
#[tauri::command]
pub fn set_profile_trigger(
    state: State<'_, AppState>,
    name: String,
    device: String,
) -> Result<(), String> {
    let trigger = if device.is_empty() { None } else { Some(device) };
    profiles::set_trigger(&name, trigger.clone()).map_err(|e| e.to_string())?;
    // Keep the cache in step so a later autosave doesn't overwrite the trigger
    // we just set on the active profile with a stale value.
    let mut mixer = state.lock_mixer()?;
    if mixer.active_profile.as_deref() == Some(name.as_str()) {
        mixer.active_trigger = trigger;
    }
    Ok(())
}

/// Apply a saved profile: reconcile the channel **layout** (create missing
/// channels, remove extras — streams evacuate to the default first), then
/// apply volumes/mutes/outputs, replace the assignment set, and clear the
/// auto-route ledger so the new routing is enforced within the next poll.
#[tauri::command]
pub fn load_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<(), String> {
    let profile = profiles::load(&name).map_err(|e| e.to_string())?;
    if profile.channels.is_empty() {
        return Err(format!("profile {name} has no channels"));
    }

    // ---- layout reconciliation ----
    let current: Vec<ChannelDef> = {
        let mixer = state.lock_mixer()?;
        mixer.channel_defs.channels.clone()
    };
    let prefs = state.lock_mixer()?.prefs.clone();
    for channel in &profile.channels {
        if !current.iter().any(|c| c.name == channel.name) {
            state
                .backend
                .create_virtual_sink(&channel.name, &prefs.decorate(&channel.label))
                .map_err(|e| e.to_string())?;
        }
    }
    for old in &current {
        if !profile.channels.iter().any(|c| c.name == old.name) {
            // Evacuate this channel's streams before destroying it.
            if let Ok(streams) = state.backend.list_app_streams() {
                for stream in streams {
                    if stream.assigned_sink.as_deref() == Some(old.name.as_str()) {
                        let _ = state.backend.move_stream_to_sink(stream.index, "");
                    }
                }
            }
            if let Err(e) = state.backend.destroy_virtual_sink(&old.name) {
                eprintln!("sink: removing {} for profile failed: {e}", old.name);
            }
        }
    }

    // ---- channel state ----
    for channel in &profile.channels {
        state
            .backend
            .set_sink_volume(&channel.name, channel.volume_percent)
            .map_err(|e| e.to_string())?;
        state
            .backend
            .set_sink_mute(&channel.name, channel.muted)
            .map_err(|e| e.to_string())?;
        // Output: profile's choice, or follow-default when unset.
        if let Err(e) = state
            .backend
            .set_channel_output(&channel.name, profile.outputs.get(&channel.name))
        {
            eprintln!("sink: profile output for {} failed: {e}", channel.name);
        }
        if let Err(e) = state
            .backend
            .set_channel_failover(&channel.name, profile.outputs.failover(&channel.name))
        {
            eprintln!("sink: profile failover for {} failed: {e}", channel.name);
        }
        // EQ: non-fatal like output/failover — one channel's insert failing
        // must not abort the whole profile load.
        if let Err(e) = state
            .backend
            .set_channel_eq(&channel.name, &profile.eq.get(&channel.name))
        {
            eprintln!("sink: profile eq for {} failed: {e}", channel.name);
        }
    }

    // ---- mix bus reconciliation ----
    let mut target_buses = profile.buses.clone();
    // The master mix always exists and carries the profile's full channel
    // set (this also upgrades old profiles saved before the master model).
    let names: Vec<String> = profile.channels.iter().map(|c| c.name.clone()).collect();
    target_buses.sync_master(&names);
    let current_buses = {
        let mixer = state.lock_mixer()?;
        mixer.buses.clone()
    };
    for old in &current_buses.buses {
        if target_buses.get(&old.name).is_none() {
            let _ = state.backend.destroy_bus(&old.name);
        }
    }
    for bus in &target_buses.buses {
        if current_buses.get(&bus.name).is_none() {
            if let Err(e) = state.backend.create_bus(&bus.name, &prefs.decorate(&bus.label)) {
                eprintln!("sink: profile mix {} failed: {e}", bus.name);
                continue;
            }
        }
        if let Err(e) = state
            .backend
            .set_bus_members(&bus.name, &bus.effective_members(&names))
        {
            eprintln!("sink: profile members for mix {} failed: {e}", bus.name);
        }
    }

    let (defs, assignments, outputs, eq) = {
        let mut mixer = state.lock_mixer()?;
        mixer.buses = target_buses.clone();
        mixer.channel_defs = crate::persistence::channels::Channels {
            channels: profile
                .channels
                .iter()
                .map(|c| ChannelDef {
                    name: c.name.clone(),
                    label: c.label.clone(),
                    icon: c.icon.clone(),
                    stream_mix: c.stream_mix,
                })
                .collect(),
        };
        mixer.channels = profile.channels.clone();
        mixer.assignments = profile.assignments.clone();
        mixer.outputs = profile.outputs.clone();
        mixer.eq = profile.eq.clone();
        mixer.auto_routed.clear();
        (
            mixer.channel_defs.clone(),
            mixer.assignments.clone(),
            mixer.outputs.clone(),
            mixer.eq.clone(),
        )
    };

    defs.save().map_err(|e| e.to_string())?;
    assignments.save().map_err(|e| e.to_string())?;
    outputs.save().map_err(|e| e.to_string())?;
    eq.save().map_err(|e| e.to_string())?;
    target_buses.save().map_err(|e| e.to_string())?;
    wireplumber::write(&assignments).map_err(|e| e.to_string())?;
    // The loaded profile becomes the live-bound (autosaving) one.
    set_active(&state, Some(name))?;
    crate::refresh_tray(&app);
    Ok(())
}

/// Create a profile with a clean slate: the classic four channels at
/// 100%/unmuted, no assignments, all outputs following the default. It is
/// saved but not applied — load it to start fresh.
#[tauri::command]
pub fn create_blank_profile(app: tauri::AppHandle, name: String) -> Result<(), String> {
    let name = profiles::sanitize_name(&name).map_err(|e| e.to_string())?;
    if profiles::load(&name).is_ok() {
        return Err(format!("profile \"{name}\" already exists"));
    }
    let channels = crate::persistence::channels::Channels::default()
        .channels
        .into_iter()
        .map(|def| crate::audio::types::VirtualSink {
            name: def.name,
            label: def.label,
            icon: def.icon,
            volume_percent: 100,
            muted: false,
            stream_mix: def.stream_mix,
        })
        .collect();
    let profile = Profile {
        name,
        channels,
        assignments: Default::default(),
        outputs: Default::default(),
        eq: Default::default(),
        trigger_device: None,
        buses: Default::default(),
    };
    profiles::save(&profile).map_err(|e| e.to_string())?;
    crate::refresh_tray(&app);
    Ok(())
}

#[tauri::command]
pub fn delete_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<(), String> {
    profiles::delete(&name).map_err(|e| e.to_string())?;
    let is_active = {
        let mixer = state.lock_mixer()?;
        mixer.active_profile.as_deref() == Some(name.as_str())
    };
    if is_active {
        set_active(&state, None)?;
    }
    crate::refresh_tray(&app);
    Ok(())
}
