//! WebKitGTK's GPU (DMABUF) renderer is opt-in from Settings; off, WebKit
//! stays on its CPU path, which every stack can draw. On NVIDIA + KWin the
//! GPU path failed three ways, each handled here:
//! - native Wayland died at launch with a protocol error unless NVIDIA's
//!   explicit sync is off;
//! - under X11 or XWayland the window stayed blank, so GPU is Wayland-only;
//!
//! It is also only offered with a real GPU: without one (a VM's virtual GPU,
//! a container) WebKit falls back to software GL, which measured ~9x the CPU
//! of the plain CPU path.
//! - a window that crashes can't be toggled back, so a GPU launch that never
//!   finishes starting switches the setting off for the next one.

use std::path::{Path, PathBuf};

use crate::persistence::prefs::Prefs;

const DISABLE_GPU_VAR: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";
/// NVIDIA's driver reads this; Mesa ignores it.
const NVIDIA_EXPLICIT_SYNC_VAR: &str = "__NV_DISABLE_EXPLICIT_SYNC";

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
        // A second launch handing off to a still-starting first one.
        Some(pid) if owner_alive(pid) => Decision::Gpu { arm: false },
        Some(_) => Decision::FallBack,
        None => Decision::Gpu { arm: true },
    }
}

/// GTK's choice of display backend, as it will make it at startup.
fn wayland_session() -> bool {
    match std::env::var("GDK_BACKEND") {
        Ok(backends) if !backends.is_empty() && backends != "*" => backends.starts_with("wayland"),
        _ => std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty()),
    }
}

/// PCI vendors of GPUs with hardware drivers: NVIDIA, AMD, Intel. Virtual
/// GPUs (virtio, VMware, QXL, VirtualBox) are left out on purpose.
const GPU_VENDORS: [&str; 3] = ["0x10de", "0x1002", "0x8086"];
/// ARM boards' GPUs aren't PCI devices, so their DRM driver names them.
const SOC_GPU_DRIVERS: [&str; 7] = [
    "v3d", "panfrost", "panthor", "lima", "msm", "asahi", "etnaviv",
];

/// Whether any DRM render node (what WebKit's GPU path draws through) is
/// backed by a hardware GPU and openable here; `drm` is normally
/// /sys/class/drm and `dev` /dev/dri. Sandboxes often show the host's sysfs
/// without its device files, so the node itself must open.
fn has_hardware_gpu(drm: &Path, dev: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(drm) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with("renderD") {
            return false;
        }
        let openable = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(dev.join(&name))
            .is_ok();
        if !openable {
            return false;
        }
        let device = entry.path().join("device");
        let vendor = std::fs::read_to_string(device.join("vendor")).unwrap_or_default();
        let driver = std::fs::read_link(device.join("driver"))
            .ok()
            .and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        GPU_VENDORS.contains(&vendor.trim()) || SOC_GPU_DRIVERS.contains(&driver.as_str())
    })
}

/// Whether GPU rendering can be offered at all on this machine and session.
pub fn gpu_available() -> bool {
    wayland_session() && has_hardware_gpu(Path::new("/sys/class/drm"), Path::new("/dev/dri"))
}

/// Whether this process is drawing with the GPU renderer.
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

/// Matching the executable, not just the pid, so a reused pid can't pass.
fn is_sink_process(pid: u32) -> bool {
    match (
        std::fs::read_link(format!("/proc/{pid}/exe")),
        std::env::current_exe(),
    ) {
        (Ok(theirs), Ok(ours)) => theirs == ours,
        _ => false,
    }
}

fn arm_probe() {
    let Some(path) = probe_path() else { return };
    if let Err(e) = crate::persistence::write_atomic(&path, std::process::id().to_string()) {
        eprintln!("sink: GPU launch check unavailable: {e}");
    }
}

fn clear_probe() {
    if let Some(path) = probe_path() {
        let _ = std::fs::remove_file(path);
    }
}

/// Called once the webview has started (it invoked the frontend's first
/// command), which is the proof a GPU launch needs.
pub fn confirm_started() {
    if probe_owner() == Some(std::process::id()) {
        clear_probe();
    }
}

/// Pick WebKit's renderer before any webview exists; it can't change later.
pub fn configure() {
    // An explicit choice in the environment always wins.
    if std::env::var_os(DISABLE_GPU_VAR).is_some() {
        return;
    }
    let mut prefs = Prefs::load();
    let owner = probe_owner();
    match decide(prefs.gpu_rendering, gpu_available(), owner, is_sink_process) {
        Decision::Cpu => std::env::set_var(DISABLE_GPU_VAR, "1"),
        Decision::FallBack => {
            eprintln!(
                "sink: the last launch with GPU rendering never started; using the CPU renderer"
            );
            prefs.gpu_rendering = false;
            prefs.gpu_rendering_failed = true;
            if let Err(e) = prefs.save() {
                eprintln!("sink: saving the renderer fallback failed: {e}");
            }
            clear_probe();
            std::env::set_var(DISABLE_GPU_VAR, "1");
        }
        Decision::Gpu { arm } => {
            if arm {
                arm_probe();
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
    fn cpu_unless_opted_in() {
        assert_eq!(decide(false, true, None, GONE), Decision::Cpu);
        assert_eq!(decide(false, true, Some(7), GONE), Decision::Cpu);
    }

    /// Builds a fake /sys/class/drm with one render node, checks it, and
    /// removes it again.
    fn qualifies(tag: &str, vendor: Option<&str>, driver: &str) -> bool {
        node_qualifies(tag, vendor, driver, true)
    }

    fn node_qualifies(tag: &str, vendor: Option<&str>, driver: &str, device_file: bool) -> bool {
        let root = std::env::temp_dir().join(format!("sink-drm-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let device = root.join("devices").join("gpu0");
        std::fs::create_dir_all(&device).unwrap();
        if let Some(v) = vendor {
            std::fs::write(device.join("vendor"), format!("{v}\n")).unwrap();
        }
        let drivers = root.join("drivers").join(driver);
        std::fs::create_dir_all(&drivers).unwrap();
        std::os::unix::fs::symlink(&drivers, device.join("driver")).unwrap();
        let node = root.join("class").join("renderD128");
        std::fs::create_dir_all(&node).unwrap();
        std::os::unix::fs::symlink(&device, node.join("device")).unwrap();
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
        assert!(qualifies("nvidia", Some("0x10de"), "nvidia"));
        assert!(qualifies("amd", Some("0x1002"), "amdgpu"));
        assert!(qualifies("pi", None, "v3d"));
    }

    #[test]
    fn virtual_gpus_and_no_gpu_do_not() {
        // A QEMU virtio GPU, as on the test VMs: software GL in the end.
        assert!(!qualifies("virtio", Some("0x1af4"), "virtio-pci"));
        assert!(!has_hardware_gpu(
            Path::new("/nonexistent/drm"),
            Path::new("/nonexistent/dev")
        ));
    }

    #[test]
    fn a_gpu_whose_device_is_not_reachable_does_not() {
        // A container sees the host's sysfs but not its /dev/dri.
        assert!(!node_qualifies(
            "sandboxed",
            Some("0x10de"),
            "nvidia",
            false
        ));
    }

    #[test]
    fn unsupported_stays_on_cpu() {
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
        arm_probe();
        assert_eq!(probe_owner(), Some(std::process::id()));
        confirm_started();
        assert_eq!(probe_owner(), None);

        crate::persistence::write_atomic(&probe_path().unwrap(), "1").unwrap();
        confirm_started();
        assert_eq!(probe_owner(), Some(1), "another instance's probe stays");
    }
}
