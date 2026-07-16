//! Native PipeWire backend (Phase 2): replaces pactl subprocess calls with
//! pipewire-rs. All PipeWire objects live on a dedicated loop thread (see
//! `thread.rs`); this facade sends commands over a pipewire channel and
//! blocks on an mpsc reply with a timeout.
//!
//! Extras over the pactl backend: real per-sink level metering (`levels`).

mod dsp;
mod eq;
mod eq_chain;
pub mod levels;
pub mod meter;
mod mic;
mod pods;
mod ring;
mod thread;

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pipewire as pw;

use crate::audio::backend::AudioBackend;
use crate::audio::types::{AppStream, OutputDevice};
use crate::error::SinkError;
use levels::LevelStore;
use thread::Cmd;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

pub struct PipeWireBackend {
    sender: Mutex<pw::channel::Sender<Cmd>>,
    /// Live per-sink peak levels, fed by the meter capture streams.
    pub levels: Arc<LevelStore>,
}

impl PipeWireBackend {
    pub fn new() -> Result<Self, SinkError> {
        let levels = Arc::new(LevelStore::new());
        let (sender, receiver) = pw::channel::channel();
        let (init_tx, init_rx) = mpsc::channel();

        let thread_levels = levels.clone();
        std::thread::Builder::new()
            .name("pipewire-loop".into())
            .spawn(move || thread::run(receiver, init_tx, thread_levels))
            .map_err(|e| SinkError::Config(format!("spawn pipewire thread: {e}")))?;

        match init_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Self {
                sender: Mutex::new(sender),
                levels,
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(SinkError::Config(
                "pipewire loop did not come up within 5s".into(),
            )),
        }
    }

    fn request<T>(
        &self,
        build: impl FnOnce(mpsc::Sender<Result<T, SinkError>>) -> Cmd,
    ) -> Result<T, SinkError> {
        let (tx, rx) = mpsc::channel();
        {
            let sender = self
                .sender
                .lock()
                .map_err(|_| SinkError::Config("pipewire sender lock poisoned".into()))?;
            sender
                .send(build(tx))
                .map_err(|_| SinkError::Config("pipewire loop is gone".into()))?;
        }
        rx.recv_timeout(REQUEST_TIMEOUT)
            .map_err(|_| SinkError::Config("pipewire request timed out".into()))?
    }
}

impl AudioBackend for PipeWireBackend {
    fn create_virtual_sink(&self, name: &str, label: &str) -> Result<(), SinkError> {
        let name = name.to_string();
        let label = label.to_string();
        self.request(|reply| Cmd::CreateSink { name, label, reply })
    }

    fn destroy_virtual_sink(&self, name: &str) -> Result<(), SinkError> {
        let name = name.to_string();
        self.request(|reply| Cmd::DestroySink { name, reply })
    }

    fn list_app_streams(&self) -> Result<Vec<AppStream>, SinkError> {
        self.request(|reply| Cmd::ListStreams { reply })
    }

    fn list_output_devices(&self) -> Result<Vec<OutputDevice>, SinkError> {
        self.request(|reply| Cmd::ListOutputs { reply })
    }

    fn resolved_channel_outputs(
        &self,
    ) -> Result<std::collections::HashMap<String, Option<String>>, SinkError> {
        self.request(|reply| Cmd::ResolvedOutputs { reply })
    }

    fn set_sink_volume(&self, sink_name: &str, volume_percent: u8) -> Result<(), SinkError> {
        let name = sink_name.to_string();
        self.request(|reply| Cmd::SetNodeVolumeByName {
            name,
            percent: volume_percent,
            reply,
        })
    }

    fn set_sink_mute(&self, sink_name: &str, muted: bool) -> Result<(), SinkError> {
        let name = sink_name.to_string();
        self.request(|reply| Cmd::SetNodeMuteByName { name, muted, reply })
    }

    fn move_stream_to_sink(&self, stream_index: u32, sink_name: &str) -> Result<(), SinkError> {
        let sink_name = sink_name.to_string();
        self.request(|reply| Cmd::MoveStream {
            id: stream_index,
            sink_name,
            reply,
        })
    }

    fn set_app_volume(&self, stream_index: u32, volume_percent: u8) -> Result<(), SinkError> {
        self.request(|reply| Cmd::SetNodeVolumeById {
            id: stream_index,
            percent: volume_percent,
            reply,
        })
    }

    fn set_channel_output(
        &self,
        sink_name: &str,
        output_name: Option<&str>,
    ) -> Result<(), SinkError> {
        let sink_name = sink_name.to_string();
        let output_name = output_name.map(str::to_string);
        self.request(|reply| Cmd::SetChannelOutput {
            sink_name,
            output_name,
            reply,
        })
    }

    fn set_channel_failover(&self, sink_name: &str, enabled: bool) -> Result<(), SinkError> {
        let sink_name = sink_name.to_string();
        self.request(|reply| Cmd::SetChannelFailover {
            sink_name,
            enabled,
            reply,
        })
    }

    fn create_bus(&self, name: &str, label: &str) -> Result<(), SinkError> {
        let name = name.to_string();
        let label = label.to_string();
        self.request(|reply| Cmd::CreateBus { name, label, reply })
    }

    fn destroy_bus(&self, name: &str) -> Result<(), SinkError> {
        let name = name.to_string();
        self.request(|reply| Cmd::DestroyBus { name, reply })
    }

    fn set_bus_members(&self, name: &str, channels: &[String]) -> Result<(), SinkError> {
        let name = name.to_string();
        let channels = channels.to_vec();
        self.request(|reply| Cmd::SetBusMembers { name, channels, reply })
    }

    fn set_monitor(&self, name: &str, enabled: bool) -> Result<(), SinkError> {
        let name = name.to_string();
        self.request(|reply| Cmd::SetMonitor { name, enabled, reply })
    }

    fn list_input_devices(&self) -> Result<Vec<crate::audio::types::OutputDevice>, SinkError> {
        self.request(|reply| Cmd::ListInputs { reply })
    }

    fn set_mic_config(&self, config: &crate::audio::types::MicConfig) -> Result<(), SinkError> {
        let config = config.clone();
        self.request(|reply| Cmd::SetMicConfig { config, reply })
    }

    fn set_channel_eq(
        &self,
        sink_name: &str,
        config: &crate::audio::types::EqConfig,
    ) -> Result<(), SinkError> {
        let sink_name = sink_name.to_string();
        let config = config.clone();
        self.request(|reply| Cmd::SetChannelEq { sink_name, config, reply })
    }

    fn get_default_devices(&self) -> Result<(Option<String>, Option<String>), SinkError> {
        self.request(|reply| Cmd::GetDefaults { reply })
    }

    fn set_default_output(&self, name: &str) -> Result<(), SinkError> {
        let name = name.to_string();
        self.request(|reply| Cmd::SetDefault { input: false, name, reply })
    }

    fn set_default_input(&self, name: &str) -> Result<(), SinkError> {
        let name = name.to_string();
        self.request(|reply| Cmd::SetDefault { input: true, name, reply })
    }
}
