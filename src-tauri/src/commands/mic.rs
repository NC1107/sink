use tauri::{AppHandle, Emitter, State};

use crate::audio::deepfilter;
use crate::audio::types::{MicConfig, NoiseSuppression, OutputDevice};
use crate::persistence::mic;
use crate::state::AppState;

#[tauri::command]
pub fn get_mic_config(state: State<'_, AppState>) -> Result<MicConfig, String> {
    let mixer = state.lock_mixer()?;
    Ok(mixer.mic.clone())
}

/// Apply and persist the mic chain configuration. The published label is
/// decorated per the device-naming preference; the stored config stays raw.
#[tauri::command]
pub fn set_mic_config(state: State<'_, AppState>, mut config: MicConfig) -> Result<(), String> {
    config.clamp_ranges();
    let mut applied = config.clone();
    applied.output_label = state.lock_mixer()?.prefs.decorate(&config.output_label);
    state
        .backend
        .set_mic_config(&applied)
        .map_err(|e| e.to_string())?;
    {
        let mut mixer = state.lock_mixer()?;
        mixer.mic = config.clone();
    }
    mic::save(&config).map_err(|e| e.to_string())
}

/// Hardware microphones available as the chain's input.
#[tauri::command]
pub fn get_input_devices(state: State<'_, AppState>) -> Result<Vec<OutputDevice>, String> {
    state
        .backend
        .list_input_devices()
        .map_err(|e| e.to_string())
}

/// Whether Strong noise suppression can run, is installed, and is running.
#[tauri::command]
pub fn get_noise_engine(state: State<'_, AppState>) -> Result<deepfilter::EngineStatus, String> {
    let engine = state
        .backend
        .noise_engine_state()
        .map_err(|e| e.to_string())?;
    Ok(deepfilter::status(engine))
}

/// Download the Strong engine's plugin, emitting `noise-engine-progress`
/// as `[done, total]` bytes.
#[tauri::command]
pub async fn download_noise_engine(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        deepfilter::download(|done, total| {
            let _ = app.emit("noise-engine-progress", (done, total));
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// Start the Strong engine again after it stopped.
#[tauri::command]
pub fn retry_noise_engine(state: State<'_, AppState>) -> Result<(), String> {
    state
        .backend
        .retry_noise_engine()
        .map_err(|e| e.to_string())
}

/// Delete the downloaded plugin (a system install is left alone).
#[tauri::command]
pub fn remove_noise_engine() -> Result<(), String> {
    deepfilter::remove_download().map_err(|e| e.to_string())
}

/// Open the project page of the engine behind a mode in the browser.
#[tauri::command]
pub fn open_noise_engine_page(mode: NoiseSuppression) -> Result<(), String> {
    let url = match mode {
        NoiseSuppression::Off => return Ok(()),
        NoiseSuppression::Light => deepfilter::LIGHT_URL,
        NoiseSuppression::Strong => deepfilter::STRONG_URL,
    };
    std::process::Command::new("xdg-open")
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|mut child| {
            // Reap it off-thread: some desktops' openers block until the
            // browser exits, and an unwaited child lingers as a zombie.
            std::thread::spawn(move || child.wait());
        })
        .map_err(|e| format!("open {url}: {e}"))
}
