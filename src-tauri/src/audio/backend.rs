use crate::audio::types::{AppStream, EqConfig, MicConfig, OutputDevice};
use crate::error::SinkError;

/// Abstraction over the audio system (native PipeWire, pactl fallback);
/// commands must only ever talk to this trait, never a concrete backend.
pub trait AudioBackend: Send + Sync {
    /// `label` is the human-readable device description shown by system mixers.
    fn create_virtual_sink(&self, name: &str, label: &str) -> Result<(), SinkError>;
    fn destroy_virtual_sink(&self, name: &str) -> Result<(), SinkError>;
    fn list_app_streams(&self) -> Result<Vec<AppStream>, SinkError>;
    fn list_output_devices(&self) -> Result<Vec<OutputDevice>, SinkError>;
    fn set_sink_volume(&self, sink_name: &str, volume_percent: u8) -> Result<(), SinkError>;
    fn set_sink_mute(&self, sink_name: &str, muted: bool) -> Result<(), SinkError>;
    /// Move an app stream to a sink. An empty `sink_name` means "unassign":
    /// the stream is returned to the system default sink.
    fn move_stream_to_sink(&self, stream_index: u32, sink_name: &str) -> Result<(), SinkError>;
    fn set_app_volume(&self, stream_index: u32, volume_percent: u8) -> Result<(), SinkError>;

    /// Route a channel to a physical output device. `None` means "follow the
    /// system default" (with automatic failover).
    fn set_channel_output(
        &self,
        sink_name: &str,
        output_name: Option<&str>,
    ) -> Result<(), SinkError>;

    /// Turn a channel's auto-failover on or off. Off = stay on the chosen
    /// device (or exact default) and go silent when it's gone, not fall back.
    fn set_channel_failover(&self, _sink_name: &str, _enabled: bool) -> Result<(), SinkError> {
        Ok(())
    }

    /// Apply a channel's parametric EQ (insert/re-tune/remove the biquad
    /// chain). Native-only: the pactl fallback has no in-graph insert point.
    fn set_channel_eq(&self, sink_name: &str, config: &EqConfig) -> Result<(), SinkError>;

    /// Per-channel resolved output after explicit/default/fallback resolution,
    /// so the UI can show what "System default" resolves to.
    fn resolved_channel_outputs(
        &self,
    ) -> Result<std::collections::HashMap<String, Option<String>>, SinkError> {
        Ok(std::collections::HashMap::new())
    }

    /// Create a mix bus: a capturable virtual source whose label is the
    /// device name recorders (OBS) display. Native-only.
    fn create_bus(
        &self,
        name: &str,
        label: &str,
        role: crate::persistence::buses::MixRole,
    ) -> Result<(), SinkError>;

    /// Destroy a mix bus (its links go with it).
    fn destroy_bus(&self, name: &str) -> Result<(), SinkError>;

    /// Replace the set of channels feeding a mix bus.
    fn set_bus_members(&self, name: &str, channels: &[String]) -> Result<(), SinkError>;

    /// Include (or drop) the virtual mic as a mix member. Its own method, not a
    /// `set_bus_members` flag: membership resyncs must never touch it.
    fn set_bus_mic(&self, name: &str, mic: bool) -> Result<(), SinkError>;

    /// Set one member's send level within one mix (0-150%; 100 = unity).
    /// Independent of the member's own volume/EQ - only this mix hears it.
    fn set_bus_member_gain(
        &self,
        bus_name: &str,
        member: &str,
        percent: u8,
    ) -> Result<(), SinkError>;

    /// Monitor a channel/mix/mic on the system default output (session
    /// scoped, an extra passive link set). Native-only.
    fn set_monitor(&self, name: &str, enabled: bool) -> Result<(), SinkError>;

    /// Hardware capture devices (microphones) for the mic chain.
    fn list_input_devices(&self) -> Result<Vec<OutputDevice>, SinkError>;

    /// Current system defaults: (output sink name, input source name).
    fn get_default_devices(&self) -> Result<(Option<String>, Option<String>), SinkError>;

    /// Set the system default output device. Channels following the
    /// default relink automatically.
    fn set_default_output(&self, name: &str) -> Result<(), SinkError>;

    /// Set the system default input device (what the mic chain captures
    /// when no explicit input is chosen).
    fn set_default_input(&self, name: &str) -> Result<(), SinkError>;

    /// Apply the mic chain configuration. Native-backend only; the pactl
    /// fallback reports it as unsupported.
    fn set_mic_config(&self, config: &MicConfig) -> Result<(), SinkError>;
}
