//! Global hotkeys for profile switching and the balance slider. Under
//! Wayland the desktop portal is the only way to see a key while another
//! app is focused; an X11 session without the portal grabs keys directly.

pub mod portal;
pub mod x11;

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::persistence::hotkeys::HotkeyConfig;
use crate::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
            Action::BalanceA => "CTRL+ALT+Left",
            Action::BalanceB => "CTRL+ALT+Right",
            Action::BalanceCenter => "CTRL+ALT+Down",
        }
    }

    /// X11 grabs spell them with key codes.
    pub fn x11_trigger(self) -> &'static str {
        match self {
            Action::ProfileNext => "Ctrl+Alt+BracketRight",
            Action::ProfilePrev => "Ctrl+Alt+BracketLeft",
            Action::BalanceA => "Ctrl+Alt+ArrowLeft",
            Action::BalanceB => "Ctrl+Alt+ArrowRight",
            Action::BalanceCenter => "Ctrl+Alt+ArrowDown",
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

/// Managed by Tauri; the backend is picked once at start.
#[derive(Default)]
pub struct Hotkeys {
    pub backend: Mutex<Backend>,
    pub config: Mutex<HotkeyConfig>,
}

impl Hotkeys {
    pub fn backend(&self) -> Backend {
        match &*self
            .backend
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            Backend::Portal(h) => Backend::Portal(h.clone()),
            Backend::X11(h) => Backend::X11(h.clone()),
            Backend::None => Backend::None,
        }
    }

    pub fn config(&self) -> HotkeyConfig {
        self.config
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// Connect the best available backend; never blocks startup.
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let hotkeys = app.state::<Hotkeys>();
        let config = HotkeyConfig::load();
        *hotkeys
            .config
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = config.clone();
        let backend = match portal::connect().await {
            Ok(handle) => {
                let handle = Arc::new(handle);
                portal::listen(handle.clone(), app.clone());
                Backend::Portal(handle)
            }
            Err(portal_err) => {
                let x11 = std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "x11");
                match x11.then(|| x11::connect(&config, app.clone())) {
                    Some(Ok(handle)) => Backend::X11(Arc::new(handle)),
                    Some(Err(e)) => {
                        eprintln!(
                            "sink: global hotkeys unavailable (portal: {portal_err}; x11: {e})"
                        );
                        Backend::None
                    }
                    None => {
                        eprintln!("sink: global hotkeys unavailable ({portal_err})");
                        Backend::None
                    }
                }
            }
        };
        eprintln!("sink: hotkeys via {}", backend.name());
        *hotkeys
            .backend
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = backend;
    });
}

pub fn perform(app: &AppHandle, action: Action) {
    let result = match action {
        Action::ProfileNext => switch_profile(app, 1),
        Action::ProfilePrev => switch_profile(app, -1),
        Action::BalanceA => nudge_balance(app, -1),
        Action::BalanceB => nudge_balance(app, 1),
        Action::BalanceCenter => set_balance(app, 0.0),
    };
    if let Err(e) = result {
        eprintln!("sink: hotkey {} failed: {e}", action.id());
    }
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

/// Volumes for a position, snapping to centre near the middle like the
/// slider does.
pub fn balance_volumes(position: f32) -> (u8, u8) {
    let p = position.clamp(-1.0, 1.0);
    let p = if p.abs() < 0.04 { 0.0 } else { p };
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
        assert_eq!(balance_volumes(0.03), (100, 100), "snaps to centre");
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
    fn every_action_round_trips_its_id() {
        for action in Action::ALL {
            assert_eq!(Action::from_id(action.id()), Some(action));
        }
        assert_eq!(Action::from_id("nope"), None);
    }
}
