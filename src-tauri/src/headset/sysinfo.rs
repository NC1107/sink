//! Lightweight system stats for the OLED "System" dashboard: CPU load (from
//! `/proc/stat` deltas), RAM (`/proc/meminfo`), CPU temperature (hwmon), and a
//! best-effort GPU read (NVIDIA via `nvidia-smi`, AMD via amdgpu sysfs).
//!
//! Deliberately dependency-free — plain `/proc` and `/sys` reads plus one
//! optional subprocess for NVIDIA.

use std::fs;
use std::process::Command;

/// Rolling CPU sampler: percentage needs two `/proc/stat` reads over time.
#[derive(Default)]
pub struct CpuSampler {
    prev_total: u64,
    prev_idle: u64,
}

impl CpuSampler {
    /// Busy percentage since the previous call (0 on the very first call).
    pub fn sample(&mut self) -> u8 {
        let Ok(stat) = fs::read_to_string("/proc/stat") else {
            return 0;
        };
        let Some(line) = stat.lines().next() else {
            return 0;
        };
        // "cpu  user nice system idle iowait irq softirq steal ..."
        let vals: Vec<u64> = line
            .split_whitespace()
            .skip(1)
            .filter_map(|v| v.parse().ok())
            .collect();
        if vals.len() < 5 {
            return 0;
        }
        let idle = vals[3] + vals[4]; // idle + iowait
        let total: u64 = vals.iter().sum();
        let dt = total.saturating_sub(self.prev_total);
        let di = idle.saturating_sub(self.prev_idle);
        self.prev_total = total;
        self.prev_idle = idle;
        if dt == 0 {
            return 0;
        }
        (((dt - di) as f64 / dt as f64) * 100.0).round() as u8
    }
}

/// RAM usage: (used_percent, used_gib, total_gib).
pub fn mem() -> (u8, f32, f32) {
    let Ok(info) = fs::read_to_string("/proc/meminfo") else {
        return (0, 0.0, 0.0);
    };
    let mut total = 0u64;
    let mut avail = 0u64;
    for line in info.lines() {
        if let Some(v) = line.strip_prefix("MemTotal:") {
            total = parse_kb(v);
        } else if let Some(v) = line.strip_prefix("MemAvailable:") {
            avail = parse_kb(v);
        }
    }
    if total == 0 {
        return (0, 0.0, 0.0);
    }
    let used = total.saturating_sub(avail);
    let pct = ((used as f64 / total as f64) * 100.0).round() as u8;
    let gib = |kb: u64| kb as f32 / 1024.0 / 1024.0;
    (pct, gib(used), gib(total))
}

fn parse_kb(s: &str) -> u64 {
    s.split_whitespace()
        .next()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// CPU package temperature in °C, if any hwmon exposes one.
pub fn cpu_temp() -> Option<u8> {
    // Prefer a sensor labelled like a CPU package; fall back to the first.
    let dirs = fs::read_dir("/sys/class/hwmon").ok()?;
    let mut fallback = None;
    for entry in dirs.flatten() {
        let base = entry.path();
        let name = fs::read_to_string(base.join("name")).unwrap_or_default();
        let name = name.trim();
        let is_cpu = matches!(name, "k10temp" | "coretemp" | "zenpower" | "cpu_thermal");
        if let Some(t) = read_temp_input(&base) {
            if is_cpu {
                return Some(t);
            }
            fallback.get_or_insert(t);
        }
    }
    fallback
}

fn read_temp_input(base: &std::path::Path) -> Option<u8> {
    for i in 1..=6 {
        if let Ok(v) = fs::read_to_string(base.join(format!("temp{i}_input"))) {
            if let Ok(milli) = v.trim().parse::<i32>() {
                return Some((milli / 1000).clamp(0, 120) as u8);
            }
        }
    }
    None
}

/// Current media metadata via `playerctl`: (title, artist, playing?).
/// Returns None when playerctl is missing or nothing is playing.
pub fn now_playing() -> Option<(String, String, bool)> {
    let out = Command::new("playerctl")
        .args(["metadata", "--format", "{{title}}\t{{artist}}\t{{status}}"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&out.stdout);
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let mut parts = line.splitn(3, '\t');
    let title = parts.next().unwrap_or("").to_string();
    let artist = parts.next().unwrap_or("").to_string();
    let status = parts.next().unwrap_or("").to_string();
    if title.is_empty() && artist.is_empty() {
        return None;
    }
    Some((title, artist, status.eq_ignore_ascii_case("Playing")))
}

/// A GPU reading (best effort). Any field may be absent depending on vendor.
pub struct GpuStat {
    pub util: u8,
    pub mem_pct: Option<u8>,
    pub temp: Option<u8>,
}

/// Read GPU stats: NVIDIA via `nvidia-smi`, else AMD via amdgpu sysfs.
pub fn gpu() -> Option<GpuStat> {
    nvidia().or_else(amd)
}

fn nvidia() -> Option<GpuStat> {
    let out = Command::new("nvidia-smi")
        .args([
            "--query-gpu=utilization.gpu,memory.used,memory.total,temperature.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&out.stdout);
    let first = line.lines().next()?;
    let f: Vec<&str> = first.split(',').map(|s| s.trim()).collect();
    if f.len() < 4 {
        return None;
    }
    let util = f[0].parse().ok()?;
    let used: f32 = f[1].parse().ok()?;
    let total: f32 = f[2].parse().ok()?;
    let temp = f[3].parse().ok();
    let mem_pct = if total > 0.0 {
        Some((used / total * 100.0).round() as u8)
    } else {
        None
    };
    Some(GpuStat { util, mem_pct, temp })
}

fn amd() -> Option<GpuStat> {
    // Scan DRM cards for an amdgpu-style busy-percent file.
    let dirs = fs::read_dir("/sys/class/drm").ok()?;
    for entry in dirs.flatten() {
        let dev = entry.path().join("device");
        let busy = dev.join("gpu_busy_percent");
        let Ok(util_s) = fs::read_to_string(&busy) else {
            continue;
        };
        let util: u8 = util_s.trim().parse().unwrap_or(0);
        let mem_pct = match (
            fs::read_to_string(dev.join("mem_info_vram_used")),
            fs::read_to_string(dev.join("mem_info_vram_total")),
        ) {
            (Ok(u), Ok(t)) => {
                let u: f64 = u.trim().parse().unwrap_or(0.0);
                let t: f64 = t.trim().parse().unwrap_or(0.0);
                if t > 0.0 {
                    Some((u / t * 100.0).round() as u8)
                } else {
                    None
                }
            }
            _ => None,
        };
        let temp = read_temp_input(&dev.join("hwmon")).or_else(|| amd_hwmon_temp(&dev));
        return Some(GpuStat { util, mem_pct, temp });
    }
    None
}

fn amd_hwmon_temp(dev: &std::path::Path) -> Option<u8> {
    let hwmon = dev.join("hwmon");
    let dirs = fs::read_dir(hwmon).ok()?;
    for entry in dirs.flatten() {
        if let Some(t) = read_temp_input(&entry.path()) {
            return Some(t);
        }
    }
    None
}
