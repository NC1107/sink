//! SteelSeries Arctis Nova Pro Wireless base-station HID protocol.
//!
//! Pure, side-effect-free builders and parsers so the wire format can be unit
//! tested without any hardware. Everything talks report id `0x06`; the device
//! answers on `0x06` (command replies) and `0x07` (unsolicited push events).
//!
//! Command bytes were cross-checked against three independent reverse-engineering
//! efforts (HeadsetControl, elegos/Linux-Arctis-Manager, loteran/Arctis-Sound-Manager)
//! and verified live against a real base station (PID 0x12e0).

use serde::Serialize;

pub const VENDOR_ID: u16 = 0x1038;
/// Nova Pro Wireless base stations (incl. the Xbox "X" variants).
pub const PRODUCT_IDS: [u16; 3] = [0x12e0, 0x12e5, 0x225d];

/// HID report id every base-station packet carries as its first byte.
pub const REPORT_ID: u8 = 0x06;
/// Report id the device tags unsolicited push events with.
pub const EVENT_REPORT_ID: u8 = 0x07;
/// All control/output reports are zero-padded to this length.
pub const COMMAND_LEN: usize = 64;

// --- command opcodes (second byte, after the report id) -----------------
const CMD_SAVE: u8 = 0x09;
const CMD_STATUS: u8 = 0xb0;
const CMD_VOLUME_QUERY: u8 = 0x20;
const CMD_MIC_VOLUME: u8 = 0x37;
const CMD_SIDETONE: u8 = 0x39;
const CMD_EQ_BANDS: u8 = 0x33;
const CMD_EQ_PRESET: u8 = 0x2e;
const CMD_GAIN: u8 = 0x27;
const CMD_ANC: u8 = 0xbd;
const CMD_TRANSPARENCY: u8 = 0xb9;
const CMD_AUTO_OFF: u8 = 0xc1;
const CMD_WIRELESS_MODE: u8 = 0xc3;
const CMD_MIC_LED: u8 = 0xbf;
const CMD_LINE_OUT: u8 = 0x43;
const CMD_LINE_OUT_VOL: u8 = 0x47;
const CMD_CHATMIX: u8 = 0x49;

/// 0 dB baseline for a hardware EQ band; each unit is 0.5 dB.
const EQ_BASELINE: u8 = 0x14;
pub const EQ_BANDS: usize = 10;
pub const EQ_DB_MIN: f32 = -10.0;
pub const EQ_DB_MAX: f32 = 10.0;
/// The custom-preset slot the 10-band EQ writes into.
const EQ_CUSTOM_PRESET: u8 = 4;

/// Active noise-cancelling mode. Serialised as a lowercase string for the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AncMode {
    Off,
    Transparent,
    On,
}
impl AncMode {
    pub fn from_raw(v: u8) -> Self {
        match v {
            1 => AncMode::Transparent,
            2 => AncMode::On,
            _ => AncMode::Off,
        }
    }
    pub fn to_raw(self) -> u8 {
        match self {
            AncMode::Off => 0,
            AncMode::Transparent => 1,
            AncMode::On => 2,
        }
    }
}

/// Speaker vs. stream mode on the base station's line-out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LineOut {
    Speaker,
    Stream,
}
impl LineOut {
    fn from_raw(v: u8) -> Self {
        if v == 2 { LineOut::Stream } else { LineOut::Speaker }
    }
    pub fn to_raw(self) -> u8 {
        match self {
            LineOut::Speaker => 1,
            LineOut::Stream => 2,
        }
    }
}

/// Whether the headset is docked/charging, wirelessly online, or absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerStatus {
    Offline,
    CableCharging,
    Online,
    Unknown,
}
impl PowerStatus {
    fn from_raw(v: u8) -> Self {
        match v {
            0x01 => PowerStatus::Offline,
            0x02 => PowerStatus::CableCharging,
            0x08 => PowerStatus::Online,
            _ => PowerStatus::Unknown,
        }
    }
}

/// The auto-off timer index maps to a fixed set of minute values.
fn auto_off_minutes(idx: u8) -> u16 {
    match idx {
        1 => 1,
        2 => 5,
        3 => 10,
        4 => 15,
        5 => 30,
        6 => 60,
        _ => 0,
    }
}

// --- command builders ---------------------------------------------------
// Each returns the *unpadded* report body starting with the report id. The
// hidraw layer zero-pads to COMMAND_LEN before writing.

/// Minimal handshake to enable event reporting (ChatMix wheel, volume) and
/// pull an initial status snapshot — **without** writing any user settings, so
/// connecting never clobbers the headset's saved ANC/sidetone/EQ/etc.
pub fn init_safe() -> Vec<Vec<u8>> {
    vec![
        vec![REPORT_ID, 0x8d, 0x01],
        set_chatmix_enabled(true),
        vec![REPORT_ID, 0xb7, 0x00],
        status_query(),
        volume_query(),
    ]
}

pub fn save() -> Vec<u8> {
    vec![REPORT_ID, CMD_SAVE]
}
pub fn status_query() -> Vec<u8> {
    vec![REPORT_ID, CMD_STATUS]
}
pub fn volume_query() -> Vec<u8> {
    vec![REPORT_ID, CMD_VOLUME_QUERY]
}
/// Sidetone level 0 (off) .. 3 (high).
pub fn set_sidetone(level: u8) -> Vec<u8> {
    vec![REPORT_ID, CMD_SIDETONE, level.min(3)]
}
/// Mic volume 0x01 (muted) .. 0x0a (100%).
pub fn set_mic_volume(level: u8) -> Vec<u8> {
    vec![REPORT_ID, CMD_MIC_VOLUME, level.clamp(1, 10)]
}
/// Mic-mute LED brightness 1 .. 10.
pub fn set_mic_led(level: u8) -> Vec<u8> {
    vec![REPORT_ID, CMD_MIC_LED, level.clamp(1, 10)]
}
pub fn set_anc(mode: AncMode) -> Vec<u8> {
    vec![REPORT_ID, CMD_ANC, mode.to_raw()]
}
/// Transparency (transparent-mode passthrough) level 1 .. 10.
pub fn set_transparency_level(level: u8) -> Vec<u8> {
    vec![REPORT_ID, CMD_TRANSPARENCY, level.clamp(1, 10)]
}
/// Auto-off timer index 0 (never) .. 6 (60 min).
pub fn set_auto_off(idx: u8) -> Vec<u8> {
    vec![REPORT_ID, CMD_AUTO_OFF, idx.min(6)]
}
/// Audio gain: 1 = low, 2 = high.
pub fn set_gain_high(high: bool) -> Vec<u8> {
    vec![REPORT_ID, CMD_GAIN, if high { 2 } else { 1 }]
}
/// 2.4 GHz mode: false = speed, true = range.
pub fn set_wireless_range(range: bool) -> Vec<u8> {
    vec![REPORT_ID, CMD_WIRELESS_MODE, u8::from(range)]
}
pub fn set_line_out(mode: LineOut) -> Vec<u8> {
    vec![REPORT_ID, CMD_LINE_OUT, mode.to_raw()]
}
/// Stream-mode line-out volumes (main L/R and aux), each 0 .. 100.
pub fn set_line_out_volumes(left: u8, right: u8, aux: u8) -> Vec<u8> {
    vec![
        REPORT_ID,
        CMD_LINE_OUT_VOL,
        left.min(100),
        right.min(100),
        aux.min(100),
    ]
}
/// Enable/disable the hardware ChatMix wheel reporting.
pub fn set_chatmix_enabled(on: bool) -> Vec<u8> {
    vec![REPORT_ID, CMD_CHATMIX, u8::from(on)]
}
/// Select a built-in EQ preset (0 .. 3).
pub fn set_eq_preset(preset: u8) -> Vec<u8> {
    vec![REPORT_ID, CMD_EQ_PRESET, preset.min(3)]
}
/// Select the custom preset slot — sent right before [`set_eq_bands`].
pub fn select_custom_eq() -> Vec<u8> {
    vec![REPORT_ID, CMD_EQ_PRESET, EQ_CUSTOM_PRESET]
}
/// Encode 10 band gains (dB, clamped to ±10) into the device's 0.5 dB units.
pub fn set_eq_bands(bands_db: &[f32]) -> Vec<u8> {
    let mut cmd = vec![REPORT_ID, CMD_EQ_BANDS];
    for i in 0..EQ_BANDS {
        let db = bands_db.get(i).copied().unwrap_or(0.0).clamp(EQ_DB_MIN, EQ_DB_MAX);
        cmd.push(((EQ_BASELINE as f32) + 2.0 * db).round() as u8);
    }
    cmd
}
/// Decode a device EQ byte back to dB (inverse of [`set_eq_bands`]).
/// Kept for parsing live EQ-band events (0x0731) and used by tests.
#[allow(dead_code)]
pub fn eq_byte_to_db(byte: u8) -> f32 {
    (byte as f32 - EQ_BASELINE as f32) / 2.0
}

/// Full decoded base-station status, as pushed to the frontend.
#[derive(Debug, Clone, Serialize, Default)]
pub struct HeadsetStatus {
    /// True once at least one valid status frame has been parsed.
    pub present: bool,
    pub power_status: Option<String>,
    pub headset_battery_percent: Option<u8>,
    pub charge_slot_battery_percent: Option<u8>,
    pub anc: Option<AncMode>,
    pub transparency_level: Option<u8>,
    pub mic_muted: Option<bool>,
    pub mic_led_percent: Option<u8>,
    pub auto_off_minutes: Option<u16>,
    pub wireless_range_mode: Option<bool>,
    pub wireless_paired: Option<bool>,
    pub bluetooth_connected: Option<bool>,
    pub bluetooth_powered: Option<bool>,
    /// Station output volume 0 .. 100 (from the 0x0725 event / 0x20 query).
    pub volume_percent: Option<u8>,
    /// ChatMix game/chat balance 0 .. 100 each (from the 0x0745 event).
    pub chatmix_game: Option<u8>,
    pub chatmix_chat: Option<u8>,
    pub line_out: Option<LineOut>,
}

fn pct(raw: u8, max: u8) -> u8 {
    ((raw.min(max) as u16 * 100) / max as u16) as u8
}

impl HeadsetStatus {
    /// Merge a raw 64-byte HID report into this status. Returns true if the
    /// frame was recognised (and something may have changed).
    pub fn apply_frame(&mut self, buf: &[u8]) -> bool {
        if buf.len() < 3 {
            return false;
        }
        match (buf[0], buf[1]) {
            // Full status snapshot (reply to 0x06 0xb0).
            (_, CMD_STATUS) if buf.len() >= 16 => {
                self.present = true;
                self.bluetooth_powered = Some(buf[4] == 0); // power_status: 0=on,1=off
                self.bluetooth_connected = Some(buf[5] == 1);
                self.headset_battery_percent = Some(pct(buf[6], 8));
                self.charge_slot_battery_percent = Some(pct(buf[7], 8));
                self.transparency_level = Some(buf[8]);
                self.mic_muted = Some(buf[9] == 1);
                self.anc = Some(AncMode::from_raw(buf[10]));
                self.mic_led_percent = Some(pct(buf[11], 10));
                self.auto_off_minutes = Some(auto_off_minutes(buf[12]));
                self.wireless_range_mode = Some(buf[13] == 1);
                self.wireless_paired = Some(buf[14] == 8);
                self.power_status =
                    Some(serde_json_str(&PowerStatus::from_raw(buf[15])));
                true
            }
            // Station volume: raw 56 -> 0%, raw 0 -> 100%.
            (EVENT_REPORT_ID, 0x25) => {
                let raw = buf[2].min(56);
                self.volume_percent = Some(((56 - raw) as u16 * 100 / 56) as u8);
                true
            }
            // Reply to the 0x20 volume query: raw volume sits in byte 3.
            (_, CMD_VOLUME_QUERY) if buf.len() >= 4 => {
                let raw = buf[3].min(56);
                self.volume_percent = Some(((56 - raw) as u16 * 100 / 56) as u8);
                true
            }
            // ChatMix game/chat balance.
            (EVENT_REPORT_ID, 0x45) if buf.len() >= 4 => {
                self.chatmix_game = Some(buf[2].min(100));
                self.chatmix_chat = Some(buf[3].min(100));
                true
            }
            // ANC change event.
            (EVENT_REPORT_ID, CMD_ANC) => {
                self.anc = Some(AncMode::from_raw(buf[2]));
                true
            }
            // Transparency level change event.
            (EVENT_REPORT_ID, CMD_TRANSPARENCY) => {
                self.transparency_level = Some(buf[2]);
                true
            }
            // Line-out mode change event.
            (EVENT_REPORT_ID, CMD_LINE_OUT) => {
                self.line_out = Some(LineOut::from_raw(buf[2]));
                true
            }
            _ => false,
        }
    }
}

/// If `buf` is a ChatMix wheel event (`0x07 0x45 <game> <chat>`), return the
/// game/chat balance (0..100 each). Used to drive the software mix live.
pub fn chatmix_from_frame(buf: &[u8]) -> Option<(u8, u8)> {
    if buf.len() >= 4 && buf[0] == EVENT_REPORT_ID && buf[1] == 0x45 {
        Some((buf[2].min(100), buf[3].min(100)))
    } else {
        None
    }
}

/// Serialise a serde enum to its bare string (drops the JSON quotes).
fn serde_json_str<T: Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|j| j.as_str().map(str::to_owned))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eq_encoding_roundtrips() {
        assert_eq!(set_eq_bands(&[0.0; 10])[2..], [EQ_BASELINE; 10]);
        // +10 dB -> baseline + 20; -10 dB -> baseline - 20.
        let cmd = set_eq_bands(&[10.0, -10.0]);
        assert_eq!(cmd[2], EQ_BASELINE + 20);
        assert_eq!(cmd[3], EQ_BASELINE - 20);
        assert!((eq_byte_to_db(EQ_BASELINE + 20) - 10.0).abs() < 1e-6);
    }

    #[test]
    fn eq_clamps_out_of_range() {
        let cmd = set_eq_bands(&[99.0, -99.0]);
        assert_eq!(cmd[2], EQ_BASELINE + 20);
        assert_eq!(cmd[3], EQ_BASELINE - 20);
    }

    #[test]
    fn parses_real_status_frame() {
        // Captured live from a real base station.
        let frame = [
            0x06, 0xb0, 0x00, 0x00, 0x01, 0x00, 0x02, 0x08, 0x08, 0x01, 0x02, 0x0a, 0x05,
            0x01, 0x08, 0x08,
        ];
        let mut s = HeadsetStatus::default();
        assert!(s.apply_frame(&frame));
        assert_eq!(s.headset_battery_percent, Some(25)); // 2/8
        assert_eq!(s.charge_slot_battery_percent, Some(100)); // 8/8
        assert_eq!(s.anc, Some(AncMode::On));
        assert_eq!(s.mic_muted, Some(true));
        assert_eq!(s.wireless_range_mode, Some(true));
        assert_eq!(s.auto_off_minutes, Some(30));
        assert_eq!(s.power_status.as_deref(), Some("online"));
    }

    #[test]
    fn parses_volume_event() {
        let mut s = HeadsetStatus::default();
        assert!(s.apply_frame(&[0x07, 0x25, 0]));
        assert_eq!(s.volume_percent, Some(100));
        assert!(s.apply_frame(&[0x07, 0x25, 56]));
        assert_eq!(s.volume_percent, Some(0));
    }

    #[test]
    fn ignores_unknown_frames() {
        let mut s = HeadsetStatus::default();
        assert!(!s.apply_frame(&[0x07, 0xff, 0]));
    }
}
