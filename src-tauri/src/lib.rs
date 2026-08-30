mod audio;
mod commands;
mod error;
mod mixer;
mod persistence;
mod state;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tauri::menu::{CheckMenuItem, Menu, MenuItem}; // CheckMenuItem: profile rows
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, WindowEvent};

use audio::backend::AudioBackend;
use audio::pactl::PactlBackend;
use audio::pw_native::levels::LevelStore;
use audio::pw_native::PipeWireBackend;
use state::AppState;

pub fn run() {
    // Prefer the native PipeWire backend (Phase 2); fall back to pactl
    // subprocess calls if the native loop can't come up. Levels (real VU
    // metering) are native-only.
    let (backend, levels): (Arc<dyn AudioBackend>, Option<Arc<LevelStore>>) =
        match PipeWireBackend::new() {
            Ok(backend) => {
                let levels = backend.levels.clone();
                (Arc::new(backend), Some(levels))
            }
            Err(e) => {
                eprintln!("sink: native PipeWire backend unavailable ({e}); using pactl fallback");
                (Arc::new(PactlBackend::new()), None)
            }
        };
    let backend_native = levels.is_some();
    let app_state = AppState::new(backend, backend_native);

    let result = tauri::Builder::default()
        // Single-instance guard (must be the first plugin). A second launch
        // focuses the running window instead of spawning a duplicate - two
        // instances would create clashing virtual sinks and fight over the
        // audio graph. Sink hides to tray, so unhide + focus the existing one.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::devices::get_virtual_devices,
            commands::devices::get_app_streams,
            commands::devices::get_output_devices,
            commands::devices::init_virtual_devices,
            commands::devices::teardown_virtual_devices,
            commands::devices::get_channel_outputs,
            commands::devices::get_resolved_outputs,
            commands::devices::get_channel_failover,
            commands::devices::set_channel_failover,
            commands::devices::set_channel_output,
            commands::apps::get_seen_apps,
            commands::apps::set_app_ignored,
            commands::apps::forget_app,
            commands::apps::set_app_assignment,
            commands::channels::add_channel,
            commands::channels::rename_channel,
            commands::channels::reorder_channels,
            commands::channels::remove_channel,
            commands::channels::set_channel_icon,
            commands::buses::list_buses,
            commands::buses::add_bus,
            commands::buses::rename_bus,
            commands::buses::remove_bus,
            commands::buses::set_bus_members,
            commands::buses::set_bus_mic,
            commands::buses::set_bus_exclude,
            commands::buses::set_bus_volume,
            commands::buses::set_bus_mute,
            commands::buses::set_bus_member_gain,
            commands::buses::open_mix_fader_window,
            commands::routing::route_app_to_channel,
            commands::routing::set_channel_volume,
            commands::routing::toggle_channel_mute,
            commands::routing::set_app_volume,
            commands::routing::rename_app,
            commands::routing::set_monitor,
            commands::mic::get_mic_config,
            commands::mic::set_mic_config,
            commands::mic::get_input_devices,
            commands::eq::get_channel_eq_configs,
            commands::eq::set_channel_eq,
            commands::eq::list_eq_presets,
            commands::eq::save_user_eq_preset,
            commands::eq::delete_user_eq_preset,
            commands::eq::export_channel_eq,
            commands::eq::export_channel_eq_to_file,
            commands::eq::import_eq_config,
            commands::eq::import_eq_file,
            commands::profiles::list_profiles,
            commands::profiles::load_profile,
            commands::profiles::delete_profile,
            commands::profiles::set_profile_trigger,
            commands::profiles::create_blank_profile,
            commands::profiles::get_active_profile,
            commands::settings::get_backend_info,
            commands::settings::get_autostart,
            commands::settings::set_autostart,
            commands::settings::get_default_devices,
            commands::settings::set_default_output,
            commands::settings::set_default_input,
            commands::settings::get_prefs,
            commands::settings::set_device_label_style,
            commands::settings::set_onboarded,
            commands::settings::set_balance_channels,
            commands::settings::set_balance_visible,
            commands::settings::set_start_minimized,
            commands::settings::reset_app,
        ])
        .setup(move |app| {
            build_tray(app)?;
            // The window starts hidden (config) to avoid a flash; show it
            // now unless launched with --minimized (autostart-to-tray).
            let minimized = std::env::args().any(|a| a == "--minimized");
            if !minimized {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                }
            }
            if let Some(levels) = levels {
                spawn_level_emitter(app.handle().clone(), levels);
            }
            spawn_route_enforcer(app.handle().clone());
            Ok(())
        })
        // Close button hides to tray instead of quitting - main window
        // only; a mix's popout just closes.
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Err(e) = window.hide() {
                    eprintln!("sink: failed to hide window: {e}");
                }
            }
        })
        .run(tauri::generate_context!());

    if let Err(e) = result {
        eprintln!("sink: fatal error while running tauri application: {e}");
        std::process::exit(1);
    }
}

/// Worst-case delay before a new stream lands on its channel; the gap is
/// audible (it plays on the default output). Affordable because a native
/// check is one round-trip answered from the loop's node map (~150us).
const ROUTE_ENFORCE_INTERVAL: Duration = Duration::from_millis(200);

/// The pactl fallback forks two processes per check - too dear for 5 Hz.
const ROUTE_ENFORCE_INTERVAL_PACTL: Duration = Duration::from_secs(2);

/// Longer than the UI's 2s poll, so an on-screen window isn't shadowed by
/// the ticker; clock-based so a stalled webview is still covered.
const UI_POLL_GRACE: Duration = Duration::from_secs(5);

/// Enforces saved app→channel assignments from the backend. The UI poll
/// pauses in the tray (TD-009), and enforcement used to pause with it -
/// streams opened meanwhile stayed on the default sink.
fn spawn_route_enforcer(handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        let native = handle.state::<AppState>().backend_native;
        let interval = if native {
            ROUTE_ENFORCE_INTERVAL
        } else {
            ROUTE_ENFORCE_INTERVAL_PACTL
        };
        let mut last_error: Option<String> = None;
        loop {
            std::thread::sleep(interval);
            let state = handle.state::<AppState>();
            if !native && state.ui_polled_within(UI_POLL_GRACE) {
                continue;
            }
            match commands::devices::refresh_streams(state.inner()) {
                Ok(_) => last_error = None,
                // Usually transient; log each message once, not every tick.
                Err(e) => {
                    if last_error.as_deref() != Some(e.as_str()) {
                        eprintln!("sink: auto-route enforcement failed: {e}");
                        last_error = Some(e);
                    }
                }
            }
        }
    });
}

/// Streams per-channel peak levels to the UI at 10 Hz as `levels` events.
/// Peaks are drained (read-and-reset), so silence decays to zero.
fn spawn_level_emitter(handle: tauri::AppHandle, levels: Arc<LevelStore>) {
    std::thread::spawn(move || {
        let mut prev_all_zero = false;
        loop {
            std::thread::sleep(Duration::from_millis(100));
            // The app's dominant state is sitting in the tray during a game.
            // Don't lock the registry, serialize a map and wake the webview
            // for a window nobody can see (TD-008).
            let onscreen = handle
                .get_webview_window("main")
                .map(|w| w.is_visible().unwrap_or(true) && !w.is_minimized().unwrap_or(false))
                .unwrap_or(true);
            if !onscreen {
                // Force a fresh frame when the window returns.
                prev_all_zero = false;
                continue;
            }
            // The meter registry is dynamic (user-defined channels + mic).
            let payload: HashMap<String, [f32; 2]> = levels
                .names()
                .into_iter()
                .map(|(name, slot)| (name, [levels.drain(slot, 0), levels.drain(slot, 1)]))
                .collect();
            // Emit the first all-zero frame so the meters settle to zero, then
            // go quiet until sound returns instead of pushing silence at 10 Hz.
            let all_zero = payload.values().all(|[l, r]| *l < 1e-4 && *r < 1e-4);
            if all_zero && prev_all_zero {
                continue;
            }
            prev_all_zero = all_zero;
            if handle.emit("levels", &payload).is_err() {
                // App is shutting down.
                break;
            }
        }
    });
}

/// Build the tray menu, including the live Profiles submenu (check on the
/// active profile). Rebuilt via `refresh_tray` whenever profiles change.
fn build_tray_menu(
    app: &tauri::AppHandle,
) -> Result<Menu<tauri::Wry>, Box<dyn std::error::Error>> {
    use tauri::menu::{IsMenuItem, Submenu};

    let show = MenuItem::with_id(app, "show", "Show Window", true, None::<&str>)?;

    let active = app
        .state::<AppState>()
        .lock_mixer()
        .ok()
        .and_then(|m| m.active_profile.clone());
    let profile_items: Vec<CheckMenuItem<tauri::Wry>> = persistence::profiles::list()
        .unwrap_or_default()
        .into_iter()
        .map(|info| {
            CheckMenuItem::with_id(
                app,
                format!("profile:{}", info.name),
                &info.name,
                true,
                active.as_deref() == Some(info.name.as_str()),
                None::<&str>,
            )
        })
        .collect::<Result<_, _>>()?;
    let profile_refs: Vec<&dyn IsMenuItem<tauri::Wry>> = profile_items
        .iter()
        .map(|i| i as &dyn IsMenuItem<tauri::Wry>)
        .collect();
    let profiles_menu = Submenu::with_items(app, "Profiles", true, &profile_refs)?;

    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    Ok(Menu::with_items(app, &[&show, &profiles_menu, &quit])?)
}

/// Rebuild the tray menu (called after anything that changes profiles or
/// their active state).
pub(crate) fn refresh_tray(app: &tauri::AppHandle) {
    if let Some(tray) = app.tray_by_id("sink-tray") {
        match build_tray_menu(app) {
            Ok(menu) => {
                if let Err(e) = tray.set_menu(Some(menu)) {
                    eprintln!("sink: tray menu refresh failed: {e}");
                }
            }
            Err(e) => eprintln!("sink: tray menu rebuild failed: {e}"),
        }
    }
}

fn build_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let menu = build_tray_menu(app.handle())?;

    // Dedicated 22px tray glyph from the icon pack (white for the common
    // dark panel; the full-color icon stays on the window/dock).
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-white-22.png"))?;

    TrayIconBuilder::with_id("sink-tray")
        .icon(icon)
        .tooltip("sink")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| {
            let id = event.id.as_ref();
            if let Some(name) = id.strip_prefix("profile:") {
                // Switch profiles straight from the tray; tell the UI.
                match commands::profiles::load_profile(
                    app.clone(),
                    app.state(),
                    name.to_string(),
                ) {
                    Ok(()) => {
                        let _ = app.emit("profile-changed", name);
                    }
                    Err(e) => eprintln!("sink: tray profile switch failed: {e}"),
                }
                return;
            }
            match id {
                "show" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                "quit" => {
                    // Clean up our virtual sinks before exiting. Best-effort:
                    // log failures but never block quitting.
                    let state = app.state::<AppState>();
                    for err in state.teardown_virtual_sinks() {
                        eprintln!("sink: teardown: {err}");
                    }
                    app.exit(0);
                }
                _ => {}
            }
        })
        .build(app)?;

    Ok(())
}
