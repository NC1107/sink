use tauri::{AppHandle, Manager};

use crate::hotkeys::{Action, Backend, HotkeyStatus, Hotkeys, ShortcutInfo};
use crate::persistence::hotkeys::BALANCE_STEPS;

#[tauri::command]
pub async fn get_hotkeys(app: AppHandle) -> Result<HotkeyStatus, String> {
    let hotkeys = app.state::<Hotkeys>();
    let config = hotkeys.config();
    let backend = hotkeys.backend();
    let shortcuts: Vec<ShortcutInfo> = match &backend {
        Backend::Portal(handle) => crate::hotkeys::portal::shortcuts(handle).await?,
        Backend::X11(handle) => handle.shortcuts(&config),
        Backend::None => Vec::new(),
    };
    Ok(HotkeyStatus {
        backend: backend.name(),
        shortcuts,
        balance_step: config.balance_step,
        steps: BALANCE_STEPS,
    })
}

/// Portal: the desktop's binding dialog. X11 binds through set_hotkey_binding.
#[tauri::command]
pub async fn configure_hotkeys(app: AppHandle) -> Result<(), String> {
    match app.state::<Hotkeys>().backend() {
        Backend::Portal(handle) => crate::hotkeys::portal::configure(&handle).await,
        Backend::X11(_) => Err("bindings are edited per shortcut on X11".into()),
        Backend::None => Err("global hotkeys are not available on this desktop".into()),
    }
}

#[tauri::command]
pub fn set_hotkey_binding(app: AppHandle, id: String, trigger: String) -> Result<(), String> {
    let action = Action::from_id(&id).ok_or_else(|| format!("unknown hotkey: {id}"))?;
    let hotkeys = app.state::<Hotkeys>();
    let Backend::X11(handle) = hotkeys.backend() else {
        return Err("bindings are edited in the desktop's settings here".into());
    };
    handle.bind(action, &trigger)?;
    let config = {
        let mut config = hotkeys
            .config
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        config.bindings.insert(id, trigger);
        config.clone()
    };
    config.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_balance_step(app: AppHandle, step: u8) -> Result<(), String> {
    if !BALANCE_STEPS.contains(&step) {
        return Err(format!("unsupported balance step: {step}"));
    }
    let hotkeys = app.state::<Hotkeys>();
    let config = {
        let mut config = hotkeys
            .config
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        config.balance_step = step;
        config.clone()
    };
    config.save().map_err(|e| e.to_string())
}
