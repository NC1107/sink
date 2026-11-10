use tauri::{AppHandle, Emitter, State};

use crate::audio::deepfilter;
use crate::audio::types::{
    is_own_sink, own_input_refusal, MicConfig, NoiseSuppression, OutputDevice,
};
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
    apply_mic_config(state.inner(), &config)
}

pub fn apply_mic_config(state: &AppState, config: &MicConfig) -> Result<(), String> {
    // Capturing one of our own nodes loops the chain into itself (the
    // follow-default path already refuses them - see follow_default_input).
    // The picker lists real inputs; this guards stale configs and invokes.
    if let Some(input) = config.input_device.as_deref().filter(|i| is_own_sink(i)) {
        return Err(own_input_refusal(input));
    }
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
    mic::save(config).map_err(|e| e.to_string())
}

/// Hardware microphones available as the chain's input.
#[tauri::command]
pub fn get_input_devices(state: State<'_, AppState>) -> Result<Vec<OutputDevice>, String> {
    state
        .backend
        .list_input_devices()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_noise_engine(state: State<'_, AppState>) -> Result<deepfilter::EngineStatus, String> {
    let engine = state
        .backend
        .noise_engine_state()
        .map_err(|e| e.to_string())?;
    Ok(deepfilter::status(engine))
}

/// Emits `noise-engine-progress` as `[done, total]` bytes.
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

#[tauri::command]
pub fn retry_noise_engine(state: State<'_, AppState>) -> Result<(), String> {
    state
        .backend
        .retry_noise_engine()
        .map_err(|e| e.to_string())
}

/// Only Sink's own download: a system install belongs to the distro.
#[tauri::command]
pub fn remove_noise_engine() -> Result<(), String> {
    deepfilter::remove_download().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::mock::MockBackend;
    use crate::persistence::testing::TempConfig;
    use std::sync::Arc;

    fn app() -> AppState {
        let backend = Arc::new(MockBackend::with_streams(vec![]));
        AppState::new(backend, true)
    }

    /// Capturing one of our own nodes loops the chain into itself: a pinned
    /// channel monitor, a mix, or the virtual mic must all be refused.
    #[test]
    fn mic_input_rejects_our_own_nodes() {
        let _cfg = TempConfig::new("mic-loop");
        let state = app();

        for target in [
            "sink_game.monitor",
            "sink_stream",
            "sink_bus_voice",
            "sink_mic",
        ] {
            let config = MicConfig {
                input_device: Some(target.to_string()),
                ..MicConfig::default()
            };
            let err = apply_mic_config(&state, &config).expect_err("own node must be refused");
            assert!(
                err.contains("loops the mic chain into itself"),
                "unexpected error for {target}: {err}"
            );
        }
        assert!(
            state
                .lock_mixer()
                .expect("mixer")
                .mic
                .input_device
                .is_none(),
            "a refused pin must not stick"
        );
    }

    #[test]
    fn mic_input_accepts_real_devices_and_follow_default() {
        let _cfg = TempConfig::new("mic-ok");
        let state = app();

        let config = MicConfig {
            input_device: Some("alsa_input.usb-mic".to_string()),
            ..MicConfig::default()
        };
        apply_mic_config(&state, &config).expect("real device");
        assert_eq!(
            state.lock_mixer().expect("mixer").mic.input_device,
            Some("alsa_input.usb-mic".to_string())
        );

        apply_mic_config(&state, &MicConfig::default()).expect("follow default");
        assert!(
            state
                .lock_mixer()
                .expect("mixer")
                .mic
                .input_device
                .is_none(),
            "clearing the pin follows the default again"
        );
    }
}

/// Takes a mode, not a URL, so the frontend can't open arbitrary pages.
#[tauri::command]
pub fn open_noise_engine_page(mode: NoiseSuppression) -> Result<(), String> {
    let url = match mode {
        NoiseSuppression::Off => return Ok(()),
        NoiseSuppression::Light => deepfilter::LIGHT_URL,
        NoiseSuppression::Strong => deepfilter::STRONG_URL,
    };
    super::open_url(url)
}
