#![allow(dead_code)]
//! Shared filesystem locations for npad's persisted state.
//!
//! Everything that must survive a restart (preferences, session state) lives
//! under one directory, so there is a single place to look and a single place
//! to clean. This module owns that location and the atomic-write helper used
//! to persist state without ever leaving a half-written file behind.
//!
//! OWNED BY: U10.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Directory name used underneath the user's config root.
const APP_DIR: &str = "npad";

/// Counter used to give every temporary file a unique name.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Resolve the directory that holds npad's persisted state.
///
/// `XDG_CONFIG_HOME` wins when it is set and non-empty; otherwise
/// `$HOME/.config` is used. `npad` is always appended.
pub fn config_dir() -> PathBuf {
    config_dir_from(
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

/// Pure core of [`config_dir`], without environment lookups so the precedence
/// rules can be exercised deterministically.
fn config_dir_from(xdg_config_home: Option<PathBuf>, home: Option<PathBuf>) -> PathBuf {
    let root = xdg_config_home
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| {
            home.filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".config")
        });
    root.join(APP_DIR)
}

/// Create the config directory (and any missing parent) and return it.
pub fn ensure_config_dir() -> std::io::Result<PathBuf> {
    ensure_dir_at(&config_dir())
}

/// Create `dir` and any missing parent, then return it.
fn ensure_dir_at(dir: &Path) -> std::io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    Ok(dir.to_path_buf())
}

/// Write `contents` to `path` without ever leaving a half-written file behind.
///
/// The bytes are written to a temporary sibling file that is flushed to disk
/// and then renamed over the target. `rename` is atomic within a filesystem,
/// so a crash - or a failed write - leaves the previous content intact.
pub fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    fs::create_dir_all(parent_dir(path))?;
    let temp = temp_sibling(path);

    let written = (|| -> std::io::Result<()> {
        let mut file = File::create(&temp)?;
        file.write_all(contents)?;
        file.sync_all()?;
        Ok(())
    })();
    if let Err(err) = written {
        let _ = fs::remove_file(&temp);
        return Err(err);
    }

    if let Err(err) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(err);
    }
    Ok(())
}

/// Directory that holds `path`, defaulting to the current directory.
fn parent_dir(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

/// Unique temporary file next to `path`, used by [`write_atomic`].
fn temp_sibling(path: &Path) -> PathBuf {
    let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let stem = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| APP_DIR.to_string());
    parent_dir(path).join(format!(".{stem}.{pid}.{seq}.tmp"))
}

/// Create a unique temporary directory for a test and return its path.
#[cfg(test)]
pub(crate) fn temp_dir(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let path = std::env::temp_dir().join(format!("npad-{prefix}-{pid}-{seq}"));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("create temp dir");
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_dir_prefers_xdg_config_home() {
        let dir = config_dir_from(
            Some(PathBuf::from("/tmp/xdg")),
            Some(PathBuf::from("/home/user")),
        );
        assert_eq!(dir, PathBuf::from("/tmp/xdg").join("npad"));
    }

    #[test]
    fn config_dir_falls_back_to_home_config() {
        let dir = config_dir_from(None, Some(PathBuf::from("/home/user")));
        assert_eq!(
            dir,
            PathBuf::from("/home/user").join(".config").join("npad")
        );
    }

    #[test]
    fn config_dir_ignores_an_empty_xdg_config_home() {
        let dir = config_dir_from(Some(PathBuf::new()), Some(PathBuf::from("/home/user")));
        assert_eq!(
            dir,
            PathBuf::from("/home/user").join(".config").join("npad")
        );
    }

    #[test]
    fn config_dir_without_any_home_stays_relative() {
        let dir = config_dir_from(None, None);
        assert!(dir.ends_with("npad"));
        assert!(dir.is_relative());
    }

    #[test]
    fn config_dir_ends_with_the_app_directory() {
        assert!(config_dir().ends_with("npad"));
    }

    #[test]
    fn ensure_dir_at_creates_missing_parents() {
        let root = temp_dir("ensure");
        let target = root.join("a").join("b");
        let created = ensure_dir_at(&target).expect("ensure dir");
        assert_eq!(created, target);
        assert!(target.is_dir());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn write_atomic_creates_the_file_without_leaving_temp_files() {
        let root = temp_dir("atomic-new");
        let target = root.join("state.json");
        write_atomic(&target, b"hello").expect("write");
        assert_eq!(fs::read_to_string(&target).expect("read"), "hello");
        assert_eq!(temp_files_in(&root), 0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn write_atomic_replaces_existing_content() {
        let root = temp_dir("atomic-replace");
        let target = root.join("state.json");
        write_atomic(&target, b"first").expect("write first");
        write_atomic(&target, b"second").expect("write second");
        assert_eq!(fs::read_to_string(&target).expect("read"), "second");
        assert_eq!(temp_files_in(&root), 0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn write_atomic_keeps_the_previous_content_when_it_fails() {
        let root = temp_dir("atomic-fail");
        // A directory in place of the target makes the final rename fail.
        let target = root.join("state.json");
        fs::create_dir(&target).expect("create dir");
        assert!(write_atomic(&target, b"nope").is_err());
        assert!(target.is_dir());
        assert_eq!(temp_files_in(&root), 0);
        let _ = fs::remove_dir_all(&root);
    }

    /// Number of leftover temporary files in `dir`.
    fn temp_files_in(dir: &Path) -> usize {
        fs::read_dir(dir)
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .count()
    }
}
