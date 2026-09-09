use serde::Serialize;
use tauri::State;

use crate::audio::types::is_virtual_sink;
use crate::state::AppState;


/// A seen-app entry enriched with its current routing, alias and icon.
#[derive(Debug, Clone, Serialize)]
pub struct SeenApp {
    pub match_prop: String,
    pub match_value: String,
    pub display_name: String,
    pub icon_name: Option<String>,
    pub icon_path: Option<String>,
    pub last_seen: u64,
    pub ignored: bool,
    pub assigned_sink: Option<String>,
    pub alias: Option<String>,
}

/// Full app history (live and gone, including ignored entries - the
/// frontend decides what to show where).
#[tauri::command]
pub fn get_seen_apps(state: State<'_, AppState>) -> Result<Vec<SeenApp>, String> {
    let mixer = state.lock_mixer()?;
    Ok(mixer
        .seen
        .apps
        .iter()
        .map(|entry| {
            let binary = (entry.match_prop == "application.process.binary")
                .then_some(entry.match_value.as_str());
            // History entries have no live process - name-based lookup only.
            let resolved = crate::audio::icons::resolve(
                &entry.display_name,
                binary,
                entry.icon_name.as_deref(),
                None,
            );
            SeenApp {
                match_prop: entry.match_prop.clone(),
                match_value: entry.match_value.clone(),
                display_name: resolved
                    .display_name
                    .unwrap_or_else(|| entry.display_name.clone()),
                icon_name: entry.icon_name.clone(),
                icon_path: history_icon(entry, resolved.icon_path),
                last_seen: entry.last_seen,
                ignored: entry.ignored,
                assigned_sink: mixer
                    .assignments
                    .sink_for(&entry.match_prop, &entry.match_value)
                    .map(str::to_string),
                alias: mixer
                    .aliases
                    .get(&entry.match_prop, &entry.match_value)
                    .map(str::to_string),
            }
        })
        .collect())
}

/// The icon stored while the app was live, as long as the file is still
/// there (a removed theme falls back to a fresh lookup).
fn history_icon(
    entry: &crate::persistence::seen::SeenEntry,
    resolved: Option<String>,
) -> Option<String> {
    entry
        .icon_path
        .as_deref()
        .and_then(crate::audio::icons::real_path)
        .or_else(|| {
            crate::audio::icons::identity_icon(&entry.match_prop, &entry.match_value, resolved)
        })
}

/// Hide (or un-hide) an app from the list and from auto-routing.
#[tauri::command]
pub fn set_app_ignored(
    state: State<'_, AppState>,
    match_prop: String,
    match_value: String,
    ignored: bool,
) -> Result<(), String> {
    let seen = {
        let mut mixer = state.lock_mixer()?;
        if !mixer.seen.set_ignored(&match_prop, &match_value, ignored) {
            return Err("unknown app".to_string());
        }
        mixer.seen.clone()
    };
    seen.save().map_err(|e| e.to_string())
}

/// Erase an app from history entirely: sighting, assignment and alias.
#[tauri::command]
pub fn forget_app(
    state: State<'_, AppState>,
    match_prop: String,
    match_value: String,
) -> Result<(), String> {
    let (seen, assignments, aliases) = {
        let mut mixer = state.lock_mixer()?;
        mixer.seen.forget(&match_prop, &match_value);
        mixer.assignments.remove(&match_prop, &match_value);
        mixer.aliases.set(&match_prop, &match_value, "");
        crate::commands::profiles::autosave_active(&mixer);
        (
            mixer.seen.clone(),
            mixer.assignments.clone(),
            mixer.aliases.clone(),
        )
    };
    seen.save().map_err(|e| e.to_string())?;
    assignments.save().map_err(|e| e.to_string())?;
    aliases.save().map_err(|e| e.to_string())?;
    Ok(())
}

/// Edit an app's routing assignment while it isn't running (pre-routing):
/// the app lands on its channel the moment it next plays audio. Empty
/// `sink_name` clears the assignment.
#[tauri::command]
pub fn set_app_assignment(
    state: State<'_, AppState>,
    match_prop: String,
    match_value: String,
    sink_name: String,
) -> Result<(), String> {
    if !sink_name.is_empty() && !is_virtual_sink(&sink_name) {
        return Err(format!("unknown channel: {sink_name}"));
    }
    let assignments = {
        let mut mixer = state.lock_mixer()?;
        if sink_name.is_empty() {
            mixer.assignments.remove(&match_prop, &match_value);
        } else {
            mixer.assignments.set(&match_prop, &match_value, &sink_name);
        }
        crate::commands::profiles::autosave_active(&mixer);
        mixer.assignments.clone()
    };
    assignments.save().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::seen::SeenEntry;

    fn row(icon_path: Option<&str>) -> SeenEntry {
        SeenEntry {
            match_prop: "process.exe".into(),
            match_value: "factorio".into(),
            display_name: "Factorio".into(),
            icon_name: None,
            icon_path: icon_path.map(str::to_string),
            last_seen: 1,
            ignored: false,
        }
    }

    #[test]
    fn history_keeps_its_icon_until_the_file_is_gone() {
        let dir = std::env::temp_dir().join("sink-test-history-icon");
        let _ = std::fs::create_dir_all(&dir);
        let icon = dir.join("factorio.png");
        std::fs::write(&icon, b"png").expect("writes");
        let stored = icon.to_string_lossy().into_owned();
        assert_eq!(
            history_icon(&row(Some(&stored)), Some("/fresh.png".into())),
            Some(stored.clone())
        );
        std::fs::remove_file(&icon).expect("removes");
        assert_eq!(
            history_icon(&row(Some(&stored)), Some("/fresh.png".into())),
            Some("/fresh.png".into())
        );
        assert_eq!(history_icon(&row(None), None), None);
    }
}
