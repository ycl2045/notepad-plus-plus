#![allow(dead_code)]
//! User preferences, persisted as JSON under the user config directory.
//!
//! OWNED BY: U10.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ui::paths::{config_dir, write_atomic};
use crate::ui::theme::ThemeMode;

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
    /// Load the preferences.
    ///
    /// A missing or corrupt file is not an error here: the defaults are used
    /// instead. Use [`Prefs::load_strict`] when the reason matters.
    pub fn load() -> Self {
        Self::load_strict().unwrap_or_default()
    }

    /// Load the preferences and report why that failed.
    ///
    /// The error is a [`serde_json::Error`]; for a missing or unreadable file
    /// it carries the underlying I/O error (`Error::is_io`).
    pub fn load_strict() -> Result<Prefs, serde_json::Error> {
        Self::load_from(&prefs_path())
    }

    /// Load the preferences from an explicit path.
    pub fn load_from(path: &Path) -> Result<Prefs, serde_json::Error> {
        let raw = std::fs::read_to_string(path).map_err(serde_json::Error::io)?;
        serde_json::from_str(&raw)
    }

    /// Persist the preferences.
    ///
    /// The write is atomic: the previous file stays intact if the write fails.
    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&prefs_path())
    }

    /// Persist the preferences to an explicit path, atomically.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        let raw = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        write_atomic(path, raw.as_bytes())
    }

    /// Font description in the `pango::FontDescription` string form, e.g.
    /// `"monospace 11"`.
    ///
    /// A blank family falls back to `monospace` and a zero size is clamped to
    /// `1`, so the result is always parseable by pango.
    pub fn font_description(&self) -> String {
        let family = self.font_family.trim();
        let family = if family.is_empty() {
            "monospace"
        } else {
            family
        };
        let size = self.font_size.max(1);
        format!("{family} {size}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::paths::temp_dir;

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

    #[test]
    fn font_description_uses_the_pango_format() {
        assert_eq!(Prefs::default().font_description(), "monospace 11");
        let p = Prefs {
            font_family: String::from("DejaVu Sans Mono"),
            font_size: 14,
            ..Prefs::default()
        };
        assert_eq!(p.font_description(), "DejaVu Sans Mono 14");
    }

    #[test]
    fn font_description_falls_back_for_unusable_values() {
        let p = Prefs {
            font_family: String::from("   "),
            font_size: 0,
            ..Prefs::default()
        };
        assert_eq!(p.font_description(), "monospace 1");
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = temp_dir("prefs-roundtrip");
        let path = dir.join("prefs.json");
        let prefs = Prefs {
            theme: ThemeMode::Light,
            font_family: String::from("Noto Sans Mono"),
            font_size: 13,
            tab_width: 8,
            wrap: true,
            restore_session: false,
        };
        prefs.save_to(&path).expect("save prefs");
        let back = Prefs::load_from(&path).expect("load prefs");
        assert_eq!(back, prefs);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_strict_reports_a_missing_file() {
        let dir = temp_dir("prefs-missing");
        let err = Prefs::load_from(&dir.join("prefs.json")).expect_err("missing file must fail");
        assert!(err.is_io());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_strict_reports_a_corrupt_file() {
        let dir = temp_dir("prefs-corrupt");
        let path = dir.join("prefs.json");
        std::fs::write(&path, "{not json").expect("write");
        let err = Prefs::load_from(&path).expect_err("corrupt file must fail");
        assert!(!err.is_io());
        // The lenient loader still yields the defaults.
        assert_eq!(
            Prefs::load_from(&path).unwrap_or_default(),
            Prefs::default()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
