//! Global hotkeys for profile switching and the balance slider: the desktop
//! portal under Wayland, direct key grabs on a portal-less X11 session.

pub mod portal;
pub mod x11;

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::persistence::hotkeys::HotkeyConfig;
use crate::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    ProfileNext,
    ProfilePrev,
    BalanceA,
    BalanceB,
    BalanceCenter,
}

impl Action {
    pub const ALL: [Action; 5] = [
        Action::ProfileNext,
        Action::ProfilePrev,
        Action::BalanceA,
        Action::BalanceB,
        Action::BalanceCenter,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Action::ProfileNext => "profile.next",
            Action::ProfilePrev => "profile.prev",
            Action::BalanceA => "balance.a",
            Action::BalanceB => "balance.b",
            Action::BalanceCenter => "balance.center",
        }
    }

    pub fn from_id(id: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.id() == id)
    }

    pub fn description(self) -> &'static str {
        match self {
            Action::ProfileNext => "Next profile",
            Action::ProfilePrev => "Previous profile",
            Action::BalanceA => "Balance toward A",
            Action::BalanceB => "Balance toward B",
            Action::BalanceCenter => "Center the balance",
        }
    }

    /// The portal spells triggers with xkb keysym names.
    pub fn portal_trigger(self) -> &'static str {
        match self {
            Action::ProfileNext => "CTRL+ALT+bracketright",
            Action::ProfilePrev => "CTRL+ALT+bracketleft",
            Action::BalanceA => "CTRL+ALT+comma",
            Action::BalanceB => "CTRL+ALT+period",
            Action::BalanceCenter => "CTRL+ALT+slash",
        }
    }

    /// X11 grabs spell them with key codes.
    pub fn x11_trigger(self) -> &'static str {
        match self {
            Action::ProfileNext => "Ctrl+Alt+BracketRight",
            Action::ProfilePrev => "Ctrl+Alt+BracketLeft",
            Action::BalanceA => "Ctrl+Alt+Comma",
            Action::BalanceB => "Ctrl+Alt+Period",
            Action::BalanceCenter => "Ctrl+Alt+Slash",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ShortcutInfo {
    pub id: String,
    pub description: String,
    /// Human-readable key, empty when the desktop has none bound yet.
    pub trigger: String,
}

#[derive(Debug, Serialize)]
pub struct HotkeyStatus {
    /// "portal", "x11" or "none".
    pub backend: &'static str,
    pub shortcuts: Vec<ShortcutInfo>,
    pub balance_step: u8,
    pub steps: [u8; 4],
}

#[derive(Default)]
pub enum Backend {
    Portal(Arc<portal::Handle>),
    X11(Arc<x11::Handle>),
    #[default]
    None,
}

impl Backend {
    pub fn name(&self) -> &'static str {
        match self {
            Backend::Portal(_) => "portal",
            Backend::X11(_) => "x11",
            Backend::None => "none",
        }
    }
}

/// Managed by Tauri.
#[derive(Default)]
pub struct Hotkeys {
    backend: Mutex<Backend>,
    config: Mutex<HotkeyConfig>,
    /// Held while an action runs, so key repeat can't queue up profile loads.
    running: Mutex<()>,
    /// Keys currently held: a desktop re-sends the activation at the key
    /// repeat rate, and one press must be one action.
    held: Mutex<std::collections::HashSet<Action>>,
}

impl Hotkeys {
    /// True once per press: the first activation until the key is released.
    pub fn press(&self, action: Action) -> bool {
        lock(&self.held).insert(action)
    }

    pub fn release(&self, action: Action) {
        lock(&self.held).remove(&action);
    }
}

impl Hotkeys {
    pub fn backend(&self) -> Backend {
        match &*lock(&self.backend) {
            Backend::Portal(h) => Backend::Portal(h.clone()),
            Backend::X11(h) => Backend::X11(h.clone()),
            Backend::None => Backend::None,
        }
    }

    pub fn config(&self) -> HotkeyConfig {
        lock(&self.config).clone()
    }

    pub fn update_config(&self, change: impl FnOnce(&mut HotkeyConfig)) -> HotkeyConfig {
        let mut config = lock(&self.config);
        change(&mut config);
        config.clone()
    }
}

/// A poisoned lock still holds usable state; a panic elsewhere shouldn't
/// take the hotkeys down with it.
pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Connect the best available backend; never blocks startup. A portal
/// session that ends (portal restart, relogin) is reconnected.
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let config = HotkeyConfig::load();
        app.state::<Hotkeys>()
            .update_config(|c| *c = config.clone());
        let mut first = true;
        loop {
            match portal::connect().await {
                Ok(handle) => {
                    first = false;
                    let handle = Arc::new(handle);
                    set_backend(&app, Backend::Portal(handle.clone()));
                    portal::run(handle, &app).await;
                    set_backend(&app, Backend::None);
                    eprintln!("sink: hotkey portal session ended; reconnecting");
                }
                Err(e) if first => {
                    set_backend(&app, fallback(&config, &app, &e));
                    return;
                }
                Err(e) => eprintln!("sink: hotkey portal reconnect failed: {e}"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });
}

/// Without a portal, X11 can still grab keys; Wayland cannot.
fn fallback(config: &HotkeyConfig, app: &AppHandle, portal_err: &str) -> Backend {
    let env = |k| std::env::var(k).ok().filter(|v| !v.is_empty());
    let x11 = session_is_x11(
        env("XDG_SESSION_TYPE").as_deref(),
        env("DISPLAY").is_some(),
        env("WAYLAND_DISPLAY").is_some(),
    );
    match x11.then(|| x11::connect(config, app.clone())) {
        Some(Ok(handle)) => Backend::X11(Arc::new(handle)),
        Some(Err(e)) => {
            eprintln!("sink: global hotkeys unavailable (portal: {portal_err}; x11: {e})");
            Backend::None
        }
        None => {
            eprintln!("sink: global hotkeys unavailable ({portal_err})");
            Backend::None
        }
    }
}

fn set_backend(app: &AppHandle, backend: Backend) {
    eprintln!("sink: hotkeys via {}", backend.name());
    *lock(&app.state::<Hotkeys>().backend) = backend;
    let _ = app.emit("hotkeys-changed", ());
}

/// Run `action` off the caller's thread; a press that lands while one is
/// still running is dropped rather than queued.
pub fn perform(app: &AppHandle, action: Action) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let hotkeys = app.state::<Hotkeys>();
        let Ok(_running) = hotkeys.running.try_lock() else {
            return;
        };
        let result = match action {
            Action::ProfileNext => switch_profile(&app, 1),
            Action::ProfilePrev => switch_profile(&app, -1),
            Action::BalanceA => nudge_balance(&app, -1),
            Action::BalanceB => nudge_balance(&app, 1),
            Action::BalanceCenter => set_balance(&app, 0.0),
        };
        if let Err(e) = result {
            eprintln!("sink: hotkey {} failed: {e}", action.id());
        }
    });
}

fn switch_profile(app: &AppHandle, direction: i32) -> Result<(), String> {
    let profiles = crate::persistence::profiles::list().map_err(|e| e.to_string())?;
    if profiles.is_empty() {
        return Ok(());
    }
    let state = app.state::<AppState>();
    let active = state.lock_mixer()?.active_profile.clone();
    let name = next_profile(
        &profiles.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        active.as_deref(),
        direction,
    )
    .to_string();
    crate::commands::profiles::load_profile(app.clone(), app.state(), name.clone())?;
    let _ = app.emit("profile-changed", name);
    Ok(())
}

/// The profile after (or before) the active one, wrapping; the first when
/// none is active.
pub fn next_profile<'a>(names: &[&'a str], active: Option<&str>, direction: i32) -> &'a str {
    let n = names.len() as i32;
    let current = active.and_then(|a| names.iter().position(|n| *n == a));
    let index = match current {
        Some(i) => (i as i32 + direction).rem_euclid(n),
        None => 0,
    };
    names[index as usize]
}

/// The slider position for a pair of volumes: + favours B, - favours A.
pub fn balance_position(a: u8, b: u8) -> f32 {
    (f32::from(b) - f32::from(a)) / 100.0
}

/// Volumes for a position. No centre dead zone: a one-point step must move.
pub fn balance_volumes(position: f32) -> (u8, u8) {
    let p = position.clamp(-1.0, 1.0);
    let a = (100.0 * (1.0 - p).min(1.0)).round() as u8;
    let b = (100.0 * (1.0 + p).min(1.0)).round() as u8;
    (a, b)
}

/// A channel name with its current volume.
type Level = (String, u8);

/// The pair the balance slider works on: the preference, else Game and
/// Chat, else the first two channels.
fn balance_pair(state: &AppState) -> Result<Option<(Level, Level)>, String> {
    let mixer = state.lock_mixer()?;
    let channels = &mixer.channel_defs.channels;
    let find = |name: &str| {
        channels
            .iter()
            .find(|c| c.name == name)
            .map(|c| (c.name.clone(), c.volume_percent))
    };
    let nth = |i: usize| channels.get(i).map(|c| (c.name.clone(), c.volume_percent));
    let a = mixer
        .prefs
        .balance_a
        .as_deref()
        .and_then(find)
        .or_else(|| find("sink_game"))
        .or_else(|| nth(0));
    let b = mixer
        .prefs
        .balance_b
        .as_deref()
        .and_then(find)
        .or_else(|| find("sink_chat"))
        .or_else(|| nth(1));
    Ok(a.zip(b).filter(|(a, b)| a.0 != b.0))
}

fn nudge_balance(app: &AppHandle, direction: i32) -> Result<(), String> {
    let step = f32::from(app.state::<Hotkeys>().config().balance_step) / 100.0;
    let state = app.state::<AppState>();
    let Some((a, b)) = balance_pair(&state)? else {
        return Ok(());
    };
    set_balance(app, balance_position(a.1, b.1) + direction as f32 * step)
}

fn set_balance(app: &AppHandle, position: f32) -> Result<(), String> {
    let state = app.state::<AppState>();
    let Some((a, b)) = balance_pair(&state)? else {
        return Ok(());
    };
    let (va, vb) = balance_volumes(position);
    crate::commands::routing::set_channel_volume(app.state(), a.0, va)?;
    crate::commands::routing::set_channel_volume(app.state(), b.0, vb)?;
    let _ = app.emit("channels-changed", ());
    Ok(())
}

/// Launchers don't always pass the session type on; a bare X display is
/// still X11, while XWayland leaves both displays set and keys ungrabbable.
fn session_is_x11(session_type: Option<&str>, display: bool, wayland_display: bool) -> bool {
    match session_type {
        Some("x11") => true,
        Some("wayland") => false,
        _ => display && !wayland_display,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_cycle_in_both_directions() {
        let names = ["Default", "Stream", "Night"];
        assert_eq!(next_profile(&names, Some("Default"), 1), "Stream");
        assert_eq!(next_profile(&names, Some("Night"), 1), "Default");
        assert_eq!(next_profile(&names, Some("Default"), -1), "Night");
        assert_eq!(next_profile(&names, None, 1), "Default");
        assert_eq!(next_profile(&names, Some("gone"), -1), "Default");
    }

    #[test]
    fn balance_maths_matches_the_slider() {
        assert_eq!(balance_volumes(0.0), (100, 100));
        assert_eq!(balance_volumes(0.5), (50, 100));
        assert_eq!(balance_volumes(-0.25), (100, 75));
        assert_eq!(balance_volumes(2.0), (0, 100), "clamped");
        assert_eq!(balance_position(50, 100), 0.5);
        assert_eq!(balance_position(100, 75), -0.25);
    }

    #[test]
    fn a_step_moves_the_position_by_the_step() {
        let (a, b) = balance_volumes(balance_position(100, 100) + 0.10);
        assert_eq!((a, b), (90, 100));
        let (a, b) = balance_volumes(balance_position(90, 100) - 0.25);
        assert_eq!((a, b), (100, 85));
    }

    #[test]
    fn the_smallest_step_still_moves_and_accumulates() {
        let (mut a, mut b) = (100u8, 100u8);
        for _ in 0..3 {
            let next = balance_volumes(balance_position(a, b) + 0.01);
            assert_ne!(next, (a, b));
            (a, b) = next;
        }
        assert_eq!((a, b), (97, 100));
    }

    #[test]
    fn x11_is_detected_from_the_session_or_a_bare_display() {
        assert!(session_is_x11(Some("x11"), true, false));
        assert!(
            session_is_x11(None, true, false),
            "launched without a session type"
        );
        assert!(
            session_is_x11(Some("tty"), true, false),
            "started from a console login"
        );
        assert!(!session_is_x11(Some("wayland"), true, false));
        assert!(
            !session_is_x11(None, true, true),
            "xwayland is not a grab target"
        );
        assert!(!session_is_x11(None, false, false));
    }

    #[test]
    fn a_held_key_is_one_press_until_released() {
        let hotkeys = Hotkeys::default();
        assert!(hotkeys.press(Action::ProfileNext));
        assert!(!hotkeys.press(Action::ProfileNext), "auto-repeat");
        assert!(
            hotkeys.press(Action::BalanceA),
            "another key is independent"
        );
        hotkeys.release(Action::ProfileNext);
        assert!(hotkeys.press(Action::ProfileNext));
    }

    #[test]
    fn every_action_round_trips_its_id() {
        for action in Action::ALL {
            assert_eq!(Action::from_id(action.id()), Some(action));
        }
        assert_eq!(Action::from_id("nope"), None);
    }
}
