//! Session state: open files, caret positions, recent files, window geometry.
//!
//! Persisted as JSON under the user config directory so the editor restores
//! the previous workspace on the next launch.
//!
//! OWNED BY: U10.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ui::prefs::Prefs;

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

pub fn session_path() -> PathBuf {
    config_dir().join("session.json")
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionFile {
    pub path: PathBuf,
    pub caret: usize,
    pub scroll: f64,
}

impl Default for SessionFile {
    fn default() -> Self {
        SessionFile {
            path: PathBuf::new(),
            caret: 0,
            scroll: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowState {
    pub width: i32,
    pub height: i32,
}

impl Default for WindowState {
    fn default() -> Self {
        WindowState {
            width: 1000,
            height: 700,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionState {
    pub open_files: Vec<SessionFile>,
    pub active_index: usize,
    pub recent_files: Vec<PathBuf>,
    pub window: WindowState,
    pub prefs: Prefs,
}

impl SessionState {
    pub fn load() -> Self {
        match std::fs::read_to_string(session_path()) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
            Err(_) => SessionState::default(),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = session_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, raw)
    }

    /// Push a path onto the recent-files list, de-duplicating and capping it.
    pub fn push_recent(&mut self, path: &PathBuf, max: usize) {
        self.recent_files.retain(|p| p != path);
        self.recent_files.insert(0, path.clone());
        self.recent_files.truncate(max);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_through_json() {
        let mut s = SessionState::default();
        s.open_files.push(SessionFile {
            path: PathBuf::from("/tmp/a.rs"),
            caret: 42,
            scroll: 12.5,
        });
        s.active_index = 0;
        s.window = WindowState {
            width: 1200,
            height: 800,
        };
        s.prefs.tab_width = 2;
        let raw = serde_json::to_string(&s).unwrap();
        let back: SessionState = serde_json::from_str(&raw).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn recent_files_are_deduped_and_capped() {
        let mut s = SessionState::default();
        s.push_recent(&PathBuf::from("/a"), 3);
        s.push_recent(&PathBuf::from("/b"), 3);
        s.push_recent(&PathBuf::from("/a"), 3);
        assert_eq!(s.recent_files.len(), 2);
        assert_eq!(s.recent_files[0], PathBuf::from("/a"));
        for i in 0..10 {
            s.push_recent(&PathBuf::from(format!("/f{}", i)), 3);
        }
        assert_eq!(s.recent_files.len(), 3);
    }

    #[test]
    fn missing_file_yields_defaults() {
        let s = SessionState::load();
        assert!(s.open_files.is_empty());
        assert_eq!(s.window.width, 1000);
    }
}
