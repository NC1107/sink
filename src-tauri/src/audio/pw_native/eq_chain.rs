//! Per-channel EQ insert: captures a channel sink's monitor (post-fader,
//! the same tap point as the meters), runs the biquad cascade, and plays
//! the processed signal back out through a stream whose ports the loop
//! links to the channel's real targets (device, buses) instead of the raw
//! monitor.
//!
//! Topology:  channel sink ──monitor──▶ capture ──EQ──▶ ring ──▶ playback ──▶ device/buses
//!
//! Same construction as the mic chain (`mic.rs`), minus metering: EQ taps
//! add no LevelStore slots, so the MAX_METERS budget is untouched.

use std::sync::Arc;

use pipewire as pw;
use pw::spa;
use spa::pod::Pod;

use crate::audio::pw_native::eq::{EqEngine, EqParams};
use crate::audio::pw_native::ring::Ring;
use crate::audio::types::EqConfig;
use crate::error::SinkError;

/// node.name prefixes of the EQ helper streams (under INTERNAL_PREFIX, so
/// they never show up in app/stream listings).
pub const EQ_CAPTURE_PREFIX: &str = "sink-internal-eq-capture-";
pub const EQ_PLAYBACK_PREFIX: &str = "sink-internal-eq-playback-";

struct EqCaptureCtx {
    engine: EqEngine,
    params: Arc<EqParams>,
    ring: Arc<Ring>,
    scratch: Vec<f32>,
}

struct EqPlaybackCtx {
    ring: Arc<Ring>,
}

pub struct EqChainHandle {
    _capture: pw::stream::StreamRc,
    _capture_listener: pw::stream::StreamListener<EqCaptureCtx>,
    playback: pw::stream::StreamRc,
    _playback_listener: pw::stream::StreamListener<EqPlaybackCtx>,
    pub params: Arc<EqParams>,
}

impl EqChainHandle {
    /// Node id of the playback stream — the loop links its output ports to
    /// the channel's targets. Only valid once the server has created the
    /// stream's node (callers filter u32::MAX, like `mic_playback_node`).
    pub fn playback_node_id(&self) -> u32 {
        self.playback.node_id()
    }
}

/// Stereo F32 format pod for stream negotiation.
fn stereo_f32_format() -> Result<Vec<u8>, SinkError> {
    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::F32LE);
    info.set_channels(2);
    let object = spa::pod::Object {
        type_: spa::sys::SPA_TYPE_OBJECT_Format,
        id: spa::sys::SPA_PARAM_EnumFormat,
        properties: info.into(),
    };
    spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(object),
    )
    .map(|(c, _)| c.into_inner())
    .map_err(|e| SinkError::Config(format!("eq format pod: {e:?}")))
}

impl EqChainHandle {
    /// Build both streams against a live channel sink node.
    pub fn new(
        core: &pw::core::CoreRc,
        sink_name: &str,
        sink_id: u32,
        config: &EqConfig,
    ) -> Result<Self, SinkError> {
        let err = |stage: &str, e: pw::Error| SinkError::Config(format!("eq {stage}: {e}"));
        let params = Arc::new(EqParams::from_config(config));
        // Interleaved stereo: 8192 samples = the same ~85 ms of headroom at
        // 48 kHz as the mic's 4096 mono; real added latency is one quantum.
        let ring = Arc::new(Ring::new(8192));

        // ---- capture: channel monitor -> EQ -> ring ----
        // Passive like the meters: the channel's own app streams drive the
        // sink; the EQ tap must not keep an idle channel running.
        let capture_name = format!("{EQ_CAPTURE_PREFIX}{sink_name}");
        let capture = pw::stream::StreamRc::new(
            core.clone(),
            &capture_name,
            pw::properties::properties! {
                "media.type" => "Audio",
                "media.category" => "Capture",
                "node.name" => capture_name.as_str(),
                "stream.capture.sink" => "true",
                "node.passive" => "true",
                "node.dont-reconnect" => "true",
            },
        )
        .map_err(|e| err("capture stream", e))?;

        let capture_listener = capture
            .add_local_listener_with_user_data(EqCaptureCtx {
                engine: EqEngine::new(48000.0),
                params: params.clone(),
                ring: ring.clone(),
                scratch: Vec::with_capacity(8192),
            })
            .param_changed(|_, ctx, id, param| {
                // Coefficients are rate-relative: redesign on renegotiation.
                // Filter state resets — same tradeoff the mic chain accepts.
                if id != spa::param::ParamType::Format.as_raw() {
                    return;
                }
                let Some(param) = param else { return };
                let mut info = spa::param::audio::AudioInfoRaw::new();
                if info.parse(param).is_ok() && info.rate() > 0 {
                    ctx.engine.set_sample_rate(info.rate() as f32);
                }
            })
            .process(|stream, ctx| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let datas = buffer.datas_mut();
                let Some(data) = datas.first_mut() else { return };
                let valid = data.chunk().size() as usize;
                let Some(bytes) = data.data() else { return };

                let n = (valid.min(bytes.len())) / 4;
                ctx.scratch.clear();
                ctx.scratch.extend(
                    bytes[..n * 4]
                        .chunks_exact(4)
                        .map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]])),
                );

                ctx.engine.process_interleaved(&mut ctx.scratch, &ctx.params);
                ctx.ring.push(&ctx.scratch);
            })
            .register()
            .map_err(|e| err("capture listener", e))?;

        let format = stereo_f32_format()?;
        let mut capture_params = [Pod::from_bytes(&format)
            .ok_or_else(|| SinkError::Config("eq capture format pod invalid".into()))?];
        capture
            .connect(
                spa::utils::Direction::Input,
                Some(sink_id),
                pw::stream::StreamFlags::AUTOCONNECT
                    | pw::stream::StreamFlags::MAP_BUFFERS
                    | pw::stream::StreamFlags::RT_PROCESS,
                &mut capture_params,
            )
            .map_err(|e| err("capture connect", e))?;

        // ---- playback: ring -> device/buses ----
        // node.autoconnect=false keeps WirePlumber's hands off this stream
        // (it routes playback streams to the default sink — the link police
        // in thread.rs destroys anything that slips through anyway); the
        // loop links it to the channel's resolved targets itself.
        let playback_name = format!("{EQ_PLAYBACK_PREFIX}{sink_name}");
        let playback = pw::stream::StreamRc::new(
            core.clone(),
            &playback_name,
            pw::properties::properties! {
                "media.type" => "Audio",
                "media.category" => "Playback",
                "node.name" => playback_name.as_str(),
                "node.autoconnect" => "false",
                "node.dont-reconnect" => "true",
            },
        )
        .map_err(|e| err("playback stream", e))?;

        let playback_listener = playback
            .add_local_listener_with_user_data(EqPlaybackCtx { ring })
            .process(|stream, ctx| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                // Fill only what the graph asked for this cycle (frames);
                // interleaved stereo = 2 samples, 8 bytes per frame.
                let requested = buffer.requested() as usize;
                let datas = buffer.datas_mut();
                let Some(data) = datas.first_mut() else { return };
                let max_bytes = data.data().map(|d| d.len()).unwrap_or(0);
                let max_frames = max_bytes / 8;
                let frames = if requested > 0 {
                    requested.min(max_frames)
                } else {
                    max_frames.min(1024)
                };
                if frames == 0 {
                    return;
                }
                if let Some(bytes) = data.data() {
                    let mut chunk_samples = [0.0f32; 1024];
                    let total_samples = frames * 2;
                    let mut written = 0;
                    while written < total_samples {
                        let take = (total_samples - written).min(chunk_samples.len());
                        ctx.ring.pop(&mut chunk_samples[..take]);
                        for (i, s) in chunk_samples[..take].iter().enumerate() {
                            let off = (written + i) * 4;
                            bytes[off..off + 4].copy_from_slice(&s.to_ne_bytes());
                        }
                        written += take;
                    }
                }
                let chunk = data.chunk_mut();
                *chunk.offset_mut() = 0;
                *chunk.stride_mut() = 8;
                *chunk.size_mut() = (frames * 8) as u32;
            })
            .register()
            .map_err(|e| err("playback listener", e))?;

        let mut playback_params = [Pod::from_bytes(&format)
            .ok_or_else(|| SinkError::Config("eq playback format pod invalid".into()))?];
        playback
            .connect(
                spa::utils::Direction::Output,
                None,
                // No AUTOCONNECT: the loop creates the links itself.
                pw::stream::StreamFlags::MAP_BUFFERS | pw::stream::StreamFlags::RT_PROCESS,
                &mut playback_params,
            )
            .map_err(|e| err("playback connect", e))?;

        Ok(Self {
            _capture: capture,
            _capture_listener: capture_listener,
            playback,
            _playback_listener: playback_listener,
            params,
        })
    }
}
