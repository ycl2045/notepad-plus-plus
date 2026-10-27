//! Bookmarks: the per-tab set of bookmarked lines and the navigation rules.
//!
//! A bookmark is a 0-based line index. The set is sorted and free of
//! duplicates, so the "next" and "previous" rules are pure order questions.
//!
//! This module is deliberately free of GTK: every rule can be unit tested
//! without a window, and the editor only has to translate between the caret
//! and a line index.
//!
//! OWNED BY: gap-closure unit A (bookmarks).

use std::collections::BTreeSet;

/// The bookmarked lines of one document, sorted and without duplicates.
pub type BookmarkSet = BTreeSet<usize>;

/// Toggle the bookmark on `line`; returns whether the line is bookmarked
/// afterwards.
pub fn toggle(set: &mut BookmarkSet, line: usize) -> bool {
    if set.remove(&line) {
        false
    } else {
        set.insert(line);
        true
    }
}

/// The smallest valid bookmark strictly after `from_line`.
///
/// Lines at or past `line_count` are stale and are skipped. When there is no
/// bookmark after `from_line`, the search wraps around to the smallest valid
/// bookmark. `None` when the set holds no valid bookmark at all.
pub fn next(set: &BookmarkSet, from_line: usize, line_count: usize) -> Option<usize> {
    let mut smallest = None;
    for &line in set {
        if line >= line_count {
            // A stale line: the document shrank since the bookmark was set.
            continue;
        }
        if line > from_line {
            // The set is sorted, so this is the smallest one after `from_line`.
            return Some(line);
        }
        if smallest.is_none() {
            smallest = Some(line);
        }
    }
    smallest
}

/// The largest valid bookmark strictly before `from_line`.
///
/// Lines at or past `line_count` are stale and are skipped. When there is no
/// bookmark before `from_line`, the search wraps around to the largest valid
/// bookmark. `None` when the set holds no valid bookmark at all.
pub fn previous(set: &BookmarkSet, from_line: usize, line_count: usize) -> Option<usize> {
    let mut largest = None;
    let mut predecessor = None;
    for &line in set {
        if line >= line_count {
            continue;
        }
        largest = Some(line);
        if line < from_line {
            // The set is sorted, so the last one wins.
            predecessor = Some(line);
        }
    }
    predecessor.or(largest)
}

/// The bookmarks of a persisted list: sorted, without duplicates and without
/// lines that the document does not have any more.
pub fn sanitise(lines: &[usize], line_count: usize) -> BookmarkSet {
    lines
        .iter()
        .copied()
        .filter(|&line| line < line_count)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_of(lines: &[usize]) -> BookmarkSet {
        lines.iter().copied().collect()
    }

    #[test]
    fn toggle_adds_then_removes_a_line() {
        let mut set = BookmarkSet::new();
        assert!(toggle(&mut set, 3));
        assert!(set.contains(&3));
        assert!(!toggle(&mut set, 3));
        assert!(set.is_empty());
    }

    #[test]
    fn next_moves_to_the_following_bookmark() {
        let set = set_of(&[2, 5, 9]);
        assert_eq!(next(&set, 0, 10), Some(2));
        assert_eq!(next(&set, 2, 10), Some(5));
        assert_eq!(next(&set, 4, 10), Some(5));
        assert_eq!(next(&set, 8, 10), Some(9));
    }

    #[test]
    fn next_wraps_to_the_first_bookmark() {
        let set = set_of(&[2, 5]);
        assert_eq!(next(&set, 5, 10), Some(2));
        assert_eq!(next(&set, 99, 10), Some(2));
    }

    #[test]
    fn previous_moves_to_the_preceding_bookmark() {
        let set = set_of(&[2, 5, 9]);
        assert_eq!(previous(&set, 9, 10), Some(5));
        assert_eq!(previous(&set, 5, 10), Some(2));
        assert_eq!(previous(&set, 8, 10), Some(5));
    }

    #[test]
    fn previous_wraps_to_the_last_bookmark() {
        let set = set_of(&[2, 5]);
        assert_eq!(previous(&set, 2, 10), Some(5));
        assert_eq!(previous(&set, 0, 10), Some(5));
    }

    #[test]
    fn navigation_returns_none_for_an_empty_set() {
        let set = BookmarkSet::new();
        assert_eq!(next(&set, 0, 10), None);
        assert_eq!(previous(&set, 0, 10), None);
    }

    #[test]
    fn navigation_skips_stale_lines() {
        let set = set_of(&[2, 99]);
        assert_eq!(next(&set, 0, 10), Some(2));
        assert_eq!(next(&set, 2, 10), Some(2));
        assert_eq!(previous(&set, 9, 10), Some(2));
        assert_eq!(previous(&set, 2, 10), Some(2));

        let stale_only = set_of(&[99]);
        assert_eq!(next(&stale_only, 0, 10), None);
        assert_eq!(previous(&stale_only, 0, 10), None);
    }

    #[test]
    fn sanitise_sorts_dedupes_and_drops_out_of_range_lines() {
        let set = sanitise(&[7, 1, 1, 99], 10);
        assert_eq!(set, set_of(&[1, 7]));
        assert!(sanitise(&[7, 1, 1, 99], 0).is_empty());
    }
}
