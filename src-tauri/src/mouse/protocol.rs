//! SteelSeries **Aerox 9 Wireless** mouse protocol.
//!
//! Different again from the headsets: control lives on USB interface **3**,
//! reports are unnumbered (`0x00` report id) HID *output* reports of variable
//! length (not padded), and the wireless dongle uses different opcodes from
//! the wired connection — bit 6 is set on the **first command byte only**.
//!
//! Byte sequences transcribed from rivalcfg's Aerox 9 Wireless device profile
//! (whose unit tests pin the exact wire format) and cross-checked before use.

use serde::{Deserialize, Serialize};

pub const VENDOR_ID: u16 = 0x1038;
/// 2.4 GHz dongle (wireless opcode encoding).
pub const PID_WIRELESS: u16 = 0x1858;
/// Wired / USB (base opcode encoding).
pub const PID_WIRED: u16 = 0x185a;
/// WOW Edition variants behave like the two above.
pub const PID_WIRELESS_WOW: u16 = 0x1874;
pub const PID_WIRED_WOW: u16 = 0x1876;

pub const PRODUCT_IDS: [u16; 4] = [PID_WIRELESS, PID_WIRED, PID_WIRELESS_WOW, PID_WIRED_WOW];

/// The USB interface carrying the configuration collection.
pub const CONTROL_INTERFACE: u8 = 3;

/// Devices connected over the dongle need bit 6 set on the opcode.
pub fn is_wireless(product_id: u16) -> bool {
    product_id == PID_WIRELESS || product_id == PID_WIRELESS_WOW
}

// --- opcodes (wired form; wireless ORs 0x40 onto the first byte) ---------
const CMD_DPI: u8 = 0x2d;
const CMD_POLLING: u8 = 0x2b;
const CMD_COLOR: u8 = 0x21;
const CMD_REACTIVE: u8 = 0x26;
const CMD_RAINBOW: u8 = 0x22;
const CMD_STARTUP_LIGHTING: u8 = 0x27;
const CMD_SLEEP: u8 = 0x29;
const CMD_DIM: u8 = 0x23;
const CMD_BATTERY: u8 = 0x92;
const CMD_SAVE: u8 = 0x11;

/// The TrueMove Air sensor's DPI codes are a lookup table, *not* `dpi / 100`.
const DPI_TABLE: &[(u16, u8)] = &[
    (100, 0x00),
    (200, 0x02),
    (300, 0x03),
    (400, 0x04),
    (500, 0x05),
    (600, 0x06),
    (700, 0x07),
    (800, 0x09),
    (900, 0x0a),
    (1000, 0x0b),
    (1200, 0x0d),
    (1600, 0x12),
    (2400, 0x1b),
    (3200, 0x26),
    (6400, 0x4c),
    (12800, 0x99),
    (18000, 0xd6),
];

/// Selectable DPI steps for the UI.
pub fn dpi_steps() -> Vec<u16> {
    DPI_TABLE.iter().map(|(dpi, _)| *dpi).collect()
}

/// Nearest supported sensor code for a requested DPI.
fn dpi_code(dpi: u16) -> u8 {
    DPI_TABLE
        .iter()
        .min_by_key(|(d, _)| (*d as i32 - dpi as i32).abs())
        .map(|(_, code)| *code)
        .unwrap_or(0x04)
}

/// Polling rate in Hz -> wire code.
fn polling_code(hz: u16) -> u8 {
    match hz {
        125 => 0x03,
        250 => 0x02,
        500 => 0x01,
        _ => 0x00, // 1000 Hz
    }
}

/// Supported polling rates for the UI.
pub const POLLING_RATES: [u16; 4] = [125, 250, 500, 1000];
/// RGB zones: top, middle, bottom.
pub const ZONE_COUNT: u8 = 3;

/// A command ready to send: opcode-encoded for the connected transport.
fn encode(product_id: u16, mut bytes: Vec<u8>) -> Vec<u8> {
    if is_wireless(product_id) {
        if let Some(first) = bytes.first_mut() {
            *first |= 0x40;
        }
    }
    // hidraw expects the (unnumbered) report id as byte 0.
    let mut out = Vec::with_capacity(bytes.len() + 1);
    out.push(0x00);
    out.extend_from_slice(&bytes);
    out
}

/// Set the DPI preset list and which one is active.
pub fn set_dpi(product_id: u16, presets: &[u16], selected: u8) -> Vec<u8> {
    let presets: Vec<u16> = presets.iter().copied().take(5).collect();
    let count = presets.len().max(1) as u8;
    let mut cmd = vec![CMD_DPI, count, selected.min(count.saturating_sub(1))];
    for dpi in &presets {
        cmd.push(dpi_code(*dpi));
    }
    encode(product_id, cmd)
}

/// Set the polling rate (125 / 250 / 500 / 1000 Hz).
pub fn set_polling(product_id: u16, hz: u16) -> Vec<u8> {
    encode(product_id, vec![CMD_POLLING, polling_code(hz)])
}

/// Set one RGB zone's solid colour (zone 0 = top, 1 = middle, 2 = bottom).
pub fn set_zone_color(product_id: u16, zone: u8, r: u8, g: u8, b: u8) -> Vec<u8> {
    encode(
        product_id,
        vec![CMD_COLOR, 0x01, zone.min(ZONE_COUNT - 1), r, g, b],
    )
}

/// Reactive (click-flash) colour; `None` disables it.
pub fn set_reactive(product_id: u16, rgb: Option<(u8, u8, u8)>) -> Vec<u8> {
    let cmd = match rgb {
        Some((r, g, b)) => vec![CMD_REACTIVE, 0x01, 0x00, r, g, b],
        None => vec![CMD_REACTIVE, 0x00, 0x00, 0x00, 0x00, 0x00],
    };
    encode(product_id, cmd)
}

/// Enable the built-in rainbow effect.
pub fn set_rainbow(product_id: u16) -> Vec<u8> {
    encode(product_id, vec![CMD_RAINBOW, 0xff])
}

/// Which effects are active at power-on.
pub fn set_startup_lighting(product_id: u16, rainbow: bool, reactive: bool) -> Vec<u8> {
    encode(
        product_id,
        vec![CMD_STARTUP_LIGHTING, u8::from(rainbow), u8::from(reactive)],
    )
}

/// Sleep timeout in minutes (0 disables, max 20). Sent as 3-byte LE ms.
pub fn set_sleep_minutes(product_id: u16, minutes: u16) -> Vec<u8> {
    let ms = (minutes.min(20) as u32) * 60_000;
    encode(
        product_id,
        vec![
            CMD_SLEEP,
            (ms & 0xff) as u8,
            ((ms >> 8) & 0xff) as u8,
            ((ms >> 16) & 0xff) as u8,
        ],
    )
}

/// Light-dim timeout in seconds (0 disables, max 1200). Sent as 3-byte LE ms.
pub fn set_dim_seconds(product_id: u16, seconds: u16) -> Vec<u8> {
    let ms = (seconds.min(1200) as u32) * 1000;
    encode(
        product_id,
        vec![
            CMD_DIM,
            0x0f,
            0x01,
            0x00,
            0x00,
            (ms & 0xff) as u8,
            ((ms >> 8) & 0xff) as u8,
            ((ms >> 16) & 0xff) as u8,
        ],
    )
}

/// Battery query — the reply is read back separately.
pub fn battery_query(product_id: u16) -> Vec<u8> {
    encode(product_id, vec![CMD_BATTERY])
}

/// Persist the current configuration to the mouse's onboard memory.
pub fn save(product_id: u16) -> Vec<u8> {
    encode(product_id, vec![CMD_SAVE, 0x00])
}

/// Decode a battery reply.
///
/// The reply echoes the command in byte 0 and carries the battery byte in
/// byte 1: bit 7 is the charging flag, the low 7 bits are a level where
/// percent = (level - 1) * 5. An idle dongle (mouse currently on the cable)
/// answers `0xff`, which falls outside the valid range and is rejected.
/// Confirmed against real hardware: wired reply `92 95 …` = charging, 100%.
pub fn parse_battery(buf: &[u8]) -> Option<(u8, bool)> {
    let raw = *buf.get(1)?;
    let charging = raw & 0x80 != 0;
    let level = raw & 0x7f;
    let percent = (level as i32 - 1) * 5;
    if !(0..=100).contains(&percent) {
        return None;
    }
    Some((percent as u8, charging))
}

/// Full mouse configuration as persisted and shown in the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MouseConfig {
    pub dpi_presets: Vec<u16>,
    pub dpi_selected: u8,
    pub polling_hz: u16,
    /// One RGB triple per zone (top, middle, bottom).
    pub zones: Vec<[u8; 3]>,
    pub rainbow: bool,
    pub reactive: Option<[u8; 3]>,
    pub sleep_minutes: u16,
    pub dim_seconds: u16,
}

impl Default for MouseConfig {
    fn default() -> Self {
        // rivalcfg's documented factory defaults.
        Self {
            dpi_presets: vec![400, 800, 1200, 2400, 3200],
            dpi_selected: 0,
            polling_hz: 1000,
            zones: vec![[255, 0, 0], [0, 255, 0], [0, 0, 255]],
            rainbow: true,
            reactive: None,
            sleep_minutes: 5,
            dim_seconds: 30,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wireless_sets_bit6_on_first_byte_only() {
        let wired = set_polling(PID_WIRED, 1000);
        let wireless = set_polling(PID_WIRELESS, 1000);
        // byte 0 is the report id; byte 1 is the opcode.
        assert_eq!(wired[1], 0x2b);
        assert_eq!(wireless[1], 0x6b);
        assert_eq!(wired[2], wireless[2]); // payload unchanged
    }

    #[test]
    fn dpi_uses_the_sensor_lookup_table() {
        let cmd = set_dpi(PID_WIRED, &[400, 800, 1200, 2400, 3200], 0);
        assert_eq!(cmd[1], 0x2d); // opcode
        assert_eq!(cmd[2], 5); // count
        assert_eq!(cmd[3], 0); // selected
        assert_eq!(&cmd[4..9], &[0x04, 0x09, 0x0d, 0x1b, 0x26]);
    }

    #[test]
    fn dpi_snaps_to_the_nearest_supported_step() {
        // 810 is closest to 800 (code 0x09), not 900.
        let cmd = set_dpi(PID_WIRED, &[810], 0);
        assert_eq!(cmd[4], 0x09);
    }

    #[test]
    fn polling_codes_match_the_spec() {
        assert_eq!(set_polling(PID_WIRED, 1000)[2], 0x00);
        assert_eq!(set_polling(PID_WIRED, 500)[2], 0x01);
        assert_eq!(set_polling(PID_WIRED, 250)[2], 0x02);
        assert_eq!(set_polling(PID_WIRED, 125)[2], 0x03);
    }

    #[test]
    fn timeouts_encode_as_three_byte_le_milliseconds() {
        // 5 minutes = 300000 ms = 0x0493E0 -> E0 93 04
        let cmd = set_sleep_minutes(PID_WIRED, 5);
        assert_eq!(&cmd[2..5], &[0xe0, 0x93, 0x04]);
        // 30 s = 30000 ms = 0x7530 -> 30 75 00
        let dim = set_dim_seconds(PID_WIRED, 30);
        assert_eq!(&dim[6..9], &[0x30, 0x75, 0x00]);
    }

    #[test]
    fn battery_decodes_level_and_charging() {
        // Captured from real hardware over the cable: echo 0x92, byte1 0x95.
        assert_eq!(parse_battery(&[0x92, 0x95]), Some((100, true)));
        // 0x15 = level 21 -> (21-1)*5 = 100%, not charging
        assert_eq!(parse_battery(&[0x92, 0x15]), Some((100, false)));
        // level 11 -> 50%
        assert_eq!(parse_battery(&[0x92, 0x0b]), Some((50, false)));
    }

    #[test]
    fn battery_rejects_the_idle_dongle_reply() {
        // Captured from real hardware: an idle dongle answers 0xff, which
        // would otherwise decode to a nonsensical 630%.
        assert_eq!(parse_battery(&[0x40, 0xff]), None);
        assert_eq!(parse_battery(&[0x00, 0x7f]), None);
    }
}
