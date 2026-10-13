//! WebKitGTK's GPU (DMABUF) renderer is opt-in from Settings. Off, WebKit
//! draws on the CPU, which works everywhere. On NVIDIA + KWin the GPU path
//! failed in ways a user can't fix from inside the app, so it is used only:
//! - on Wayland, since under X11/XWayland the window stayed blank;
//! - with NVIDIA's explicit sync off, without which Wayland exited at launch
//!   with a protocol error;
//! - with a hardware GPU, since without one WebKit falls back to software GL
//!   at about 9x the CPU of the plain CPU path;
//! - until a GPU launch fails to start, which switches the setting off so the
//!   next launch opens on the CPU instead of crashing again.

use std::path::{Path, PathBuf};

use crate::persistence::prefs::Prefs;

const DISABLE_GPU_VAR: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";
/// Read by NVIDIA's driver only; Mesa ignores it.
const NVIDIA_EXPLICIT_SYNC_VAR: &str = "__NV_DISABLE_EXPLICIT_SYNC";
/// NVIDIA, AMD and Intel. Sink only ships for x86_64, and virtual GPUs
/// (virtio, VMware, QXL) are left out on purpose: they end in software GL.
const GPU_VENDORS: [&str; 3] = ["0x10de", "0x1002", "0x8086"];

#[derive(Debug, PartialEq)]
enum Decision {
    Cpu,
    /// `arm` is false when another instance's launch is still in flight.
    Gpu {
        arm: bool,
    },
    /// The last GPU launch never confirmed it started.
    FallBack,
}

fn decide(
    enabled: bool,
    supported: bool,
    probe_owner: Option<u32>,
    owner_alive: impl Fn(u32) -> bool,
) -> Decision {
    if !enabled || !supported {
        return Decision::Cpu;
    }
    match probe_owner {
        // A second launch handing off to a first one that is still starting.
        Some(pid) if owner_alive(pid) => Decision::Gpu { arm: false },
        Some(_) => Decision::FallBack,
        None => Decision::Gpu { arm: true },
    }
}

/// The backend GTK will pick at startup.
fn wayland_session() -> bool {
    match std::env::var("GDK_BACKEND") {
        Ok(backends) if !backends.is_empty() && backends != "*" => backends.starts_with("wayland"),
        _ => std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty()),
    }
}

/// `drm` is normally /sys/class/drm and `dev` /dev/dri. The device node has
/// to open too: sandboxes show the host's sysfs without its device files.
fn has_hardware_gpu(drm: &Path, dev: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(drm) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name();
        let vendor =
            std::fs::read_to_string(entry.path().join("device/vendor")).unwrap_or_default();
        name.to_string_lossy().starts_with("renderD")
            && GPU_VENDORS.contains(&vendor.trim())
            && std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(dev.join(&name))
                .is_ok()
    })
}

/// Whether GPU rendering can be offered on this machine and session.
pub fn gpu_available() -> bool {
    wayland_session() && has_hardware_gpu(Path::new("/sys/class/drm"), Path::new("/dev/dri"))
}

/// Whether this process draws with the GPU renderer.
pub fn gpu_active() -> bool {
    std::env::var_os(DISABLE_GPU_VAR).is_none_or(|v| v == "0")
}

fn probe_path() -> Option<PathBuf> {
    crate::persistence::config_root().map(|root| root.join("sink").join("gpu-probe"))
}

fn probe_owner() -> Option<u32> {
    std::fs::read_to_string(probe_path()?)
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// The executable must match too, so a reused pid can't pass for Sink.
fn is_sink_process(pid: u32) -> bool {
    match (
        std::fs::read_link(format!("/proc/{pid}/exe")),
        std::env::current_exe(),
    ) {
        (Ok(theirs), Ok(ours)) => theirs == ours,
        _ => false,
    }
}

fn arm_probe() -> std::io::Result<()> {
    let path = probe_path().ok_or_else(|| std::io::Error::other("no config directory"))?;
    crate::persistence::write_atomic(&path, std::process::id().to_string())
}

/// Forget an unconfirmed launch; turning the setting back on is a fresh try.
pub fn clear_probe() {
    let Some(path) = probe_path() else { return };
    if let Err(e) = std::fs::remove_file(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            eprintln!("sink: clearing the GPU launch check failed: {e}");
        }
    }
}

/// Called from the frontend's first command, which proves the webview started.
pub fn confirm_started() {
    if probe_owner() == Some(std::process::id()) {
        clear_probe();
    }
}

/// Must run before any webview exists: WebKit reads these once.
pub fn configure() {
    // An explicit choice in the environment always wins.
    if std::env::var_os(DISABLE_GPU_VAR).is_some() {
        return;
    }
    let mut prefs = Prefs::load();
    let decision = decide(
        prefs.gpu_rendering,
        gpu_available(),
        probe_owner(),
        is_sink_process,
    );
    match decision {
        Decision::Cpu => std::env::set_var(DISABLE_GPU_VAR, "1"),
        Decision::FallBack => {
            eprintln!("sink: the last launch with GPU rendering never started; using the CPU");
            prefs.gpu_rendering = false;
            prefs.gpu_rendering_failed = true;
            if let Err(e) = prefs.save() {
                eprintln!("sink: saving the GPU rendering fallback failed: {e}");
            }
            clear_probe();
            std::env::set_var(DISABLE_GPU_VAR, "1");
        }
        Decision::Gpu { arm } => {
            // Without the launch check a crash would repeat on every start.
            if arm {
                if let Err(e) = arm_probe() {
                    eprintln!("sink: GPU launch check unavailable ({e}); using the CPU");
                    std::env::set_var(DISABLE_GPU_VAR, "1");
                    return;
                }
            }
            if std::env::var_os(NVIDIA_EXPLICIT_SYNC_VAR).is_none() {
                std::env::set_var(NVIDIA_EXPLICIT_SYNC_VAR, "1");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALIVE: fn(u32) -> bool = |_| true;
    const GONE: fn(u32) -> bool = |_| false;

    #[test]
    fn cpu_unless_opted_in_and_supported() {
        assert_eq!(decide(false, true, None, GONE), Decision::Cpu);
        assert_eq!(decide(false, true, Some(7), GONE), Decision::Cpu);
        assert_eq!(decide(true, false, None, GONE), Decision::Cpu);
    }

    #[test]
    fn fresh_gpu_launch_arms_the_check() {
        assert_eq!(decide(true, true, None, GONE), Decision::Gpu { arm: true });
    }

    #[test]
    fn unconfirmed_launch_falls_back() {
        assert_eq!(decide(true, true, Some(7), GONE), Decision::FallBack);
    }

    #[test]
    fn second_launch_leaves_a_starting_instance_alone() {
        assert_eq!(
            decide(true, true, Some(7), ALIVE),
            Decision::Gpu { arm: false }
        );
    }

    #[test]
    fn probe_round_trip_clears_only_our_own() {
        let _root = crate::persistence::testing::TempConfig::new("gpu-probe");
        assert_eq!(probe_owner(), None);
        arm_probe().unwrap();
        assert_eq!(probe_owner(), Some(std::process::id()));
        confirm_started();
        assert_eq!(probe_owner(), None);

        crate::persistence::write_atomic(&probe_path().unwrap(), "1").unwrap();
        confirm_started();
        assert_eq!(probe_owner(), Some(1), "another instance's probe stays");
        clear_probe();
        assert_eq!(probe_owner(), None);
    }

    /// A fake /sys/class/drm + /dev/dri with one render node.
    fn gpu_found(tag: &str, vendor: &str, device_file: bool) -> bool {
        let root = std::env::temp_dir().join(format!("sink-drm-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let device = root.join("class/renderD128/device");
        std::fs::create_dir_all(&device).unwrap();
        std::fs::write(device.join("vendor"), format!("{vendor}\n")).unwrap();
        let dev = root.join("dev");
        std::fs::create_dir_all(&dev).unwrap();
        if device_file {
            std::fs::write(dev.join("renderD128"), "").unwrap();
        }
        let found = has_hardware_gpu(&root.join("class"), &dev);
        let _ = std::fs::remove_dir_all(&root);
        found
    }

    #[test]
    fn hardware_gpus_qualify() {
        assert!(gpu_found("nvidia", "0x10de", true));
        assert!(gpu_found("amd", "0x1002", true));
        assert!(gpu_found("intel", "0x8086", true));
    }

    #[test]
    fn virtual_unreachable_or_missing_gpus_do_not() {
        // The test VMs' virtio GPU.
        assert!(!gpu_found("virtio", "0x1af4", true));
        // A container: the host's sysfs, but no /dev/dri.
        assert!(!gpu_found("sandboxed", "0x10de", false));
        assert!(!has_hardware_gpu(
            Path::new("/nonexistent/drm"),
            Path::new("/nonexistent/dev")
        ));
    }
}
