//! The XDG GlobalShortcuts portal: the desktop owns the bindings and shows
//! its own dialog; we declare the shortcuts and act on activations.

use std::sync::Arc;

use ashpd::desktop::global_shortcuts::{
    BindShortcutsOptions, ConfigureShortcutsOptions, GlobalShortcuts, ListShortcutsOptions,
    NewShortcut,
};
use ashpd::desktop::Session;
use futures_util::StreamExt;
use tauri::AppHandle;

use super::{Action, ShortcutInfo};

pub struct Handle {
    proxy: GlobalShortcuts,
    session: Session<GlobalShortcuts>,
}

/// The id the portal files our shortcuts under. A host app is otherwise
/// named after whatever launched it (a terminal, say); a reverse-DNS id
/// is required, and us.echo.Sink.desktop ships hidden so desktops can
/// turn it into a name.
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
    if proxy.version() == 0 {
        return Err("no GlobalShortcuts portal".into());
    }
    let session = proxy
        .create_session(Default::default())
        .await
        .map_err(|e| e.to_string())?;
    let shortcuts: Vec<NewShortcut> = Action::ALL
        .iter()
        .map(|a| NewShortcut::new(a.id(), a.description()).preferred_trigger(a.portal_trigger()))
        .collect();
    proxy
        .bind_shortcuts(&session, &shortcuts, None, BindShortcutsOptions::default())
        .await
        .map_err(|e| e.to_string())?
        .response()
        .map_err(|e| e.to_string())?;
    Ok(Handle { proxy, session })
}

pub fn listen(handle: Arc<Handle>, app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut activated = match handle.proxy.receive_activated().await {
            Ok(stream) => stream,
            Err(e) => {
                eprintln!("sink: hotkey signals unavailable: {e}");
                return;
            }
        };
        while let Some(event) = activated.next().await {
            if let Some(action) = Action::from_id(event.shortcut_id()) {
                super::perform(&app, action);
            }
        }
    });
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

/// Open the desktop's own binding dialog.
pub async fn configure(handle: &Handle) -> Result<(), String> {
    handle
        .proxy
        .configure_shortcuts(&handle.session, None, ConfigureShortcutsOptions::default())
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}
