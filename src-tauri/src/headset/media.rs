//! Turn arbitrary media (still images, animated GIFs, video) into a sequence
//! of 1-bit OLED frames.
//!
//! - Still images and GIFs are decoded in-process via the `image` crate.
//! - Video containers are decoded by shelling out to the system `ffmpeg`
//!   (optional; only used when the file looks like a video and ffmpeg exists),
//!   matching Sink's existing "call the system tool" approach (pactl).
//!
//! Every frame is scaled to fit 128x64 (aspect-preserving, centred, black
//! padding) and Floyd–Steinberg dithered to monochrome.

use std::io::BufReader;
use std::path::Path;
use std::process::{Command, Stdio};

use image::{AnimationDecoder, DynamicImage, GenericImageView};

use super::oled::{Framebuffer, HEIGHT, WIDTH};

/// One decoded OLED frame plus how long to show it.
pub struct OledFrame {
    pub fb: Framebuffer,
    pub delay_ms: u16,
}

const VIDEO_EXTS: &[&str] = &["mp4", "mkv", "webm", "avi", "mov", "m4v", "mpg", "mpeg", "flv"];
const VIDEO_FPS: u32 = 24;
/// Cap decoded video length so a huge file can't exhaust memory (~30 s).
const MAX_VIDEO_FRAMES: usize = VIDEO_FPS as usize * 30;

fn extension_lower(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Decode `path` into OLED frames. Chooses image vs. video path by extension,
/// falling back to the image decoder for anything the `image` crate accepts.
pub fn load_media(path: &Path) -> Result<Vec<OledFrame>, String> {
    let ext = extension_lower(path);
    if VIDEO_EXTS.contains(&ext.as_str()) {
        return load_video_frames(path);
    }
    if ext == "gif" {
        // Try animated first; fall back to a still if it's a single-frame GIF.
        match load_gif_frames(path) {
            Ok(frames) if !frames.is_empty() => return Ok(frames),
            _ => {}
        }
    }
    load_still(path)
}

/// A single still image → one frame.
fn load_still(path: &Path) -> Result<Vec<OledFrame>, String> {
    let img = image::open(path).map_err(|e| format!("decode image: {e}"))?;
    let mut fb = Framebuffer::new();
    fb.blit_gray_dithered(&fit_gray(&img));
    Ok(vec![OledFrame { fb, delay_ms: 0 }])
}

/// Animated GIF → one frame per animation frame, preserving per-frame delays.
fn load_gif_frames(path: &Path) -> Result<Vec<OledFrame>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("open gif: {e}"))?;
    let decoder = image::codecs::gif::GifDecoder::new(BufReader::new(file))
        .map_err(|e| format!("gif decoder: {e}"))?;
    let frames = decoder
        .into_frames()
        .collect_frames()
        .map_err(|e| format!("gif frames: {e}"))?;
    let mut out = Vec::with_capacity(frames.len());
    for frame in frames {
        let (num, denom) = frame.delay().numer_denom_ms();
        let delay_ms = if denom == 0 { 100 } else { (num / denom) as u16 };
        let dynimg = DynamicImage::ImageRgba8(frame.into_buffer());
        let mut fb = Framebuffer::new();
        fb.blit_gray_dithered(&fit_gray(&dynimg));
        out.push(OledFrame {
            fb,
            delay_ms: delay_ms.max(20),
        });
    }
    Ok(out)
}

/// Video → frames via ffmpeg, which emits raw 8-bit gray 128x64 frames on
/// stdout (already scaled+padded), one every 1/VIDEO_FPS s.
fn load_video_frames(path: &Path) -> Result<Vec<OledFrame>, String> {
    let vf = format!(
        "fps={fps},scale={w}:{h}:force_original_aspect_ratio=decrease,\
         pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:color=black,format=gray",
        fps = VIDEO_FPS,
        w = WIDTH,
        h = HEIGHT
    );
    let output = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            &path.to_string_lossy(),
            "-vf",
            &vf,
            "-frames:v",
            &MAX_VIDEO_FRAMES.to_string(),
            "-f",
            "rawvideo",
            "-pix_fmt",
            "gray",
            "-",
        ])
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .output()
        .map_err(|e| format!("ffmpeg not available: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "ffmpeg failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let frame_size = WIDTH * HEIGHT;
    if output.stdout.len() < frame_size {
        return Err("ffmpeg produced no frames".into());
    }
    let delay = (1000 / VIDEO_FPS) as u16;
    let out = output
        .stdout
        .chunks_exact(frame_size)
        .map(|chunk| {
            let mut fb = Framebuffer::new();
            fb.blit_gray_dithered(chunk);
            OledFrame { fb, delay_ms: delay }
        })
        .collect();
    Ok(out)
}

/// Resize a decoded image to fit 128x64 (aspect-preserving) and return a
/// centred, black-padded WIDTH*HEIGHT grayscale buffer.
fn fit_gray(img: &DynamicImage) -> Vec<u8> {
    let (iw, ih) = img.dimensions();
    if iw == 0 || ih == 0 {
        return vec![0u8; WIDTH * HEIGHT];
    }
    let scale = (WIDTH as f32 / iw as f32).min(HEIGHT as f32 / ih as f32);
    let nw = ((iw as f32 * scale).round() as u32).clamp(1, WIDTH as u32);
    let nh = ((ih as f32 * scale).round() as u32).clamp(1, HEIGHT as u32);
    let resized = img
        .resize_exact(nw, nh, image::imageops::FilterType::Triangle)
        .to_luma8();
    let ox = (WIDTH - nw as usize) / 2;
    let oy = (HEIGHT - nh as usize) / 2;
    let mut out = vec![0u8; WIDTH * HEIGHT];
    for y in 0..nh as usize {
        for x in 0..nw as usize {
            out[(oy + y) * WIDTH + (ox + x)] = resized.get_pixel(x as u32, y as u32).0[0];
        }
    }
    out
}

/// Whether `ffmpeg` is on PATH (so the UI can gate video features).
pub fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
