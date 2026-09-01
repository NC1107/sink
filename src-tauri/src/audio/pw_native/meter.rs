//! Per-sink level metering: a small capture stream on each virtual sink's
//! monitor computes per-channel peaks in the process callback and raises
//! them into the shared `LevelStore`.

use std::io::Cursor;
use std::sync::Arc;

use pipewire as pw;
use pw::spa;
use spa::pod::serialize::PodSerializer;
use spa::pod::{Object, Pod, Value};

use crate::audio::pw_native::levels::LevelStore;
use crate::audio::pw_native::thread::METER_PREFIX;
use crate::error::SinkError;

pub struct MeterHandle {
    _stream: pw::stream::StreamRc,
    _listener: pw::stream::StreamListener<MeterCtx>,
}

struct MeterCtx {
    slot: usize,
    levels: Arc<LevelStore>,
}

impl MeterHandle {
    /// `capture_sink` = true to tap a sink's monitor; false to capture a
    /// source node directly (e.g. the Stream Mix virtual source).
    pub fn new(
        core: &pw::core::CoreRc,
        sink_name: &str,
        sink_id: u32,
        levels: Arc<LevelStore>,
        capture_sink: bool,
    ) -> Result<Self, SinkError> {
        let slot = levels
            .slot_for(sink_name)
            .ok_or_else(|| SinkError::Config(format!("meter budget exhausted for {sink_name}")))?;

        let err = |stage: &str, e: pw::Error| SinkError::Config(format!("meter {stage}: {e}"));

        let props = pw::properties::properties! {
            "media.type" => "Audio",
            "media.category" => "Capture",
            "node.name" => format!("{METER_PREFIX}{sink_name}"),
            // For sinks: capture the monitor, don't keep the sink busy.
            "stream.capture.sink" => if capture_sink { "true" } else { "false" },
            // Pin the meter to the channel it represents. Without an
            // explicit target WirePlumber may move the first-created meter
            // to a newly selected default device, making (for example) Game
            // display the combined speaker output instead of Game itself.
            "target.object" => sink_name,
            "node.dont-reconnect" => "true",
            "node.passive" => "true",
        };
        let stream = pw::stream::StreamRc::new(core.clone(), "sink-meter", props)
            .map_err(|e| err("stream", e))?;

        let listener = stream
            .add_local_listener_with_user_data(MeterCtx { slot, levels })
            .process(|stream, ctx| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let datas = buffer.datas_mut();
                let Some(data) = datas.first_mut() else {
                    return;
                };
                let valid = data.chunk().size() as usize;
                let Some(bytes) = data.data() else { return };
                let mut peaks = [0.0f32; 2];
                // f32 interleaved stereo (negotiated below).
                for (i, raw) in bytes[..valid.min(bytes.len())].chunks_exact(4).enumerate() {
                    let v = f32::from_ne_bytes([raw[0], raw[1], raw[2], raw[3]]).abs();
                    let ch = i & 1;
                    if v > peaks[ch] {
                        peaks[ch] = v;
                    }
                }
                ctx.levels.raise(ctx.slot, 0, peaks[0]);
                ctx.levels.raise(ctx.slot, 1, peaks[1]);
            })
            .register()
            .map_err(|e| err("listener", e))?;

        // Negotiate float32 stereo so the peak math above holds.
        let mut info = spa::param::audio::AudioInfoRaw::new();
        info.set_format(spa::param::audio::AudioFormat::F32LE);
        info.set_channels(2);
        let object = Object {
            type_: spa::sys::SPA_TYPE_OBJECT_Format,
            id: spa::sys::SPA_PARAM_EnumFormat,
            properties: info.into(),
        };
        let bytes = PodSerializer::serialize(Cursor::new(Vec::new()), &Value::Object(object))
            .map_err(|e| SinkError::Config(format!("meter format pod: {e:?}")))?
            .0
            .into_inner();
        let mut params = [Pod::from_bytes(&bytes)
            .ok_or_else(|| SinkError::Config("meter format pod invalid".into()))?];

        stream
            .connect(
                spa::utils::Direction::Input,
                Some(sink_id),
                pw::stream::StreamFlags::AUTOCONNECT
                    | pw::stream::StreamFlags::DONT_RECONNECT
                    | pw::stream::StreamFlags::MAP_BUFFERS,
                &mut params,
            )
            .map_err(|e| err("connect", e))?;

        Ok(Self {
            _stream: stream,
            _listener: listener,
        })
    }
}
