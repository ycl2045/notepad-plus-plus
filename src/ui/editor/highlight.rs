//! Syntax highlighting via syntect (plan §3.5).
//!
//! Contract: `Highlighter` owns the `SyntaxSet` and the LRU of per-line spans.
//! Callers ask for the spans of the *visible* rows only; the cache invalidates
//! on `Document::epoch` bumps. Unknown extensions fall back to plain text.
//!
//! OWNED BY: U1 (contract) → U5 (syntect wiring, ≥20 languages, perf budget).

use std::collections::HashMap;

use syntect::highlighting::Style as SyntectStyle;
use syntect::parsing::SyntaxSet;

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
    epoch: u64,
    language: String,
}

impl Highlighter {
    /// Build with the bundled default syntaxes (syntect `default-fancy`).
    pub fn new() -> Self {
        Highlighter {
            ps: SyntaxSet::load_defaults_newlines(),
            cache: HashMap::new(),
            epoch: u64::MAX,
            language: String::from("Plain Text"),
        }
    }

    pub fn set_language(&mut self, name: &str) {
        if self.language != name {
            self.language = name.to_string();
            self.cache.clear();
            self.epoch = u64::MAX;
        }
    }

    pub fn language(&self) -> &str {
        &self.language
    }

    /// Spans for `line` (without trailing newline). Cached per line; the caller
    /// passes the live document so the epoch check stays outside the cache key.
    pub fn line_spans(&mut self, doc: &crate::ui::editor::buffer::Document, line: usize) -> Vec<Span> {
        if self.epoch != doc.epoch {
            self.cache.clear();
            self.epoch = doc.epoch;
        }
        if let Some(spans) = self.cache.get(&line) {
            return spans.clone();
        }
        let spans = self.compute(doc, line);
        self.cache.insert(line, spans.clone());
        spans
    }

    fn compute(&self, doc: &crate::ui::editor::buffer::Document, line: usize) -> Vec<Span> {
        let text = doc.line(line);
        let text = text.trim_end_matches(['\n', '\r']);
        let syntax = self
            .ps
            .find_syntax_by_name(&self.language)
            .or_else(|| self.ps.find_syntax_by_first_line(text))
            .unwrap_or_else(|| self.ps.find_syntax_plain_text());

        let mut h = syntect::easy::HighlightLines::new(syntax, default_theme::get());
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

static DEFAULT_THEME: std::sync::OnceLock<syntect::highlighting::Theme> = std::sync::OnceLock::new();

mod default_theme {
    use super::*;
    pub fn get() -> &'static syntect::highlighting::Theme {
        DEFAULT_THEME.get_or_init(|| {
            syntect::highlighting::ThemeSet::load_defaults()
                .themes
                .get("base16-ocean.dark")
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
}
