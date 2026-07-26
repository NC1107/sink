//! SteelSeries mouse support (Aerox 9 Wireless).
//!
//! Structurally a smaller sibling of [`crate::headset`]: discover the vendor
//! control interface on `/dev/hidraw`, keep a writer around for commands, and
//! poll battery in the background.

pub mod manager;
pub mod protocol;

pub use manager::MouseManager;
