//! Pure search/replace engine (no GTK) that backs the find & replace panel.
//!
//! OWNED BY: U8.
//!
//! # Coordinate system
//!
//! Every offset handled here is a **byte** offset into the document text, the
//! same coordinate system [`Document`] uses. A multi-byte character therefore
//! spans several offsets: `find_all` on `"你好"` reports `start == 0` and
//! `end == 6`. Callers must slice the document with the very same offsets.
//!
//! # Matching model
//!
//! Matches are produced by the standard **non-overlapping, left-to-right**
//! scan over the whole document - the scan performed by
//! [`str::match_indices`] for literal patterns and by `Regex::find_iter` for
//! regular expressions. Both scans are equivalent for equivalent patterns, so
//! a literal pattern and its escaped regex counterpart always agree.
//!
//! Literal patterns are matched byte for byte. Case-insensitive matching is
//! implemented by compiling the *escaped* literal as a case-insensitive
//! regular expression, which keeps the reported offsets exact (the haystack
//! is never lower-cased).
//!
//! # `from_byte` semantics
//!
//! `from_byte` is the caret position the search starts from. A match counts as
//! *after* the caret **iff `match.start >= from_byte`**; only the start
//! offset is compared. A match that *begins* before the caret but extends
//! past it is therefore **not** after the caret and is only reachable through
//! wrap-around. `from_byte` is clamped to the document length, so a caret
//! past the end simply yields no "after" matches.
//!
//! # `wrap`
//!
//! With `wrap == true` and `from_byte > 0`, the matches that begin before
//! `from_byte` are collected as well and appended **after** the later ones.
//! The result is therefore in *search order* (caret -> end of document ->
//! start of document -> caret) and not in ascending offset order. This is
//! what makes `find_next` wrap around to the first match of the document.

// Pure engine module: the panel that calls it is wired up in a later unit.
#![allow(dead_code)]

use crate::ui::editor::buffer::{Document, EditKind};

/// Options shared by every entry point of the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchOptions {
    /// When false, matching ignores case.
    pub case_sensitive: bool,
    /// When true, `pattern` is a regular expression, otherwise it is literal.
    pub regex: bool,
    /// When true, a search that finds nothing after the caret continues at
    /// the start of the document.
    pub wrap: bool,
}

impl Default for SearchOptions {
    /// Case-insensitive, literal, wrapping - the usual editor defaults.
    fn default() -> Self {
        SearchOptions {
            case_sensitive: false,
            regex: false,
            wrap: true,
        }
    }
}

/// A single match, as the half-open byte range `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    /// Byte offset of the first byte of the match.
    pub start: usize,
    /// Byte offset one past the last byte of the match.
    pub end: usize,
}

impl Match {
    fn new(start: usize, end: usize) -> Self {
        Match { start, end }
    }

    /// Length of the match in bytes.
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// True when the match is empty (only possible for patterns that match
    /// the empty string, which the engine rejects up front).
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

/// Errors reported by the search engine.
#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    /// The pattern is not a valid regular expression.
    #[error("bad search pattern: {0}")]
    BadPattern(String),
}

/// Compiled pattern, ready to be scanned over a haystack.
enum Matcher {
    /// Case-sensitive literal needle.
    Literal(String),
    /// Compiled regular expression. Also used for case-insensitive literals,
    /// where the needle is escaped before compilation.
    Regex(regex::Regex),
}

impl Matcher {
    /// Lazily yields the non-overlapping, left-to-right matches of `text`.
    fn find_iter<'t>(&'t self, text: &'t str) -> Box<dyn Iterator<Item = Match> + 't> {
        match self {
            Matcher::Literal(needle) => Box::new(
                text.match_indices(needle.as_str())
                    .map(|(start, found)| Match::new(start, start + found.len())),
            ),
            Matcher::Regex(re) => {
                Box::new(re.find_iter(text).map(|m| Match::new(m.start(), m.end())))
            }
        }
    }
}

/// Compiles `pattern` into a [`Matcher`] according to `options`.
fn build_matcher(pattern: &str, options: &SearchOptions) -> Result<Matcher, SearchError> {
    if options.regex {
        Ok(Matcher::Regex(compile(pattern, options.case_sensitive)?))
    } else if options.case_sensitive {
        Ok(Matcher::Literal(pattern.to_string()))
    } else {
        // A case-insensitive literal is compiled as an escaped regex so that
        // the reported offsets stay exact.
        Ok(Matcher::Regex(compile(&regex::escape(pattern), false)?))
    }
}

/// Compiles a regular expression, honouring the case sensitivity flag.
fn compile(pattern: &str, case_sensitive: bool) -> Result<regex::Regex, SearchError> {
    regex::RegexBuilder::new(pattern)
        .case_insensitive(!case_sensitive)
        .build()
        .map_err(|err| SearchError::BadPattern(err.to_string()))
}

/// Every match at or after `from_byte` (plus the earlier ones when `wrap` is
/// enabled), in search order.
///
/// An empty pattern matches nothing (instead of matching at every position),
/// and a pattern that does not occur yields an empty list. Only a malformed
/// regular expression is an error.
///
/// See the module documentation for the exact `from_byte` and `wrap` rules.
pub fn find_all(
    doc: &Document,
    pattern: &str,
    options: &SearchOptions,
    from_byte: usize,
) -> Result<Vec<Match>, SearchError> {
    if pattern.is_empty() {
        return Ok(Vec::new());
    }
    let matcher = build_matcher(pattern, options)?;
    let text = doc.text();
    let from = from_byte.min(text.len());

    let mut later: Vec<Match> = Vec::new();
    let mut earlier: Vec<Match> = Vec::new();
    for m in matcher.find_iter(&text) {
        if m.start >= from {
            later.push(m);
        } else {
            earlier.push(m);
        }
    }
    if options.wrap && from > 0 {
        later.extend(earlier);
    }
    Ok(later)
}

/// The next match at or after `from_byte`.
///
/// With `wrap` enabled the search continues at the start of the document when
/// no match follows the caret; without `wrap` the result is `None` in that
/// case.
pub fn find_next(
    doc: &Document,
    pattern: &str,
    options: &SearchOptions,
    from_byte: usize,
) -> Result<Option<Match>, SearchError> {
    if pattern.is_empty() {
        return Ok(None);
    }
    let matcher = build_matcher(pattern, options)?;
    let text = doc.text();
    let from = from_byte.min(text.len());

    let mut first_before: Option<Match> = None;
    for m in matcher.find_iter(&text) {
        if m.start >= from {
            return Ok(Some(m));
        }
        if first_before.is_none() {
            first_before = Some(m);
        }
    }
    if options.wrap && from > 0 {
        Ok(first_before)
    } else {
        Ok(None)
    }
}

/// The last match that starts before `from_byte`.
///
/// With `wrap` enabled the search continues at the end of the document when
/// no match starts before `from_byte`.
pub fn find_previous(
    doc: &Document,
    pattern: &str,
    options: &SearchOptions,
    from_byte: usize,
) -> Result<Option<Match>, SearchError> {
    if pattern.is_empty() {
        return Ok(None);
    }
    let matcher = build_matcher(pattern, options)?;
    let text = doc.text();
    let from = from_byte.min(text.len());

    let mut last_before = None;
    let mut last_any = None;
    for m in matcher.find_iter(&text) {
        last_any = Some(m);
        if m.start < from {
            last_before = Some(m);
        }
    }
    if last_before.is_some() {
        return Ok(last_before);
    }
    if options.wrap { Ok(last_any) } else { Ok(None) }
}

/// Replaces every occurrence of `pattern` with `replace` and returns the
/// number of replacements.///
/// The replacements are applied **back to front** (highest offset first) so
/// that the byte offsets of the not yet applied matches stay valid. Every
/// replacement is applied through [`Document::replace`], so the undo stack
/// records them; they are stored as [`EditKind::Structural`] edits, which
/// never coalesce - undoing a replace-all therefore steps through the
/// individual replacements.
///
/// In regex mode `replace` may reference capture groups (`$1`, `${name}`);
/// a reference to a group that does not exist expands to the empty string.
/// In literal mode `replace` is used verbatim.
pub fn replace_all(
    doc: &mut Document,
    pattern: &str,
    replace: &str,
    options: &SearchOptions,
) -> Result<usize, SearchError> {
    if pattern.is_empty() {
        return Ok(0);
    }
    let matcher = build_matcher(pattern, options)?;
    let text = doc.text();

    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    match &matcher {
        Matcher::Literal(_) => {
            for m in matcher.find_iter(&text) {
                edits.push((m.start, m.end, replace.to_string()));
            }
        }
        Matcher::Regex(re) => {
            for caps in re.captures_iter(&text) {
                let Some(whole) = caps.get(0) else {
                    continue;
                };
                let mut expanded = String::new();
                caps.expand(replace, &mut expanded);
                edits.push((whole.start(), whole.end(), expanded));
            }
        }
    }

    let count = edits.len();
    for (start, end, new) in edits.into_iter().rev() {
        doc.replace(start, end - start, &new, EditKind::Structural);
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Document {
        Document::with_text(text)
    }

    fn literal(case_sensitive: bool, wrap: bool) -> SearchOptions {
        SearchOptions {
            case_sensitive,
            regex: false,
            wrap,
        }
    }

    fn regex(case_sensitive: bool, wrap: bool) -> SearchOptions {
        SearchOptions {
            case_sensitive,
            regex: true,
            wrap,
        }
    }

    #[test]
    fn literal_find_reports_byte_offsets() {
        let d = doc("hello world");
        let found = find_all(&d, "world", &literal(true, false), 0).unwrap();
        assert_eq!(found, vec![Match { start: 6, end: 11 }]);
        assert_eq!(d.slice(found[0].start, found[0].end), "world");
    }

    #[test]
    fn find_previous_returns_the_last_match_before_the_offset() {
        let d = doc("ab ab ab");
        let found = find_previous(&d, "ab", &literal(true, false), 5).unwrap();
        assert_eq!(found, Some(Match { start: 3, end: 5 }));
    }

    #[test]
    fn find_previous_wraps_to_the_last_match_of_the_document() {
        let d = doc("ab ab ab");
        let found = find_previous(&d, "ab", &literal(true, true), 0).unwrap();
        assert_eq!(found, Some(Match { start: 6, end: 8 }));
    }

    #[test]
    fn find_previous_without_wrap_stops_at_the_start_of_the_document() {
        let d = doc("ab ab ab");
        let found = find_previous(&d, "ab", &literal(true, false), 0).unwrap();
        assert_eq!(found, None);
    }

    #[test]
    fn find_previous_finds_nothing_for_an_empty_pattern() {
        let d = doc("hello");
        assert_eq!(
            find_previous(&d, "", &literal(true, true), 0).unwrap(),
            None
        );
    }

    #[test]
    fn literal_find_is_case_insensitive_by_default() {
        let d = doc("Ab ab AB");
        let found = find_all(&d, "ab", &SearchOptions::default(), 0).unwrap();
        assert_eq!(
            found,
            vec![
                Match { start: 0, end: 2 },
                Match { start: 3, end: 5 },
                Match { start: 6, end: 8 },
            ]
        );
    }

    #[test]
    fn case_sensitive_find_distinguishes_ab_from_ab() {
        let d = doc("Ab ab AB");
        let found = find_all(&d, "ab", &literal(true, false), 0).unwrap();
        assert_eq!(found, vec![Match { start: 3, end: 5 }]);
    }

    #[test]
    fn regex_find_matches_the_pattern() {
        let d = doc("a1 b22 c333");
        let found = find_all(&d, r"\d+", &regex(true, false), 0).unwrap();
        assert_eq!(
            found,
            vec![
                Match { start: 1, end: 2 },
                Match { start: 4, end: 6 },
                Match { start: 8, end: 11 },
            ]
        );
    }

    #[test]
    fn find_all_only_reports_matches_at_or_after_from_byte() {
        let d = doc("one two one");
        let found = find_all(&d, "one", &literal(true, false), 3).unwrap();
        assert_eq!(found, vec![Match { start: 8, end: 11 }]);
    }

    #[test]
    fn find_all_with_wrap_appends_earlier_matches() {
        let d = doc("one two one");
        let found = find_all(&d, "one", &literal(true, true), 4).unwrap();
        assert_eq!(
            found,
            vec![Match { start: 8, end: 11 }, Match { start: 0, end: 3 }]
        );
    }

    #[test]
    fn find_next_wraps_around_to_the_first_match() {
        let d = doc("one two one");
        let options = literal(true, true);
        assert_eq!(
            find_next(&d, "one", &options, 0).unwrap(),
            Some(Match { start: 0, end: 3 })
        );
        assert_eq!(
            find_next(&d, "one", &options, 4).unwrap(),
            Some(Match { start: 8, end: 11 })
        );
        // Nothing follows the caret: wrap back to the first match.
        assert_eq!(
            find_next(&d, "one", &options, 11).unwrap(),
            Some(Match { start: 0, end: 3 })
        );
    }

    #[test]
    fn find_next_without_wrap_returns_none_when_nothing_follows() {
        let d = doc("one two one");
        let options = literal(true, false);
        assert_eq!(
            find_next(&d, "one", &options, 4).unwrap(),
            Some(Match { start: 8, end: 11 })
        );
        assert_eq!(find_next(&d, "one", &options, 11).unwrap(), None);
    }

    #[test]
    fn replace_all_replaces_every_match_and_returns_the_count() {
        let mut d = doc("one two one");
        let n = replace_all(&mut d, "one", "1", &literal(true, false)).unwrap();
        assert_eq!(n, 2);
        assert_eq!(d.text(), "1 two 1");
    }

    #[test]
    fn replace_all_expands_regex_backreferences() {
        let mut d = doc("user@host and admin@box");
        let n = replace_all(&mut d, r"(\w+)@(\w+)", "$2@$1", &regex(true, false)).unwrap();
        assert_eq!(n, 2);
        assert_eq!(d.text(), "host@user and box@admin");
    }

    #[test]
    fn replace_all_literal_replacement_is_verbatim() {
        let mut d = doc("a1");
        replace_all(&mut d, "1", "$1", &literal(true, false)).unwrap();
        assert_eq!(d.text(), "a$1");
    }

    #[test]
    fn replace_all_is_case_insensitive_when_asked() {
        let mut d = doc("Ab ab");
        let n = replace_all(&mut d, "AB", "x", &literal(false, false)).unwrap();
        assert_eq!(n, 2);
        assert_eq!(d.text(), "x x");
    }

    #[test]
    fn replace_all_edits_are_recorded_in_the_undo_stack() {
        let mut d = doc("one two one");
        replace_all(&mut d, "one", "1", &literal(true, false)).unwrap();
        assert!(d.can_undo());
        // Back-to-front application means the undo stack unwinds from the
        // lowest offset upwards.
        d.undo();
        assert_eq!(d.text(), "one two 1");
        d.undo();
        assert_eq!(d.text(), "one two one");
        assert!(!d.can_undo());
    }

    #[test]
    fn empty_pattern_yields_no_matches() {
        let d = doc("hello");
        assert!(
            find_all(&d, "", &literal(true, false), 0)
                .unwrap()
                .is_empty()
        );
        assert_eq!(find_next(&d, "", &literal(true, false), 0).unwrap(), None);

        let mut d = doc("hello");
        assert_eq!(
            replace_all(&mut d, "", "x", &literal(true, false)).unwrap(),
            0
        );
        assert_eq!(d.text(), "hello");
    }

    #[test]
    fn non_matching_pattern_yields_no_matches() {
        let d = doc("hello world");
        assert!(
            find_all(&d, "zzz", &literal(true, false), 0)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            find_next(&d, "zzz", &literal(true, false), 0).unwrap(),
            None
        );

        let mut d = doc("hello world");
        assert_eq!(
            replace_all(&mut d, "zzz", "x", &literal(true, false)).unwrap(),
            0
        );
        assert_eq!(d.text(), "hello world");
    }

    #[test]
    fn invalid_regex_yields_bad_pattern_error() {
        let d = doc("hello");
        let options = regex(true, false);
        match find_all(&d, "(unclosed", &options, 0) {
            Err(SearchError::BadPattern(msg)) => assert!(!msg.is_empty()),
            _ => panic!("expected a BadPattern error"),
        }
        assert!(find_next(&d, "[", &options, 0).is_err());

        let mut d = doc("hello");
        assert!(replace_all(&mut d, "(", "x", &options).is_err());
    }

    #[test]
    fn multibyte_offsets_are_byte_accurate() {
        let d = doc("a你b");
        let found = find_all(&d, "你", &literal(true, false), 0).unwrap();
        assert_eq!(found, vec![Match { start: 1, end: 4 }]);
        assert_eq!(d.slice(found[0].start, found[0].end), "你");

        // A caret inside the multi-byte character is still a byte offset: the
        // match starts before it and is therefore not reported.
        let found = find_all(&d, "你", &literal(true, false), 2).unwrap();
        assert!(found.is_empty());
    }

    #[test]
    fn replace_all_handles_multibyte_content() {
        let mut d = doc("a你b你c");
        let n = replace_all(&mut d, "你", "X", &literal(true, false)).unwrap();
        assert_eq!(n, 2);
        assert_eq!(d.text(), "aXbXc");
    }
}
