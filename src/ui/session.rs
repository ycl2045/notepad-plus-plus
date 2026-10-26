#![allow(dead_code)]
//! Session state: open files, caret positions, recent files, window geometry.
//!
//! Persisted as JSON under the user config directory so the editor restores
//! the previous workspace on the next launch.
//!
//! OWNED BY: U10.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ui::paths::{config_dir, write_atomic};
use crate::ui::prefs::Prefs;

/// Maximum number of entries kept in the recent-files list.
pub const RECENT_MAX: usize = 20;

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
    /// Load the session.
    ///
    /// A missing or corrupt file is not an error here: the defaults are used
    /// instead. Use [`SessionState::load_strict`] when the reason matters.
    pub fn load() -> Self {
        Self::load_strict().unwrap_or_default()
    }

    /// Load the session and report why that failed.
    pub fn load_strict() -> Result<SessionState, serde_json::Error> {
        Self::load_from(&session_path())
    }

    /// Load the session from an explicit path.
    pub fn load_from(path: &Path) -> Result<SessionState, serde_json::Error> {
        let raw = std::fs::read_to_string(path).map_err(serde_json::Error::io)?;
        serde_json::from_str(&raw)
    }

    /// Persist the session.
    ///
    /// The write is atomic: the previous file stays intact if the write fails.
    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&session_path())
    }

    /// Persist the session to an explicit path, atomically.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        let raw = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        write_atomic(path, raw.as_bytes())
    }

    /// The file at `active_index`, or `None` when the index is out of range.
    pub fn active_file(&self) -> Option<&SessionFile> {
        self.open_files.get(self.active_index)
    }

    /// Drop the open files whose path no longer exists.
    ///
    /// Returns how many entries were removed. The active file is kept active
    /// when it still exists; otherwise `active_index` is clamped into the
    /// remaining range (`0` when nothing is left).
    pub fn prune_missing(&mut self) -> usize {
        let active_path = self.active_file().map(|f| f.path.clone());
        let before = self.open_files.len();
        self.open_files.retain(|f| f.path.exists());
        let removed = before - self.open_files.len();
        if removed == 0 {
            return 0;
        }

        self.active_index =
            match active_path.and_then(|p| self.open_files.iter().position(|f| f.path == p)) {
                Some(index) => index,
                None => self.open_files.len().saturating_sub(1),
            };
        removed
    }

    /// Push a path onto the recent-files list, de-duplicating and capping it.
    pub fn push_recent(&mut self, path: &PathBuf, max: usize) {
        self.recent_files.retain(|p| p != path);
        self.recent_files.insert(0, path.clone());
        self.recent_files.truncate(max);
    }

    /// Push a path onto the recent-files list using the default cap.
    pub fn touch_recent(&mut self, path: &PathBuf) {
        self.push_recent(path, RECENT_MAX);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::paths::temp_dir;

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
        let dir = temp_dir("session-missing");
        let s = SessionState::load_from(&dir.join("session.json")).unwrap_or_default();
        assert!(s.open_files.is_empty());
        assert_eq!(s.window.width, 1000);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn touch_recent_caps_the_list_at_twenty() {
        let mut s = SessionState::default();
        for i in 0..25 {
            s.touch_recent(&PathBuf::from(format!("/f{}", i)));
        }
        assert_eq!(s.recent_files.len(), RECENT_MAX);
        assert_eq!(s.recent_files[0], PathBuf::from("/f24"));
    }

    #[test]
    fn active_file_returns_the_entry_at_the_active_index() {
        let mut s = SessionState::default();
        s.open_files.push(SessionFile {
            path: PathBuf::from("/tmp/a.rs"),
            ..SessionFile::default()
        });
        s.active_index = 0;
        assert_eq!(
            s.active_file().map(|f| f.path.clone()),
            Some(PathBuf::from("/tmp/a.rs"))
        );
    }

    #[test]
    fn active_file_is_none_when_the_index_is_out_of_range() {
        let mut s = SessionState::default();
        s.open_files.push(SessionFile {
            path: PathBuf::from("/tmp/a.rs"),
            ..SessionFile::default()
        });
        s.active_index = 1;
        assert!(s.active_file().is_none());
        s.active_index = 99;
        assert!(s.active_file().is_none());
        s.open_files.clear();
        s.active_index = 0;
        assert!(s.active_file().is_none());
    }

    #[test]
    fn prune_missing_removes_gone_files_and_keeps_the_active_file_active() {
        let dir = temp_dir("session-prune");
        let present = dir.join("present.txt");
        let other = dir.join("other.txt");
        std::fs::write(&present, "x").expect("write");
        std::fs::write(&other, "y").expect("write");
        let mut s = SessionState {
            open_files: vec![
                SessionFile {
                    path: dir.join("gone.txt"),
                    ..SessionFile::default()
                },
                SessionFile {
                    path: present.clone(),
                    ..SessionFile::default()
                },
                SessionFile {
                    path: other,
                    ..SessionFile::default()
                },
            ],
            active_index: 1,
            ..SessionState::default()
        };

        assert_eq!(s.prune_missing(), 1);
        assert_eq!(s.open_files.len(), 2);
        assert_eq!(s.open_files[0].path, present);
        assert_eq!(s.active_index, 0);
        assert_eq!(s.active_file().map(|f| f.path.clone()), Some(present));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_missing_clamps_the_index_when_the_active_file_is_gone() {
        let dir = temp_dir("session-prune-clamp");
        let present = dir.join("present.txt");
        std::fs::write(&present, "x").expect("write");
        let mut s = SessionState {
            open_files: vec![
                SessionFile {
                    path: present.clone(),
                    ..SessionFile::default()
                },
                SessionFile {
                    path: dir.join("gone.txt"),
                    ..SessionFile::default()
                },
            ],
            active_index: 1,
            ..SessionState::default()
        };

        assert_eq!(s.prune_missing(), 1);
        assert_eq!(s.open_files.len(), 1);
        assert_eq!(s.active_index, 0);
        assert_eq!(s.active_file().map(|f| f.path.clone()), Some(present));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_missing_clamps_the_index_when_every_file_is_gone() {
        let mut s = SessionState {
            open_files: vec![SessionFile {
                path: PathBuf::from("/definitely/not/here.txt"),
                ..SessionFile::default()
            }],
            active_index: 0,
            ..SessionState::default()
        };

        assert_eq!(s.prune_missing(), 1);
        assert!(s.open_files.is_empty());
        assert_eq!(s.active_index, 0);
        assert!(s.active_file().is_none());
    }

    #[test]
    fn prune_missing_is_a_no_op_when_every_file_exists() {
        let dir = temp_dir("session-prune-noop");
        let present = dir.join("present.txt");
        std::fs::write(&present, "x").expect("write");
        let mut s = SessionState {
            open_files: vec![SessionFile {
                path: present,
                ..SessionFile::default()
            }],
            active_index: 0,
            ..SessionState::default()
        };

        assert_eq!(s.prune_missing(), 0);
        assert_eq!(s.open_files.len(), 1);
        assert_eq!(s.active_index, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = temp_dir("session-roundtrip");
        let path = dir.join("session.json");
        let mut s = SessionState::default();
        s.open_files.push(SessionFile {
            path: PathBuf::from("/tmp/a.rs"),
            caret: 7,
            scroll: 3.5,
        });
        s.active_index = 0;
        s.recent_files.push(PathBuf::from("/tmp/a.rs"));
        s.window = WindowState {
            width: 900,
            height: 600,
        };
        s.prefs.font_size = 12;

        s.save_to(&path).expect("save session");
        let back = SessionState::load_from(&path).expect("load session");
        assert_eq!(back, s);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_strict_reports_a_corrupt_session() {
        let dir = temp_dir("session-corrupt");
        let path = dir.join("session.json");
        std::fs::write(&path, "not json at all").expect("write");
        assert!(SessionState::load_from(&path).is_err());
        // The lenient loader still yields the defaults.
        assert_eq!(
            SessionState::load_from(&path).unwrap_or_default(),
            SessionState::default()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
