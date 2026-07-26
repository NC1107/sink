//! Direct Linux `hidraw` access for the base station — no libhidapi/libusb.
//!
//! Control and status packets are ordinary HID output/input reports (plain
//! `write`/`read`). The OLED framebuffer is a **Feature** report, which cannot
//! go through `write` — it needs the `HIDIOCSFEATURE` ioctl.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

use super::protocol::{COMMAND_LEN, PRODUCT_IDS, VENDOR_ID};
use super::protocol_apw;

/// Which SteelSeries base-station family a node belongs to. They speak
/// incompatible protocols, so everything downstream branches on this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    /// Arctis Nova Pro Wireless — report id 0x06, 64-byte reports, OLED.
    NovaPro,
    /// Arctis Pro Wireless (pre-Nova) — 0xAA-suffixed, 31-byte reports.
    ArctisPro,
}

impl DeviceKind {
    /// Padded length of a control/output report for this family.
    pub fn command_len(self) -> usize {
        match self {
            DeviceKind::NovaPro => COMMAND_LEN,
            DeviceKind::ArctisPro => protocol_apw::COMMAND_LEN,
        }
    }

    /// Whether this family drives an OLED panel we know how to talk to.
    pub fn has_oled(self) -> bool {
        matches!(self, DeviceKind::NovaPro)
    }
}

/// `HIDIOCSFEATURE(len)` = `_IOC(_IOC_WRITE | _IOC_READ, 'H', 0x06, len)`.
/// Encoded per the asm-generic ioctl layout (dir<<30 | size<<16 | type<<8 | nr).
fn hidiocsfeature(len: usize) -> libc::c_ulong {
    const DIR_WRITE_READ: libc::c_ulong = 3;
    ((DIR_WRITE_READ << 30)
        | ((len as libc::c_ulong) << 16)
        | ((b'H' as libc::c_ulong) << 8)
        | 0x06) as libc::c_ulong
}

/// A resolved base-station control node plus how the kernel identified it.
#[derive(Clone)]
pub struct DevicePath {
    pub dev: PathBuf,
    pub product_id: u16,
    pub kind: DeviceKind,
}

/// Locate a supported base station's control node.
///
/// Nova Pro exposes two HID interfaces; the control/OLED one is identified by
/// its report descriptor starting with usage page `0xFFC0` (`06 c0 ff`), which
/// separates it from the consumer/media-key interface. The Arctis Pro
/// generation has a single control interface, so a product-id match suffices.
///
/// Nova Pro wins when both are somehow present, since it's the richer device.
pub fn find_device() -> Option<DevicePath> {
    let mut fallback: Option<DevicePath> = None;
    let entries = std::fs::read_dir("/sys/class/hidraw").ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("hidraw") {
            continue;
        }
        let sys = Path::new("/sys/class/hidraw").join(&*name).join("device");
        let Some((pid, kind)) = matching_product(&sys) else {
            continue;
        };
        let dev = PathBuf::from("/dev").join(&*name);
        match kind {
            DeviceKind::NovaPro => {
                // Only the vendor control collection can drive status + OLED.
                let desc = std::fs::read(sys.join("report_descriptor")).unwrap_or_default();
                if desc.starts_with(&[0x06, 0xc0, 0xff]) {
                    return Some(DevicePath { dev, product_id: pid, kind });
                }
            }
            DeviceKind::ArctisPro => {
                fallback.get_or_insert(DevicePath { dev, product_id: pid, kind });
            }
        }
    }
    fallback
}

/// Parse `HID_ID=0003:00001038:000012E0` from a hidraw device's uevent and
/// classify it as one of the supported base-station families.
fn matching_product(sys_device: &Path) -> Option<(u16, DeviceKind)> {
    let uevent = std::fs::read_to_string(sys_device.join("uevent")).ok()?;
    let hid_id = uevent
        .lines()
        .find_map(|l| l.strip_prefix("HID_ID="))?;
    let mut parts = hid_id.split(':');
    let _bus = parts.next()?;
    let vendor = u16::from_str_radix(parts.next()?.trim(), 16).ok()?;
    let product = u16::from_str_radix(parts.next()?.trim(), 16).ok()?;
    if vendor != VENDOR_ID {
        return None;
    }
    if PRODUCT_IDS.contains(&product) {
        Some((product, DeviceKind::NovaPro))
    } else if protocol_apw::PRODUCT_IDS.contains(&product) {
        Some((product, DeviceKind::ArctisPro))
    } else {
        None
    }
}

/// A writable handle to the base station: output reports (commands) and
/// feature reports (OLED). Cheap to clone the path; the file is the resource.
pub struct HidDevice {
    file: File,
    pub product_id: u16,
    pub kind: DeviceKind,
    pub path: PathBuf,
}

impl HidDevice {
    /// Open the discovered control node for read+write.
    pub fn open(path: &DevicePath) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path.dev)?;
        Ok(Self {
            file,
            product_id: path.product_id,
            kind: path.kind,
            path: path.dev.clone(),
        })
    }

    /// Discover and open the base station in one step.
    pub fn discover() -> io::Result<Self> {
        let path = find_device().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "no Arctis Nova Pro Wireless base station found",
            )
        })?;
        Self::open(&path)
    }

    /// Reconstruct the discovery path so a second fd (reader/OLED) can be
    /// opened for the same node.
    pub fn device_path(&self) -> DevicePath {
        DevicePath {
            dev: self.path.clone(),
            product_id: self.product_id,
            kind: self.kind,
        }
    }

    /// Write a command as a HID **output** report, zero-padded to this
    /// family's report length (64 bytes on Nova Pro, 31 on Arctis Pro).
    pub fn write_command(&mut self, body: &[u8]) -> io::Result<()> {
        let len = self.kind.command_len();
        let mut report = vec![0u8; len];
        let n = body.len().min(len);
        report[..n].copy_from_slice(&body[..n]);
        self.file.write_all(&report)?;
        Ok(())
    }

    /// Send a HID **feature** report (used for OLED framebuffer packets).
    /// `data[0]` must be the report id; `data` is sent verbatim.
    pub fn send_feature(&mut self, data: &[u8]) -> io::Result<()> {
        let fd = self.file.as_raw_fd();
        // SAFETY: `data` outlives the call and its length matches the size we
        // encode into the ioctl request number.
        let ret = unsafe {
            libc::ioctl(
                fd,
                hidiocsfeature(data.len()),
                data.as_ptr() as *const libc::c_void,
            )
        };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Open a second, blocking handle to the same node for a reader thread, so
    /// blocking reads never contend with command writes on one fd.
    pub fn open_reader(&self) -> io::Result<HidReader> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            // O_NONBLOCK off: the reader thread blocks until an event arrives.
            .custom_flags(0)
            .open(&self.path)?;
        Ok(HidReader { file })
    }
}

/// A blocking read handle for the status/event stream.
pub struct HidReader {
    file: File,
}
impl HidReader {
    /// Block until the next HID input report arrives; returns its length.
    pub fn read_report(&mut self, buf: &mut [u8; COMMAND_LEN]) -> io::Result<usize> {
        self.file.read(buf)
    }
}
