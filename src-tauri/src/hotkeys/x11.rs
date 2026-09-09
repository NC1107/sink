//! X11 fallback: grab the keys ourselves. Bindings are ours to keep, so
//! they live in hotkeys.json.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tauri::AppHandle;

use super::{Action, ShortcutInfo};
use crate::persistence::hotkeys::HotkeyConfig;

pub struct Handle {
    manager: Mutex<GlobalHotKeyManager>,
    keys: Arc<Mutex<HashMap<u32, (Action, HotKey)>>>,
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
    std::thread::spawn(move || {
        let receiver = GlobalHotKeyEvent::receiver();
        while let Ok(event) = receiver.recv() {
            if event.state() != HotKeyState::Pressed {
                continue;
            }
            let action = keys
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(&event.id())
                .map(|(a, _)| *a);
            if let Some(action) = action {
                super::perform(&app, action);
            }
        }
    });
    Ok(handle)
}

impl Handle {
    /// Register `trigger` for `action`, releasing whatever it had.
    pub fn bind(&self, action: Action, trigger: &str) -> Result<(), String> {
        let hotkey: HotKey = trigger.parse().map_err(|e| format!("{e}"))?;
        let manager = self
            .manager
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut keys = self
            .keys
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let previous: Vec<u32> = keys
            .iter()
            .filter(|(_, (a, _))| *a == action)
            .map(|(id, _)| *id)
            .collect();
        for id in previous {
            if let Some((_, old)) = keys.remove(&id) {
                let _ = manager.unregister(old);
            }
        }
        manager.register(hotkey).map_err(|e| e.to_string())?;
        keys.insert(hotkey.id(), (action, hotkey));
        Ok(())
    }

    pub fn shortcuts(&self, config: &HotkeyConfig) -> Vec<ShortcutInfo> {
        Action::ALL
            .iter()
            .map(|a| ShortcutInfo {
                id: a.id().to_string(),
                description: a.description().to_string(),
                trigger: config
                    .bindings
                    .get(a.id())
                    .cloned()
                    .unwrap_or_else(|| a.x11_trigger().to_string()),
            })
            .collect()
    }
}
