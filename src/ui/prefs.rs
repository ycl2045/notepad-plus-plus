#![allow(dead_code)]
//! User preferences, persisted as JSON under the user config directory.
//!
//! OWNED BY: U10.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ui::theme::ThemeMode;

fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            home.join(".config")
        })
        .join("npad")
}

pub fn prefs_path() -> PathBuf {
    config_dir().join("prefs.json")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub theme: ThemeMode,
    pub font_family: String,
    pub font_size: u16,
    pub tab_width: usize,
    pub wrap: bool,
    pub restore_session: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            theme: ThemeMode::Dark,
            font_family: String::from("monospace"),
            font_size: 11,
            tab_width: 4,
            wrap: false,
            restore_session: true,
        }
    }
}

impl Prefs {
    pub fn load() -> Self {
        match std::fs::read_to_string(prefs_path()) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
            Err(_) => Prefs::default(),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = prefs_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let p = Prefs::default();
        assert_eq!(p.theme, ThemeMode::Dark);
        assert_eq!(p.font_size, 11);
        assert_eq!(p.tab_width, 4);
        assert!(p.restore_session);
    }

    #[test]
    fn roundtrips_through_json() {
        let p = Prefs {
            tab_width: 8,
            wrap: true,
            theme: ThemeMode::Light,
            ..Prefs::default()
        };
        let raw = serde_json::to_string(&p).unwrap();
        let back: Prefs = serde_json::from_str(&raw).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let back: Prefs = serde_json::from_str("{\"tab_width\": 2}").unwrap();
        assert_eq!(back.tab_width, 2);
        assert_eq!(back.font_size, Prefs::default().font_size);
    }
}
