//! The recent-files list behind the `File > Recent files` submenu.
//!
//! The helpers here are pure: they turn the persisted list of paths into the
//! rows of the submenu and drop the paths that cannot be opened any more.
//! Keeping them free of GTK is what makes the order of the list and the
//! filtering testable without a display.
//!
//! OWNED BY: U11 (menu).

use std::path::{Path, PathBuf};

/// The label of the disabled row that stands in for an empty list.
pub const EMPTY_LABEL: &str = "No recent files";

/// One row of the recent-files submenu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The text of the row: the full path of the file.
    pub label: String,
    /// The path the row opens.
    pub path: PathBuf,
}

/// The label of a recent-files row: the full path of the file.
pub fn label(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The paths of `paths` that are files, in the order they were given.
///
/// A directory, a broken link and a path that does not exist are all
/// dropped: only a file can be opened.
pub fn existing(paths: &[PathBuf]) -> Vec<PathBuf> {
    paths
        .iter()
        .filter(|path| path.is_file())
        .cloned()
        .collect()
}

/// The rows of the recent-files submenu.
///
/// The order of `paths` is kept, so the most recent file comes first, and
/// the paths that are not files are dropped.
pub fn menu_entries(paths: &[PathBuf]) -> Vec<Entry> {
    existing(paths)
        .into_iter()
        .map(|path| Entry {
            label: label(&path),
            path,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::paths::temp_dir;

    #[test]
    fn menu_entries_keep_recency_order() {
        let dir = temp_dir("recent-order");
        let first = dir.join("first.txt");
        let second = dir.join("second.txt");
        std::fs::write(&first, "1").unwrap();
        std::fs::write(&second, "2").unwrap();

        let entries = menu_entries(&[first.clone(), second.clone()]);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, first);
        assert_eq!(entries[1].path, second);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn menu_entries_drop_files_that_are_gone() {
        let dir = temp_dir("recent-gone");
        let present = dir.join("present.txt");
        std::fs::write(&present, "x").unwrap();
        let gone = dir.join("gone.txt");

        let entries = menu_entries(&[gone, present.clone()]);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, present);
        assert_eq!(entries[0].label, present.to_string_lossy().as_ref());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn menu_entries_are_empty_when_nothing_exists() {
        let dir = temp_dir("recent-empty");
        let gone = dir.join("gone.txt");

        let entries = menu_entries(&[gone]);

        assert!(entries.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn menu_entries_drop_directories() {
        let dir = temp_dir("recent-dir");
        let sub = dir.join("sub");
        std::fs::create_dir(&sub).unwrap();

        let entries = menu_entries(&[sub]);

        assert!(entries.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
