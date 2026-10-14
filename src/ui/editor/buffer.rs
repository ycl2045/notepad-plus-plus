//! Text buffer: rope-backed document with a structured undo stack.
//!
//! Contract (agreed in `.omo/plans/npad-mvp.md` §3.2):
//! - All coordinates are BYTE offsets into the rope.
//! - Every mutation bumps `epoch`, which downstream caches (rowmap, highlight)
//!   use as their invalidation key.
//! - `Edit` records both directions so undo/redo are exact inverses.
//!
//! OWNED BY: U1 (contract) → U2 (hardening: big-file perf, merge windows, tests).

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eol {
    Lf,
    Crlf,
    Cr,
}

impl Eol {
    pub fn as_str(self) -> &'static str {
        match self {
            Eol::Lf => "\n",
            Eol::Crlf => "\r\n",
            Eol::Cr => "\r",
        }
    }

    pub fn detect(sample: &str) -> Self {
        let crlf = sample.matches("\r\n").count();
        let cr = sample.matches('\r').count() - crlf;
        let lf = sample.matches('\n').count() - crlf;
        if crlf >= lf && crlf >= cr && crlf > 0 {
            Eol::Crlf
        } else if cr > lf && cr > 0 {
            Eol::Cr
        } else {
            Eol::Lf
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    Insert,
    Delete,
    /// User-visible structured boundary: never coalesce with neighbours.
    Structural,
}

#[derive(Debug, Clone)]
pub struct Edit {
    pub start: usize,
    pub old: String,
    pub new: String,
    pub kind: EditKind,
}

#[derive(Debug)]
pub struct Document {
    rope: ropey::Rope,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    pub epoch: u64,
    pub eol: Eol,
    pub path: Option<std::path::PathBuf>,
    pub dirty: bool,
    last_edit: Option<(Instant, EditKind)>,
}

/// Consecutive edits of the same kind within this window are coalesced into a
/// single undo entry (typing experience). Structural edits always break it.
pub const COALESCE_WINDOW: Duration = Duration::from_millis(800);

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub fn new() -> Self {
        Document {
            rope: ropey::Rope::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            epoch: 0,
            eol: Eol::Lf,
            path: None,
            dirty: false,
            last_edit: None,
        }
    }

    pub fn with_text(text: &str) -> Self {
        let mut d = Document::new();
        d.load(text);
        d
    }

    /// Replace the whole content, resetting undo history (used on load).
    pub fn load(&mut self, text: &str) {
        self.rope = ropey::Rope::from_str(text);
        self.eol = Eol::detect(text);
        self.undo.clear();
        self.redo.clear();
        self.epoch += 1;
        self.dirty = false;
        self.last_edit = None;
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    pub fn is_empty(&self) -> bool {
        self.rope.len_bytes() == 0
    }

    pub fn line_count(&self) -> usize {
        self.rope.len_lines().max(1)
    }

    /// Byte offset of the start of `line` (0-based). Clamped.
    pub fn line_to_byte(&self, line: usize) -> usize {
        let l = line.min(self.rope.len_lines());
        self.rope.line_to_byte(l).min(self.rope.len_bytes())
    }

    /// 0-based line index containing `byte`. Clamped to the last line.
    pub fn byte_to_line(&self, byte: usize) -> usize {
        let b = byte.min(self.rope.len_bytes());
        let line = self.rope.byte_to_line(b);
        line.min(self.rope.len_lines().saturating_sub(1))
    }

    /// Text of a single line including its trailing newline.
    pub fn line(&self, line: usize) -> String {
        if line >= self.rope.len_lines() {
            return String::new();
        }
        let start = self.rope.line_to_char(line);
        let end = self.rope.line_to_char(line + 1).min(self.rope.len_chars());
        self.rope
            .get_slice(start..end)
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    pub fn slice(&self, start: usize, end: usize) -> String {
        let len = self.rope.len_bytes();
        let lo = start.min(end).min(len);
        let hi = end.max(start).min(len).max(lo);
        match self.rope.get_byte_slice(lo..hi) {
            Some(s) => s.to_string(),
            None => String::new(),
        }
    }

    /// Replace `[start, start+old_len)` with `new` and record one undo entry.
    /// Returns the Edit that was pushed.
    pub fn replace(&mut self, start: usize, old_len: usize, new: &str, kind: EditKind) -> Edit {
        let start = start.min(self.rope.len_bytes());
        let end = (start + old_len).min(self.rope.len_bytes());
        let old: String = self.slice(start, end);

        let start_char = self.rope.byte_to_char(start);
        self.rope.remove(start_char..self.rope.byte_to_char(end));
        if !new.is_empty() {
            self.rope.insert(start_char, new);
        }

        let edit = Edit {
            start,
            old,
            new: new.to_string(),
            kind,
        };
        self.record(edit.clone());
        edit
    }

    fn record(&mut self, edit: Edit) {
        self.dirty = true;
        self.epoch += 1;
        self.redo.clear();

        let can_coalesce = matches!(
            (&self.last_edit, edit.kind),
            (Some((t, kind)), EditKind::Insert) | (Some((t, kind)), EditKind::Delete) if *kind == edit.kind && t.elapsed() <= COALESCE_WINDOW
        );

        if can_coalesce {
            if let Some(last) = self.undo.last_mut() {
                match edit.kind {
                    EditKind::Insert => {
                        let contiguous = last.start + last.new.len() == edit.start;
                        if contiguous && !last.old.is_empty() == false {
                            last.new.push_str(&edit.new);
                            self.last_edit = Some((Instant::now(), edit.kind));
                            return;
                        }
                    }
                    EditKind::Delete => {
                        if edit.start + edit.old.len() == last.start {
                            let mut old = edit.old;
                            old.push_str(&last.old);
                            last.old = old;
                            last.start = edit.start;
                            self.last_edit = Some((Instant::now(), edit.kind));
                            return;
                        }
                    }
                    EditKind::Structural => {}
                }
            }
        }

        self.last_edit = Some((Instant::now(), edit.kind));
        self.undo.push(edit);
    }

    pub fn undo(&mut self) -> Option<Edit> {
        let edit = self.undo.pop()?;
        let start_char = self.rope.byte_to_char(edit.start);
        self.rope
            .remove(start_char..self.rope.byte_to_char(edit.start + edit.new.len()));
        if !edit.old.is_empty() {
            self.rope.insert(start_char, &edit.old);
        }
        self.redo.push(edit.clone());
        self.epoch += 1;
        self.dirty = !self.undo.is_empty();
        self.last_edit = None;
        Some(edit)
    }

    pub fn redo(&mut self) -> Option<Edit> {
        let edit = self.redo.pop()?;
        let start_char = self.rope.byte_to_char(edit.start);
        self.rope
            .remove(start_char..self.rope.byte_to_char(edit.start + edit.old.len()));
        if !edit.new.is_empty() {
            self.rope.insert(start_char, &edit.new);
        }
        self.undo.push(edit.clone());
        self.epoch += 1;
        self.dirty = true;
        self.last_edit = None;
        Some(edit)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_undo_roundtrip() {
        let mut d = Document::new();
        d.replace(0, 0, "hello", EditKind::Insert);
        assert_eq!(d.text(), "hello");
        d.undo();
        assert_eq!(d.text(), "");
        d.redo();
        assert_eq!(d.text(), "hello");
    }

    #[test]
    fn delete_is_inverse_of_insert() {
        let mut d = Document::new();
        d.replace(0, 0, "abcdef", EditKind::Insert);
        d.replace(2, 2, "", EditKind::Delete);
        assert_eq!(d.text(), "abef");
        d.undo();
        assert_eq!(d.text(), "abcdef");
    }

    #[test]
    fn line_addressing_handles_multibyte() {
        let mut d = Document::new();
        d.replace(0, 0, "你好\nworld\n", EditKind::Insert);
        assert_eq!(d.line_count(), 3);
        assert_eq!(d.byte_to_line(0), 0);
        assert_eq!(d.byte_to_line(8), 1);
        assert_eq!(d.line_to_byte(2), 12);
        assert_eq!(d.line(0), "你好\n");
    }

    #[test]
    fn eol_detection() {
        assert_eq!(Eol::detect("a\r\nb\r\n"), Eol::Crlf);
        assert_eq!(Eol::detect("a\nb\n"), Eol::Lf);
        assert_eq!(Eol::detect("a\rb\r"), Eol::Cr);
    }
}
