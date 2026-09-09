//! The XDG GlobalShortcuts portal: the desktop owns the bindings and shows
//! its own dialog; we declare the shortcuts and act on activations.

use std::sync::Arc;

use ashpd::desktop::global_shortcuts::{
    BindShortcutsOptions, ConfigureShortcutsOptions, GlobalShortcuts, ListShortcutsOptions,
    NewShortcut,
};
use ashpd::desktop::Session;
use futures_util::StreamExt;
use tauri::{AppHandle, Emitter, Manager};

use super::{Action, ShortcutInfo};

pub struct Handle {
    proxy: GlobalShortcuts,
    session: Session<GlobalShortcuts>,
    /// The session's object path, to tell our activations from other apps'.
    path: String,
}

/// Registered explicitly, or the portal names us after whatever launched us.
const APP_ID: &str = "us.echo.Sink";

pub async fn connect() -> Result<Handle, String> {
    match APP_ID.parse() {
        Ok(id) => {
            if let Err(e) = ashpd::register_host_app(id).await {
                eprintln!("sink: portal app registration unavailable: {e}");
            }
        }
        Err(e) => eprintln!("sink: bad portal app id: {e}"),
    }
    let proxy = GlobalShortcuts::new().await.map_err(|e| e.to_string())?;
    let session = proxy
        .create_session(Default::default())
        .await
        .map_err(|e| e.to_string())?;
    // ashpd keeps the path private; the session serialises as its path.
    let path = serde_json::to_value(&session)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .ok_or("session path")?;
    let handle = Handle {
        proxy,
        session,
        path,
    };
    // A dismissed dialog must not cost the session: Settings can bind again.
    if let Err(e) = bind(&handle).await {
        eprintln!("sink: hotkeys not bound yet: {e}");
    }
    Ok(handle)
}

/// Declare our shortcuts; the desktop asks the user the first time.
pub async fn bind(handle: &Handle) -> Result<(), String> {
    let shortcuts: Vec<NewShortcut> = Action::ALL
        .iter()
        .map(|a| NewShortcut::new(a.id(), a.description()).preferred_trigger(a.portal_trigger()))
        .collect();
    handle
        .proxy
        .bind_shortcuts(
            &handle.session,
            &shortcuts,
            None,
            BindShortcutsOptions::default(),
        )
        .await
        .map_err(|e| e.to_string())?
        .response()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Act on activations until the session closes or the signals stop.
pub async fn run(handle: Arc<Handle>, app: &AppHandle) {
    let mut activated = match handle.proxy.receive_activated().await {
        Ok(stream) => stream,
        Err(e) => {
            eprintln!("sink: hotkey signals unavailable: {e}");
            return;
        }
    };
    let mut deactivated = match handle.proxy.receive_deactivated().await {
        Ok(stream) => stream,
        Err(e) => {
            eprintln!("sink: hotkey release signals unavailable: {e}");
            return;
        }
    };
    let mut closed = match handle.session.receive_closed().await {
        Ok(stream) => stream,
        Err(e) => {
            eprintln!("sink: hotkey session watch unavailable: {e}");
            return;
        }
    };
    // Keys edited in the desktop's settings show up in ours without a restart.
    let changes = handle.clone();
    let notify = app.clone();
    let watcher = tauri::async_runtime::spawn(async move {
        let Ok(mut changed) = changes.proxy.receive_shortcuts_changed().await else {
            return;
        };
        while changed.next().await.is_some() {
            let _ = notify.emit("hotkeys-changed", ());
        }
    });
    loop {
        tokio::select! {
            event = activated.next() => {
                let Some(event) = event else { break };
                if event.session_handle().as_str() != handle.path {
                    continue;
                }
                if let Some(action) = Action::from_id(event.shortcut_id()) {
                    if app.state::<super::Hotkeys>().press(action) {
                        super::perform(app, action);
                    }
                }
            }
            event = deactivated.next() => {
                let Some(event) = event else { break };
                if let Some(action) = Action::from_id(event.shortcut_id()) {
                    app.state::<super::Hotkeys>().release(action);
                }
            }
            _ = closed.next() => break,
        }
    }
    watcher.abort();
}

pub async fn shortcuts(handle: &Handle) -> Result<Vec<ShortcutInfo>, String> {
    let listed = handle
        .proxy
        .list_shortcuts(&handle.session, ListShortcutsOptions::default())
        .await
        .map_err(|e| e.to_string())?
        .response()
        .map_err(|e| e.to_string())?;
    Ok(listed
        .shortcuts()
        .iter()
        .map(|s| ShortcutInfo {
            id: s.id().to_string(),
            description: s.description().to_string(),
            trigger: s.trigger_description().to_string(),
        })
        .collect())
}

/// Bind (prompts only for new ids), then the desktop's own shortcut settings.
pub async fn configure(handle: &Handle) -> Result<(), String> {
    bind(handle).await?;
    if handle.proxy.version() < 2 {
        return Err("this desktop can't open shortcut settings from an app yet; use its own shortcut settings".into());
    }
    handle
        .proxy
        .configure_shortcuts(&handle.session, None, ConfigureShortcutsOptions::default())
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}
