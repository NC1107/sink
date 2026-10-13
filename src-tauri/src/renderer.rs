//! WebKitGTK's GPU (DMABUF) renderer is opt-in from Settings; off, WebKit
//! stays on its CPU path, which every stack can draw. On NVIDIA + KWin the
//! GPU path failed three ways, each handled here:
//! - native Wayland died at launch with a protocol error unless NVIDIA's
//!   explicit sync is off;
//! - under X11 or XWayland the window stayed blank, so GPU is Wayland-only;
//! - a window that crashes can't be toggled back, so a GPU launch that never
//!   finishes starting switches the setting off for the next one.

use std::path::PathBuf;

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
    wayland: bool,
    probe_owner: Option<u32>,
    owner_alive: impl Fn(u32) -> bool,
) -> Decision {
    if !enabled || !wayland {
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

/// Whether GPU rendering can be offered at all in this session.
pub fn gpu_available() -> bool {
    wayland_session()
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
    match decide(
        prefs.gpu_rendering,
        wayland_session(),
        owner,
        is_sink_process,
    ) {
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

    #[test]
    fn x11_stays_on_cpu() {
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
