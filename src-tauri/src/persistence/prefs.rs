use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::SinkError;

/// How Sink's devices are labeled in other apps' device lists.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeviceLabelStyle {
    /// "Game"
    #[default]
    Plain,
    /// "Game (Sink)"
    Suffix,
    /// "Sink · Game"
    Prefix,
}

/// App preferences, stored at `$XDG_CONFIG_HOME/sink/prefs.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prefs {
    #[serde(default)]
    pub device_label_style: DeviceLabelStyle,
    /// First-run tutorial completed (false = show it on launch).
    #[serde(default)]
    pub onboarded: bool,
    /// ChatMix-style balance: the two channel sink names being balanced
    /// (None = auto: Game/Chat when present, else the first two channels).
    #[serde(default)]
    pub balance_a: Option<String>,
    #[serde(default)]
    pub balance_b: Option<String>,
    /// Show the balance slider in the title bar.
    #[serde(default = "default_true")]
    pub show_balance: bool,
    /// When autostarting on login, boot straight to the tray instead of
    /// showing the window (only meaningful with autostart enabled).
    #[serde(default)]
    pub start_minimized: bool,
    /// Let WebKit render on the GPU (its DMABUF renderer). Off by default,
    /// since the CPU path is the one every stack can draw; see `renderer`.
    /// Read once at launch, before the webview exists.
    #[serde(default)]
    pub gpu_rendering: bool,
    /// GPU rendering was switched off because a launch with it never
    /// started, so Settings can say why.
    #[serde(default)]
    pub gpu_rendering_failed: bool,
}

fn default_true() -> bool {
    true
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            device_label_style: DeviceLabelStyle::default(),
            onboarded: false,
            balance_a: None,
            balance_b: None,
            show_balance: true,
            start_minimized: false,
            gpu_rendering: false,
            gpu_rendering_failed: false,
        }
    }
}

impl Prefs {
    pub fn config_path() -> Result<PathBuf, SinkError> {
        let dir = crate::persistence::config_root()
            .ok_or_else(|| SinkError::Config("cannot resolve the user config directory".into()))?;
        Ok(dir.join("sink").join("prefs.json"))
    }

    pub fn load() -> Self {
        let Ok(path) = Self::config_path() else {
            return Self::default();
        };
        fs::read_to_string(&path)
            .map(|raw| Self::parse(&raw))
            .unwrap_or_default()
    }

    /// Parse stored prefs; malformed input degrades to defaults rather
    /// than blocking launch.
    fn parse(raw: &str) -> Self {
        serde_json::from_str(raw).unwrap_or_else(|e| {
            eprintln!("sink: ignoring malformed prefs: {e}");
            Self::default()
        })
    }

    pub fn save(&self) -> Result<(), SinkError> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            crate::persistence::ensure_private_dir(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| SinkError::Config(format!("serialize prefs: {e}")))?;
        super::write_atomic(&path, &json)?;
        Ok(())
    }

    /// Decorate a device label per the chosen style (applied at node
    /// creation; stored labels stay raw).
    pub fn decorate(&self, label: &str) -> String {
        match self.device_label_style {
            DeviceLabelStyle::Plain => label.to_string(),
            DeviceLabelStyle::Suffix => format!("{label} (Sink)"),
            DeviceLabelStyle::Prefix => format!("Sink · {label}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decorate_styles() {
        let mut p = Prefs::default();
        assert_eq!(p.decorate("Game"), "Game");
        p.device_label_style = DeviceLabelStyle::Suffix;
        assert_eq!(p.decorate("Game"), "Game (Sink)");
        p.device_label_style = DeviceLabelStyle::Prefix;
        assert_eq!(p.decorate("Game"), "Sink · Game");
    }

    #[test]
    fn malformed_prefs_degrade_to_defaults() {
        // Corrupt / partially-written files must never panic or block
        // launch - they fall back to defaults.
        assert_eq!(Prefs::parse(""), Prefs::default());
        assert_eq!(Prefs::parse("{not json"), Prefs::default());
        assert_eq!(Prefs::parse("[]"), Prefs::default());
        assert_eq!(
            Prefs::parse(r#"{"device_label_style":"bogus_style"}"#),
            Prefs::default()
        );
        // Unknown fields are tolerated; known fields still apply.
        let p = Prefs::parse(r#"{"device_label_style":"suffix","future_field":1}"#);
        assert_eq!(p.device_label_style, DeviceLabelStyle::Suffix);
    }

    #[test]
    fn gpu_rendering_is_opt_in() {
        // Files written before the setting existed keep the CPU renderer.
        assert!(!Prefs::parse(r#"{"onboarded":true}"#).gpu_rendering);
        assert!(Prefs::parse(r#"{"gpu_rendering":true}"#).gpu_rendering);
    }
}
