use tauri::State;

use crate::audio::types::VirtualSink;
use crate::persistence::wireplumber;
use crate::state::AppState;


/// Create a new channel from a label and icon (sink name is generated).
/// The new channel starts at 100%, unmuted, following the default output.
#[tauri::command]
pub fn add_channel(
    state: State<'_, AppState>,
    label: String,
    icon: Option<String>,
) -> Result<(), String> {
    let (def, defs) = {
        let mut mixer = state.lock_mixer()?;
        let def = mixer
            .channel_defs
            .add(&label, icon)
            .map_err(|e| e.to_string())?;
        (def, mixer.channel_defs.clone())
    };

    let prefs = state.lock_mixer()?.prefs.clone();
    if let Err(e) = (|| {
        state
            .backend
            .create_virtual_sink(&def.name, &prefs.decorate(&def.label))?;
        state.backend.set_sink_volume(&def.name, 100)?;
        state.backend.set_sink_mute(&def.name, false)?;
        state.backend.set_channel_output(&def.name, None)
    })() {
        // Roll back so config matches reality: destroy the sink if it got
        // created (idempotent if it didn't), then drop the definition.
        let _ = state.backend.destroy_virtual_sink(&def.name);
        let mut mixer = state.lock_mixer()?;
        let _ = mixer.channel_defs.remove(&def.name);
        return Err(e.to_string());
    }

    defs.save().map_err(|e| e.to_string())?;
    let (buses, names) = {
        let mut mixer = state.lock_mixer()?;
        mixer.channels.push(VirtualSink {
            name: def.name,
            label: def.label,
            icon: def.icon,
            volume_percent: 100,
            muted: false,
            stream_mix: def.stream_mix,
        });
        // The new channel joins the master mix automatically.
        let names = crate::commands::buses::channel_names(&mixer);
        mixer.buses.sync_master(&names);
        crate::commands::profiles::autosave_active(&mixer);
        (mixer.buses.clone(), names)
    };
    // The master and every auto-include mix pick the new channel up.
    for bus in &buses.buses {
        let members = bus.effective_members(&names);
        if members.contains(&names[names.len() - 1]) {
            if let Err(e) = state.backend.set_bus_members(&bus.name, &members) {
                eprintln!("sink: membership for mix {} failed: {e}", bus.name);
            }
        }
    }
    buses.save().map_err(|e| e.to_string())?;
    Ok(())
}

/// Reorder the channel strips (cosmetic — no audio plumbing changes).
#[tauri::command]
pub fn reorder_channels(state: State<'_, AppState>, order: Vec<String>) -> Result<(), String> {
    let defs = {
        let mut mixer = state.lock_mixer()?;
        mixer
            .channel_defs
            .reorder(&order)
            .map_err(|e| e.to_string())?;
        // Keep the live strip list in the same order.
        mixer
            .channels
            .sort_by_key(|c| order.iter().position(|n| n == &c.name).unwrap_or(usize::MAX));
        crate::commands::profiles::autosave_active(&mixer);
        mixer.channel_defs.clone()
    };
    defs.save().map_err(|e| e.to_string())
}

/// Change a channel's strip icon.
#[tauri::command]
pub fn set_channel_icon(
    state: State<'_, AppState>,
    sink_name: String,
    icon: String,
) -> Result<(), String> {
    let icon = if icon.is_empty() { None } else { Some(icon) };
    let defs = {
        let mut mixer = state.lock_mixer()?;
        mixer
            .channel_defs
            .set_icon(&sink_name, icon.clone())
            .map_err(|e| e.to_string())?;
        if let Some(channel) = mixer.channel_mut(&sink_name) {
            channel.icon = icon;
        }
        crate::commands::profiles::autosave_active(&mixer);
        mixer.channel_defs.clone()
    };
    defs.save().map_err(|e| e.to_string())
}

/// Rename a channel's display label (the sink name stays stable, so
/// assignments, outputs and profiles keep working).
#[tauri::command]
pub fn rename_channel(
    state: State<'_, AppState>,
    sink_name: String,
    label: String,
) -> Result<(), String> {
    let defs = {
        let mut mixer = state.lock_mixer()?;
        mixer
            .channel_defs
            .rename(&sink_name, &label)
            .map_err(|e| e.to_string())?;
        if let Some(channel) = mixer.channel_mut(&sink_name) {
            channel.label = label.trim().to_string();
        }
        crate::commands::profiles::autosave_active(&mixer);
        mixer.channel_defs.clone()
    };
    defs.save().map_err(|e| e.to_string())
}

/// Delete a channel: streams on it return to the default sink, its
/// assignments are dropped, and the sink is destroyed.
#[tauri::command]
pub fn remove_channel(state: State<'_, AppState>, sink_name: String) -> Result<(), String> {
    // Validate against the definition set first (also enforces "keep one").
    {
        let mut mixer = state.lock_mixer()?;
        mixer
            .channel_defs
            .remove(&sink_name)
            .map_err(|e| e.to_string())?;
    }

    // Hand the channel's streams back to the default sink before the rug
    // is pulled out from under them.
    if let Ok(streams) = state.backend.list_app_streams() {
        for stream in streams {
            if stream.assigned_sink.as_deref() == Some(sink_name.as_str()) {
                if let Err(e) = state.backend.move_stream_to_sink(stream.index, "") {
                    eprintln!("sink: evacuating {} failed: {e}", stream.app_name);
                }
            }
        }
    }

    state
        .backend
        .destroy_virtual_sink(&sink_name)
        .map_err(|e| e.to_string())?;

    let (defs, assignments, outputs, eq, buses, names) = {
        let mut mixer = state.lock_mixer()?;
        mixer.channels.retain(|c| c.name != sink_name);
        mixer
            .assignments
            .assignments
            .retain(|a| a.sink_name != sink_name);
        mixer.outputs.remove(&sink_name);
        // The backend's DestroySink already tore down the live insert;
        // this drops the persisted config with the channel.
        mixer.eq.remove(&sink_name);
        // Drop the channel from every mix's membership too.
        mixer.buses.remove_channel(&sink_name);
        // Re-evaluate auto-routing with the channel gone.
        mixer.auto_routed.clear();
        crate::commands::profiles::autosave_active(&mixer);
        (
            mixer.channel_defs.clone(),
            mixer.assignments.clone(),
            mixer.outputs.clone(),
            mixer.eq.clone(),
            mixer.buses.clone(),
            crate::commands::buses::channel_names(&mixer),
        )
    };

    for bus in &buses.buses {
        let _ = state
            .backend
            .set_bus_members(&bus.name, &bus.effective_members(&names));
    }

    defs.save().map_err(|e| e.to_string())?;
    assignments.save().map_err(|e| e.to_string())?;
    outputs.save().map_err(|e| e.to_string())?;
    eq.save().map_err(|e| e.to_string())?;
    buses.save().map_err(|e| e.to_string())?;
    wireplumber::write(&assignments).map_err(|e| e.to_string())?;
    Ok(())
}
