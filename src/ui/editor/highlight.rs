//! Syntax highlighting via syntect (plan §3.5).
//!
//! Contract: `Highlighter` owns the `SyntaxSet` and the LRU of per-line spans.
//! Callers ask for the spans of the *visible* rows only; the cache invalidates
//! on `Document::epoch` bumps. Unknown extensions fall back to plain text.
//!
//! The cache is capped at [`CACHE_CAP`] lines so that scrolling through a
//! huge file cannot grow it without bound. Eviction is insertion-ordered:
//! the oldest entry is dropped first.
//!
//! OWNED BY: U1 (contract) → U5 (syntect wiring, ≥20 languages, perf budget).

use std::collections::{HashMap, VecDeque};

use syntect::highlighting::Style as SyntectStyle;
use syntect::parsing::{SyntaxReference, SyntaxSet};

/// Maximum number of per-line span entries kept in the cache.
pub const CACHE_CAP: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b }
    }
    // `Color`'s public API is frozen; the `to_*` + `Copy` receiver convention
    // cannot be applied without breaking existing callers.
    #[allow(clippy::wrong_self_convention)]
    pub fn to_rgba(&self) -> gtk4::gdk::RGBA {
        let mut c = gtk4::gdk::RGBA::new(0.0, 0.0, 0.0, 1.0);
        c.set_red(self.r as f32 / 255.0);
        c.set_green(self.g as f32 / 255.0);
        c.set_blue(self.b as f32 / 255.0);
        c
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub color: Color,
}

pub struct Highlighter {
    ps: SyntaxSet,
    cache: HashMap<usize, Vec<Span>>,
    order: VecDeque<usize>,
    epoch: u64,
    language: String,
    syntax_index: Option<usize>,
    /// Whether the dark syntax palette is used.
    dark: bool,
}

/// Position of the syntax named `name` inside `ps`, or `None` when the set has
/// no such syntax. Mirrors `SyntaxSet::find_syntax_by_name`, which searches the
/// syntax list in reverse.
fn syntax_index_of(ps: &SyntaxSet, name: &str) -> Option<usize> {
    ps.syntaxes().iter().rposition(|s| s.name == name)
}

impl Highlighter {
    /// Build with the bundled default syntaxes (syntect `default-fancy`).
    pub fn new() -> Self {
        let ps = SyntaxSet::load_defaults_newlines();
        let language = String::from("Plain Text");
        let syntax_index = syntax_index_of(&ps, &language);
        Highlighter {
            ps,
            cache: HashMap::new(),
            order: VecDeque::new(),
            epoch: u64::MAX,
            language,
            syntax_index,
            dark: true,
        }
    }

    /// Switch between the dark and the light syntax palette.
    pub fn set_dark(&mut self, dark: bool) {
        if self.dark != dark {
            self.dark = dark;
            self.clear_cache();
        }
    }

    pub fn set_language(&mut self, name: &str) {
        if self.language != name {
            let index = syntax_index_of(&self.ps, name);
            self.language = name.to_string();
            self.syntax_index = index;
            self.clear_cache();
            self.epoch = u64::MAX;
        }
    }

    pub fn language(&self) -> &str {
        &self.language
    }

    /// Syntect syntax name for `path`, resolved by file extension. Unknown
    /// extensions fall back to "Plain Text".
    pub fn syntax_for_path(path: &std::path::Path) -> String {
        crate::syntax::ext_to_syntax(path)
    }

    /// Number of lines currently held in the span cache.
    pub fn cached_line_count(&self) -> usize {
        self.cache.len()
    }

    /// Whether `line` currently has a cached span entry.
    pub fn is_cached(&self, line: usize) -> bool {
        self.cache.contains_key(&line)
    }

    /// Spans for `line` (without trailing newline). Cached per line; the caller
    /// passes the live document so the epoch check stays outside the cache key.
    pub fn line_spans(
        &mut self,
        doc: &crate::ui::editor::buffer::Document,
        line: usize,
    ) -> Vec<Span> {
        if self.epoch != doc.epoch {
            self.clear_cache();
            self.epoch = doc.epoch;
        }
        if let Some(spans) = self.cache.get(&line) {
            return spans.clone();
        }
        let spans = self.compute(doc, line);
        self.cache_insert(line, spans.clone());
        spans
    }

    fn clear_cache(&mut self) {
        self.cache.clear();
        self.order.clear();
    }

    fn cache_insert(&mut self, line: usize, spans: Vec<Span>) {
        if self.cache.insert(line, spans).is_some() {
            self.order.retain(|&l| l != line);
        }
        self.order.push_back(line);
        while self.cache.len() > CACHE_CAP {
            match self.order.pop_front() {
                Some(oldest) => {
                    self.cache.remove(&oldest);
                }
                None => break,
            }
        }
    }

    fn syntax_for_text<'a>(&'a self, text: &str) -> &'a SyntaxReference {
        match self.syntax_index {
            Some(index) => &self.ps.syntaxes()[index],
            None => self
                .ps
                .find_syntax_by_first_line(text)
                .unwrap_or_else(|| self.ps.find_syntax_plain_text()),
        }
    }

    fn compute(&self, doc: &crate::ui::editor::buffer::Document, line: usize) -> Vec<Span> {
        let text = doc.line(line);
        let text = text.trim_end_matches(['\n', '\r']);
        let syntax = self.syntax_for_text(text);

        let mut h = syntect::easy::HighlightLines::new(syntax, syntax_themes::get(self.dark));
        let mut spans: Vec<Span> = Vec::new();
        let mut offset = 0usize;
        for piece in text.split_inclusive('\n') {
            let owned: String = piece.to_string();
            let highlighted: Vec<(SyntectStyle, &str)> =
                h.highlight_line(&owned, &self.ps).unwrap_or_default();
            for (style, piece_text) in highlighted {
                let len = piece_text.len();
                if len == 0 {
                    continue;
                }
                spans.push(Span {
                    start: offset,
                    end: offset + len,
                    color: Color::new(style.foreground.r, style.foreground.g, style.foreground.b),
                });
                offset += len;
            }
        }
        spans
    }
}

static DARK_THEME: std::sync::OnceLock<syntect::highlighting::Theme> = std::sync::OnceLock::new();
static LIGHT_THEME: std::sync::OnceLock<syntect::highlighting::Theme> = std::sync::OnceLock::new();

mod syntax_themes {
    use super::*;

    /// The syntax palette for the given mode.
    pub fn get(dark: bool) -> &'static syntect::highlighting::Theme {
        let cell = if dark { &DARK_THEME } else { &LIGHT_THEME };
        cell.get_or_init(|| {
            let name = if dark {
                "base16-ocean.dark"
            } else {
                "base16-ocean.light"
            };
            syntect::highlighting::ThemeSet::load_defaults()
                .themes
                .get(name)
                .cloned()
                .unwrap_or_else(syntect::highlighting::Theme::default)
        })
    }
}

impl Default for Highlighter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::editor::buffer::{Document, EditKind};
    use std::path::Path;
    use std::time::Instant;

    fn rust_source(lines: usize) -> String {
        let mut text = String::with_capacity(lines * 40);
        for i in 0..lines {
            text.push_str("fn function_");
            text.push_str(&i.to_string());
            text.push_str("() { let x = 1; }\n");
        }
        text
    }

    #[test]
    fn produces_spans_for_code() {
        let doc = Document::with_text("fn main() { let x = 1; }\n");
        let mut h = Highlighter::new();
        h.set_language("Rust");
        let spans = h.line_spans(&doc, 0);
        assert!(!spans.is_empty(), "Rust line should be highlighted");
        let total: usize = spans.iter().map(|s| s.end - s.start).sum();
        assert!(total <= "fn main() { let x = 1; }".len());
    }

    #[test]
    fn unknown_language_still_returns_something() {
        let doc = Document::with_text("plain text here\n");
        let mut h = Highlighter::new();
        h.set_language("NoSuchLanguage");
        let spans = h.line_spans(&doc, 0);
        assert!(!spans.is_empty());
    }

    #[test]
    fn cache_invalidates_on_new_epoch() {
        let mut doc = Document::with_text("fn a() {}\n");
        let mut h = Highlighter::new();
        h.set_language("Rust");
        let first = h.line_spans(&doc, 0);
        doc.replace(0, doc.len_bytes(), "let x = 1;\n", EditKind::Insert);
        let second = h.line_spans(&doc, 0);
        assert_ne!(first.len(), second.len());
    }

    #[test]
    fn syntax_for_path_maps_extensions() {
        assert_eq!(Highlighter::syntax_for_path(Path::new("a.rs")), "Rust");
        assert_eq!(Highlighter::syntax_for_path(Path::new("a.PY")), "Python");
        assert_eq!(Highlighter::syntax_for_path(Path::new("main.rs")), "Rust");
        assert_eq!(
            Highlighter::syntax_for_path(Path::new("Makefile")),
            "Makefile"
        );
        assert_eq!(
            Highlighter::syntax_for_path(Path::new("a.zzzz")),
            "Plain Text"
        );
        assert_eq!(
            Highlighter::syntax_for_path(Path::new("noext")),
            "Plain Text"
        );
    }

    #[test]
    fn default_language_resolves_to_a_concrete_syntax() {
        let h = Highlighter::new();
        assert!(
            syntax_index_of(&h.ps, "Plain Text").is_some(),
            "the default language must resolve, else first-line detection would kick in"
        );
    }

    #[test]
    fn cache_evicts_oldest_line_when_full() {
        let doc = Document::with_text(&rust_source(CACHE_CAP + 64));
        let mut h = Highlighter::new();
        h.set_language("Rust");
        for line in 0..CACHE_CAP {
            let _ = h.line_spans(&doc, line);
        }
        assert_eq!(h.cached_line_count(), CACHE_CAP);
        assert!(h.is_cached(0));
        assert!(h.is_cached(CACHE_CAP - 1));

        let _ = h.line_spans(&doc, CACHE_CAP);
        assert_eq!(h.cached_line_count(), CACHE_CAP);
        assert!(!h.is_cached(0), "oldest line must be evicted");
        assert!(h.is_cached(CACHE_CAP), "newest line must be cached");
    }

    #[test]
    fn cache_is_cleared_on_language_change_and_epoch_bump() {
        let mut doc = Document::with_text("fn a() {}\n");
        let mut h = Highlighter::new();
        h.set_language("Rust");
        let _ = h.line_spans(&doc, 0);
        assert_eq!(h.cached_line_count(), 1);

        h.set_language("Python");
        assert_eq!(h.cached_line_count(), 0, "language change clears the cache");

        let _ = h.line_spans(&doc, 0);
        assert_eq!(h.cached_line_count(), 1);

        doc.replace(0, doc.len_bytes(), "let x = 1;\n", EditKind::Insert);
        let _ = h.line_spans(&doc, 1);
        assert!(!h.is_cached(0), "epoch bump invalidates cached lines");
    }

    #[test]
    fn warm_highlight_of_visible_lines_is_fast() {
        let doc = Document::with_text(&rust_source(5000));
        let mut h = Highlighter::new();
        h.set_language("Rust");

        let cold_start = Instant::now();
        for line in 0..100 {
            let _ = h.line_spans(&doc, line);
        }
        let cold = cold_start.elapsed();

        let warm_start = Instant::now();
        for line in 0..100 {
            let _ = h.line_spans(&doc, line);
        }
        let warm = warm_start.elapsed();

        eprintln!("highlight 5000-line doc: cold={cold:?} warm={warm:?}");
        assert!(
            warm.as_millis() < 50,
            "warm highlight of 100 visible lines took {warm:?}, budget is 50ms"
        );
    }
}
