//! A recording [`AudioBackend`] for tests. Nothing here is compiled into a
//! release build.
//!
//! Commands only ever reach the audio system through the trait, so a mock is
//! enough to exercise a whole command path - including the order it does
//! things in, which is what the routing races turned on.

use std::sync::Mutex;

use crate::audio::backend::AudioBackend;
use crate::audio::types::{AppStream, EqConfig, MicConfig, OutputDevice};
use crate::error::SinkError;

/// One recorded backend call. Only the calls tests assert on are named;
/// everything else lands as `Other`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    MoveStream { index: u32, sink: String },
    ListStreams,
    Other(&'static str),
}

/// Called on every move, so a test can inspect the world mid-command.
type MoveHook = Box<dyn Fn(u32, &str) + Send + Sync>;

#[derive(Default)]
pub struct MockBackend {
    streams: Mutex<Vec<AppStream>>,
    calls: Mutex<Vec<Call>>,
    on_move: Mutex<Option<MoveHook>>,
}

impl MockBackend {
    pub fn with_streams(streams: Vec<AppStream>) -> Self {
        Self {
            streams: Mutex::new(streams),
            ..Default::default()
        }
    }

    pub fn on_move(&self, f: impl Fn(u32, &str) + Send + Sync + 'static) {
        *self.on_move.lock().expect("on_move") = Some(Box::new(f));
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls.lock().expect("calls").clone()
    }

    pub fn moves(&self) -> Vec<(u32, String)> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::MoveStream { index, sink } => Some((index, sink)),
                _ => None,
            })
            .collect()
    }

    /// Simulate something outside Sink moving a stream (pavucontrol, or the
    /// user dragging it in another mixer).
    pub fn set_assigned(&self, index: u32, sink: Option<&str>) {
        for s in self.streams.lock().expect("streams").iter_mut() {
            if s.index == index {
                s.assigned_sink = sink.map(str::to_string);
            }
        }
    }

    fn record(&self, call: Call) {
        self.calls.lock().expect("calls").push(call);
    }
}

/// Build an `AppStream` with the identity fields a test cares about.
pub fn stream(index: u32, serial: u64, value: &str, on: Option<&str>) -> AppStream {
    AppStream {
        index,
        serial,
        app_name: value.to_string(),
        match_prop: "application.name".into(),
        match_value: value.into(),
        alias: None,
        icon_name: None,
        icon_path: None,
        pid: None,
        assigned_sink: on.map(str::to_string),
        volume_percent: 100,
        muted: false,
        active: true,
    }
}

impl AudioBackend for MockBackend {
    fn list_app_streams(&self) -> Result<Vec<AppStream>, SinkError> {
        self.record(Call::ListStreams);
        Ok(self.streams.lock().expect("streams").clone())
    }

    fn move_stream_to_sink(&self, stream_index: u32, sink_name: &str) -> Result<(), SinkError> {
        self.record(Call::MoveStream {
            index: stream_index,
            sink: sink_name.to_string(),
        });
        if let Some(f) = self.on_move.lock().expect("on_move").as_ref() {
            f(stream_index, sink_name);
        }
        // Reflect the move, so a later listing agrees with what happened.
        for s in self.streams.lock().expect("streams").iter_mut() {
            if s.index == stream_index {
                s.assigned_sink = (!sink_name.is_empty()).then(|| sink_name.to_string());
            }
        }
        Ok(())
    }

    fn create_virtual_sink(&self, _name: &str, _label: &str) -> Result<(), SinkError> {
        self.record(Call::Other("create_virtual_sink"));
        Ok(())
    }

    fn destroy_virtual_sink(&self, _name: &str) -> Result<(), SinkError> {
        self.record(Call::Other("destroy_virtual_sink"));
        Ok(())
    }

    fn list_output_devices(&self) -> Result<Vec<OutputDevice>, SinkError> {
        Ok(Vec::new())
    }

    fn set_sink_volume(&self, _sink_name: &str, _volume_percent: u8) -> Result<(), SinkError> {
        self.record(Call::Other("set_sink_volume"));
        Ok(())
    }

    fn set_sink_mute(&self, _sink_name: &str, _muted: bool) -> Result<(), SinkError> {
        self.record(Call::Other("set_sink_mute"));
        Ok(())
    }

    fn set_app_volume(&self, _stream_index: u32, _volume_percent: u8) -> Result<(), SinkError> {
        self.record(Call::Other("set_app_volume"));
        Ok(())
    }

    fn set_channel_output(&self, _sink_name: &str, _device: Option<&str>) -> Result<(), SinkError> {
        Ok(())
    }

    fn set_channel_eq(&self, _sink_name: &str, _config: &EqConfig) -> Result<(), SinkError> {
        Ok(())
    }

    fn resolved_channel_outputs(
        &self,
    ) -> Result<std::collections::HashMap<String, Option<String>>, SinkError> {
        Ok(std::collections::HashMap::new())
    }

    fn create_bus(&self, _name: &str, _label: &str) -> Result<(), SinkError> {
        Ok(())
    }

    fn destroy_bus(&self, _name: &str) -> Result<(), SinkError> {
        self.record(Call::Other("destroy_bus"));
        Ok(())
    }

    fn set_bus_members(&self, _name: &str, _channels: &[String]) -> Result<(), SinkError> {
        Ok(())
    }

    fn set_bus_mic(&self, _name: &str, _mic: bool) -> Result<(), SinkError> {
        Ok(())
    }

    fn set_bus_member_gain(
        &self,
        _bus_name: &str,
        _member: &str,
        _percent: u8,
    ) -> Result<(), SinkError> {
        Ok(())
    }

    fn set_monitor(&self, _name: &str, _enabled: bool) -> Result<(), SinkError> {
        Ok(())
    }

    fn list_input_devices(&self) -> Result<Vec<OutputDevice>, SinkError> {
        Ok(Vec::new())
    }

    fn get_default_devices(&self) -> Result<(Option<String>, Option<String>), SinkError> {
        Ok((None, None))
    }

    fn set_default_output(&self, _name: &str) -> Result<(), SinkError> {
        Ok(())
    }

    fn set_default_input(&self, _name: &str) -> Result<(), SinkError> {
        Ok(())
    }

    fn set_mic_config(&self, _config: &MicConfig) -> Result<(), SinkError> {
        Ok(())
    }
}
