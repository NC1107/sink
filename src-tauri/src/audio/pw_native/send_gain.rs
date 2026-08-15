//! Per-mix send gain: an optional insert on one (member, mix) pair, used
//! only while that member's level inside that specific mix has been moved
//! off unity. Lets a Stream Mix carry the mic louder/quieter than your own
//! headphones hear it, without touching the channel's real volume.
//!
//! Topology: source (channel monitor / mic) ──[loop link]──▶ capture ──gain──▶ ring ──▶ playback ──[loop link]──▶ bus
//!
//! Both ends are unmanaged (no autoconnect, no target): the reconciliation
//! loop in thread.rs links them like any other device/bus target, the same
//! way it drives the EQ insert's playback stream (`eq_chain.rs`). At unity
//! gain no insert exists at all - the loop links the member straight into
//! the bus, same as before this module existed.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use pipewire as pw;
use pw::spa;
use spa::pod::Pod;

use crate::audio::pw_native::ring::Ring;
use crate::error::SinkError;

/// node.name prefixes of the send-gain helper streams (under
/// INTERNAL_PREFIX, so they never show up in app/stream listings).
pub const SEND_CAPTURE_PREFIX: &str = "sink-internal-send-capture-";
pub const SEND_PLAYBACK_PREFIX: &str = "sink-internal-send-playback-";

struct CaptureCtx {
    gain_bits: Arc<AtomicU32>,
    ring: Arc<Ring>,
    scratch: Vec<f32>,
}

struct PlaybackCtx {
    ring: Arc<Ring>,
}

pub struct SendGainHandle {
    capture: pw::stream::StreamRc,
    _capture_listener: pw::stream::StreamListener<CaptureCtx>,
    playback: pw::stream::StreamRc,
    _playback_listener: pw::stream::StreamListener<PlaybackCtx>,
    gain_bits: Arc<AtomicU32>,
}

fn percent_to_gain(percent: u8) -> f32 {
    (f32::from(percent) / 100.0).clamp(0.0, 1.5)
}

impl SendGainHandle {
    /// Node id of the capture stream - the loop links the member's source
    /// into it. `u32::MAX` until the server assigns the node (mirrors
    /// `EqChainHandle::playback_node_id`).
    pub fn capture_node_id(&self) -> u32 {
        self.capture.node_id()
    }

    /// Node id of the playback stream - the loop links this into the bus.
    pub fn playback_node_id(&self) -> u32 {
        self.playback.node_id()
    }

    pub fn set_gain_percent(&self, percent: u8) {
        self.gain_bits
            .store(percent_to_gain(percent).to_bits(), Ordering::Relaxed);
    }
}

/// Stereo F32 format pod for stream negotiation (matches eq_chain/channels -
/// a mono source, like the mic, fans out to both capture ports via the
/// loop's existing mono→stereo pairing).
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
    .map_err(|e| SinkError::Config(format!("send gain format pod: {e:?}")))
}

impl SendGainHandle {
    /// Build both streams. `key` names the (bus, member) pair for the
    /// node.name suffix; neither end has a target - the reconciliation
    /// loop creates the links once the nodes exist.
    pub fn new(core: &pw::core::CoreRc, key: &str, percent: u8) -> Result<Self, SinkError> {
        let err = |stage: &str, e: pw::Error| SinkError::Config(format!("send gain {stage}: {e}"));
        let gain_bits = Arc::new(AtomicU32::new(percent_to_gain(percent).to_bits()));
        // Same headroom as the EQ insert (~85 ms at 48 kHz).
        let ring = Arc::new(Ring::new(8192));

        // ---- capture: member source -> gain -> ring ----
        // Passive like the EQ tap: this insert must not keep an idle
        // channel or the mic running by itself.
        let capture_name = format!("{SEND_CAPTURE_PREFIX}{key}");
        let capture = pw::stream::StreamRc::new(
            core.clone(),
            &capture_name,
            pw::properties::properties! {
                "media.type" => "Audio",
                "media.category" => "Capture",
                "node.name" => capture_name.as_str(),
                "node.passive" => "true",
                "node.autoconnect" => "false",
                "node.dont-reconnect" => "true",
            },
        )
        .map_err(|e| err("capture stream", e))?;

        let capture_listener = capture
            .add_local_listener_with_user_data(CaptureCtx {
                gain_bits: gain_bits.clone(),
                ring: ring.clone(),
                scratch: Vec::with_capacity(8192),
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
                let gain = f32::from_bits(ctx.gain_bits.load(Ordering::Relaxed));
                ctx.scratch.clear();
                ctx.scratch.extend(
                    bytes[..n * 4]
                        .chunks_exact(4)
                        .map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]]) * gain),
                );
                ctx.ring.push(&ctx.scratch);
            })
            .register()
            .map_err(|e| err("capture listener", e))?;

        let format = stereo_f32_format()?;
        let mut capture_params = [Pod::from_bytes(&format)
            .ok_or_else(|| SinkError::Config("send gain capture format pod invalid".into()))?];
        capture
            .connect(
                spa::utils::Direction::Input,
                None,
                pw::stream::StreamFlags::MAP_BUFFERS | pw::stream::StreamFlags::RT_PROCESS,
                &mut capture_params,
            )
            .map_err(|e| err("capture connect", e))?;

        // ---- playback: ring -> bus ----
        let playback_name = format!("{SEND_PLAYBACK_PREFIX}{key}");
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
            .add_local_listener_with_user_data(PlaybackCtx { ring })
            .process(|stream, ctx| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
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
            .ok_or_else(|| SinkError::Config("send gain playback format pod invalid".into()))?];
        playback
            .connect(
                spa::utils::Direction::Output,
                None,
                pw::stream::StreamFlags::MAP_BUFFERS | pw::stream::StreamFlags::RT_PROCESS,
                &mut playback_params,
            )
            .map_err(|e| err("playback connect", e))?;

        Ok(Self {
            capture,
            _capture_listener: capture_listener,
            playback,
            _playback_listener: playback_listener,
            gain_bits,
        })
    }
}
