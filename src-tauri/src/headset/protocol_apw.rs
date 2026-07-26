//! SteelSeries **Arctis Pro Wireless** (the pre-Nova generation) base-station
//! protocol.
//!
//! This is a genuinely different device from the Nova Pro Wireless, not a
//! variant — different interface, packet length, framing and value ranges:
//!
//! | | Nova Pro Wireless | Arctis Pro Wireless |
//! |---|---|---|
//! | product ids | `12e0/12e5/225d` | `1290/1294` |
//! | control interface | 4 | 0 |
//! | packet length | 64 | 31 |
//! | framing | report id `0x06` prefix | `0xAA` suffix |
//! | status query | `06 b0` | `41 aa` |
//! | battery range | 0..8 | 0..4 |
//!
//! Commands are Output reports. Over `hidraw` the kernel turns a plain
//! `write()` into either an interrupt-OUT transfer or a control SET_REPORT,
//! whichever the interface offers — which matches what the reference
//! implementations do with an explicit control transfer.
//!
//! NOTE: transcribed from loteran/Arctis-Sound-Manager's device spec and not
//! yet verified against real hardware (we have no Arctis Pro Wireless to test
//! on). Byte values are believed correct; behaviour needs confirming.

use serde::Serialize;

pub const PRODUCT_IDS: [u16; 2] = [0x1290, 0x1294];
/// Every command is padded to this length.
pub const COMMAND_LEN: usize = 31;
/// Suffix byte that terminates the opcode on this generation.
const AA: u8 = 0xaa;

const CMD_STATUS: u8 = 0x41;
const CMD_SIDETONE: u8 = 0x39;
const CMD_AUTO_OFF: u8 = 0x3c;
const CMD_SAVE: u8 = 0x90;

/// Status query (`0x41 0xAA`).
pub fn status_query() -> Vec<u8> {
    vec![CMD_STATUS, AA]
}

/// Persist settings to the base station (`0x90 0xAA`).
pub fn save() -> Vec<u8> {
    vec![CMD_SAVE, AA]
}

/// Sidetone level 0 (off) ..= 9 (max) — a wider range than Nova's 0..3.
pub fn set_sidetone(level: u8) -> Vec<u8> {
    vec![CMD_SIDETONE, AA, level.min(9)]
}

/// Auto shut-off. The wire value is *minutes / 10*, and only a few steps are
/// accepted: never, 10, 20, 30, 60, 90 minutes.
pub fn set_auto_off(minutes: u16) -> Vec<u8> {
    let raw = match minutes {
        0 => 0x00,
        1..=15 => 0x01,  // 10 min
        16..=25 => 0x02, // 20 min
        26..=45 => 0x03, // 30 min
        46..=75 => 0x06, // 60 min
        _ => 0x09,       // 90 min
    };
    vec![CMD_AUTO_OFF, AA, raw]
}

/// The auto-off wire value expressed back as minutes.
#[allow(dead_code)] // used once status frames start reporting it back
pub fn auto_off_minutes(raw: u8) -> u16 {
    (raw as u16) * 10
}

/// Minimal handshake: just ask for status. Unlike the Nova generation there
/// is no event-enable command to send, and we deliberately avoid writing any
/// user settings on connect.
pub fn init_safe() -> Vec<Vec<u8>> {
    vec![status_query()]
}

/// Decoded Arctis Pro Wireless status. Far smaller than the Nova's frame —
/// this generation only reports power state and battery.
#[derive(Debug, Clone, Serialize, Default)]
pub struct ApwStatus {
    pub present: bool,
    pub online: bool,
    /// 0..100, mapped from the device's 0..4 scale.
    pub battery_percent: Option<u8>,
}

impl ApwStatus {
    /// Merge a raw status frame. The reply is a 32-byte packet whose byte 0 is
    /// the power status (`0x04` online, `0x02` offline) and byte 1 the battery
    /// level on a 0..4 scale.
    pub fn apply_frame(&mut self, buf: &[u8]) -> bool {
        if buf.len() < 2 {
            return false;
        }
        match buf[0] {
            0x04 | 0x02 => {
                self.present = true;
                self.online = buf[0] == 0x04;
                let raw = buf[1].min(4);
                self.battery_percent = Some((raw as u16 * 100 / 4) as u8);
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_carry_the_aa_suffix() {
        assert_eq!(status_query(), vec![0x41, 0xaa]);
        assert_eq!(save(), vec![0x90, 0xaa]);
        assert_eq!(set_sidetone(3), vec![0x39, 0xaa, 3]);
    }

    #[test]
    fn sidetone_clamps_to_nine() {
        assert_eq!(set_sidetone(50)[2], 9);
    }

    #[test]
    fn auto_off_uses_ten_minute_steps() {
        assert_eq!(set_auto_off(0)[2], 0x00);
        assert_eq!(set_auto_off(10)[2], 0x01);
        assert_eq!(set_auto_off(30)[2], 0x03);
        assert_eq!(set_auto_off(60)[2], 0x06);
        assert_eq!(set_auto_off(120)[2], 0x09);
    }

    #[test]
    fn parses_documented_status_frames() {
        // Captured in the reference project: [4, 2, 0, ...] = online 50%.
        let mut s = ApwStatus::default();
        assert!(s.apply_frame(&[0x04, 0x02, 0x00]));
        assert!(s.online);
        assert_eq!(s.battery_percent, Some(50));

        assert!(s.apply_frame(&[0x02, 0x02, 0x00]));
        assert!(!s.online);
    }

    #[test]
    fn ignores_unrelated_frames() {
        let mut s = ApwStatus::default();
        assert!(!s.apply_frame(&[0xff, 0x00]));
    }
}
