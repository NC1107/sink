//! X11 fallback: grab the keys ourselves. Bindings are ours to keep, so
//! they live in hotkeys.json.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tauri::AppHandle;

use super::{lock, Action, ShortcutInfo};
use crate::persistence::hotkeys::HotkeyConfig;

pub struct Handle {
    manager: Mutex<GlobalHotKeyManager>,
    /// Live grabs by hotkey id; a key that failed to register isn't here.
    keys: Arc<Mutex<HashMap<u32, Grab>>>,
}

#[derive(Clone)]
struct Grab {
    action: Action,
    hotkey: HotKey,
    trigger: String,
}

pub fn connect(config: &HotkeyConfig, app: AppHandle) -> Result<Handle, String> {
    let manager = GlobalHotKeyManager::new().map_err(|e| e.to_string())?;
    let keys = Arc::new(Mutex::new(HashMap::new()));
    let handle = Handle {
        manager: Mutex::new(manager),
        keys: keys.clone(),
    };
    for action in Action::ALL {
        let trigger = config
            .bindings
            .get(action.id())
            .map(String::as_str)
            .unwrap_or(action.x11_trigger());
        if let Err(e) = handle.bind(action, trigger) {
            eprintln!(
                "sink: hotkey {} ({trigger}) not registered: {e}",
                action.id()
            );
        }
    }
    // Process-lifetime by design: the backend is chosen once per run.
    std::thread::spawn(move || {
        let receiver = GlobalHotKeyEvent::receiver();
        while let Ok(event) = receiver.recv() {
            if event.state() != HotKeyState::Pressed {
                continue;
            }
            let action = lock(&keys).get(&event.id()).map(|g| g.action);
            if let Some(action) = action {
                super::perform(&app, action);
            }
        }
    });
    Ok(handle)
}

impl Handle {
    /// Register `trigger` for `action`, releasing whatever it had once the
    /// new grab holds.
    pub fn bind(&self, action: Action, trigger: &str) -> Result<(), String> {
        let hotkey: HotKey = trigger.parse().map_err(|e| format!("{e}"))?;
        // A bare key would be grabbed from every other app on the desktop.
        if hotkey.mods.is_empty() {
            return Err("a hotkey needs a modifier".into());
        }
        let manager = lock(&self.manager);
        let mut keys = lock(&self.keys);
        if let Some(other) = keys.get(&hotkey.id()).filter(|g| g.action != action) {
            return Err(format!(
                "{trigger} is already {}",
                other.action.description()
            ));
        }
        manager.register(hotkey).map_err(|e| e.to_string())?;
        let previous: Vec<u32> = keys
            .iter()
            .filter(|(id, g)| g.action == action && **id != hotkey.id())
            .map(|(id, _)| *id)
            .collect();
        for id in previous {
            if let Some(old) = keys.remove(&id) {
                let _ = manager.unregister(old.hotkey);
            }
        }
        keys.insert(
            hotkey.id(),
            Grab {
                action,
                hotkey,
                trigger: trigger.to_string(),
            },
        );
        Ok(())
    }

    /// What is really grabbed: a key the desktop refused shows as unbound.
    pub fn shortcuts(&self) -> Vec<ShortcutInfo> {
        let keys = lock(&self.keys);
        Action::ALL
            .iter()
            .map(|a| ShortcutInfo {
                id: a.id().to_string(),
                description: a.description().to_string(),
                trigger: keys
                    .values()
                    .find(|g| g.action == *a)
                    .map(|g| g.trigger.clone())
                    .unwrap_or_default(),
            })
            .collect()
    }
}
