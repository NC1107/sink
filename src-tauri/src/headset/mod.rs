//! SteelSeries Arctis Nova Pro Wireless base-station support.
//!
//! Sink is otherwise a pure PipeWire audio router; this module is the only
//! part that talks to the headset hardware. It speaks the vendor HID protocol
//! directly over `/dev/hidraw` (no libhidapi) to expose device settings (ANC,
//! sidetone, mic, EQ, …), live status (battery, ChatMix, connection) and the
//! 128x64 OLED (text, notifications, images, GIFs, video).

pub mod clips;
pub mod eq_presets;
mod font;
pub mod hidraw;
pub mod manager;
pub mod media;
pub mod oled;
pub mod oled_controller;
pub mod oled_mode_media;
pub mod oled_mode_audio;
pub mod oled_mode_clock;
pub mod oled_mode_misc;
pub mod oled_mode_spectrum;
pub mod oled_mode_sys;
pub mod oled_rotation;
pub mod protocol;
pub mod protocol_apw;
pub mod sysinfo;

pub use manager::HeadsetManager;
