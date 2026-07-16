use crate::audio::types::{AppStream, EqConfig, MicConfig, OutputDevice};
use crate::error::SinkError;

/// Abstraction over the underlying audio system.
///
/// `PipeWireBackend` (native, pipewire-rs) is the default; `PactlBackend`
/// (pactl subprocess calls) is the automatic fallback. Commands must only
/// ever talk to this trait — never to a concrete backend.
pub trait AudioBackend: Send + Sync {
    /// `label` is the human-readable device description shown by system
    /// mixers (channels are user-defined since the dynamic-channels work).
    fn create_virtual_sink(&self, name: &str, label: &str) -> Result<(), SinkError>;
    fn destroy_virtual_sink(&self, name: &str) -> Result<(), SinkError>;
    fn list_app_streams(&self) -> Result<Vec<AppStream>, SinkError>;
    fn list_output_devices(&self) -> Result<Vec<OutputDevice>, SinkError>;
    fn set_sink_volume(&self, sink_name: &str, volume_percent: u8) -> Result<(), SinkError>;
    fn set_sink_mute(&self, sink_name: &str, muted: bool) -> Result<(), SinkError>;
    /// Move an app stream to a sink. An empty `sink_name` means "unassign":
    /// the stream is returned to the system default sink.
    fn move_stream_to_sink(&self, stream_index: u32, sink_name: &str) -> Result<(), SinkError>;
    /// Set the volume of a single app stream (sink input).
    /// Not in the original trait sketch, but required by the `set_app_volume`
    /// command — commands are forbidden from calling pactl directly.
    fn set_app_volume(&self, stream_index: u32, volume_percent: u8) -> Result<(), SinkError>;

    /// Route a channel's audio to a physical output device (Phase 4).
    /// `None` means "follow the system default output" (which also gives
    /// automatic failover when the device disappears). The native backend
    /// creates passive in-graph links; the pactl fallback uses
    /// module-loopback.
    fn set_channel_output(
        &self,
        sink_name: &str,
        output_name: Option<&str>,
    ) -> Result<(), SinkError>;

    /// Turn a channel's auto-failover on or off. When off, the channel routes
    /// only to its chosen device (or the exact system default) and stays
    /// silent when that's gone, instead of falling back to another sink.
    /// Backends without in-graph link control (pactl) ignore this.
    fn set_channel_failover(&self, _sink_name: &str, _enabled: bool) -> Result<(), SinkError> {
        Ok(())
    }

    /// Apply a channel's parametric EQ (insert/re-tune/remove the biquad
    /// chain in the channel's output path). Native-only: the pactl fallback
    /// has no in-graph insert point, mirroring `set_mic_config`.
    fn set_channel_eq(&self, sink_name: &str, config: &EqConfig) -> Result<(), SinkError>;

    /// Per-channel resolved output: the `node.name` of the device each channel
    /// is actually routed to right now, after explicit/default/fallback
    /// resolution (`None` = not currently routed anywhere). Lets the UI show
    /// what "System default" resolves to and makes failover visible. Backends
    /// that can't report this (pactl) return an empty map.
    fn resolved_channel_outputs(
        &self,
    ) -> Result<std::collections::HashMap<String, Option<String>>, SinkError> {
        Ok(std::collections::HashMap::new())
    }

    /// Create a mix bus: a capturable virtual source whose label is the
    /// device name recorders (OBS) display. Native-only.
    fn create_bus(&self, name: &str, label: &str) -> Result<(), SinkError>;

    /// Destroy a mix bus (its links go with it).
    fn destroy_bus(&self, name: &str) -> Result<(), SinkError>;

    /// Replace the set of channels feeding a mix bus.
    fn set_bus_members(&self, name: &str, channels: &[String]) -> Result<(), SinkError>;

    /// Monitor a channel/mix/mic on the system default output (session
    /// scoped, an extra passive link set). Native-only.
    fn set_monitor(&self, name: &str, enabled: bool) -> Result<(), SinkError>;

    /// Hardware capture devices (microphones) for the Phase 3 mic chain.
    fn list_input_devices(&self) -> Result<Vec<OutputDevice>, SinkError>;

    /// Current system defaults: (output sink name, input source name).
    fn get_default_devices(&self) -> Result<(Option<String>, Option<String>), SinkError>;

    /// Set the system default output device. Channels following the
    /// default relink automatically.
    fn set_default_output(&self, name: &str) -> Result<(), SinkError>;

    /// Set the system default input device (what the mic chain captures
    /// when no explicit input is chosen).
    fn set_default_input(&self, name: &str) -> Result<(), SinkError>;

    /// Apply the Phase 3 mic chain configuration. Native-backend only; the
    /// pactl fallback reports it as unsupported.
    fn set_mic_config(&self, config: &MicConfig) -> Result<(), SinkError>;
}
