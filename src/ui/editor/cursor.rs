//! Cursor and selection model.
//!
//! Contract (plan §3.3): `anchor` and `head` are BYTE offsets. `head` is the
//! caret position; when `anchor == head` there is no selection. All movement
//! helpers take the `Document` so line addressing stays correct with wrapping
//! and multi-byte text. Column arithmetic uses char counts, never bytes.
//!
//! OWNED BY: U1 (contract) → U3 (hardening: word movement, block selection).

use crate::ui::editor::buffer::Document;

/// Characters that separate words: whitespace plus `/ \ - _`.
fn is_word_separator(c: char) -> bool {
    c.is_whitespace() || matches!(c, '/' | '\\' | '-' | '_')
}

/// The character starting at (or containing) `byte`, with its byte offset.
fn char_at(doc: &Document, byte: usize) -> Option<(usize, char)> {
    let len = doc.len_bytes();
    let b = byte.min(len);
    if b >= len {
        return None;
    }
    let idx = doc.byte_to_char(b);
    let start = doc.char_to_byte(idx);
    let end = doc.char_to_byte(idx + 1);
    let ch = doc.slice(start, end).chars().next()?;
    Some((start, ch))
}

/// The character ending right before `byte`, with its byte offset.
fn char_before(doc: &Document, byte: usize) -> Option<(usize, char)> {
    let len = doc.len_bytes();
    let b = byte.min(len);
    if b == 0 {
        return None;
    }
    let idx = doc.byte_to_char(b);
    if idx == 0 {
        return None;
    }
    let start = doc.char_to_byte(idx - 1);
    let end = doc.char_to_byte(idx);
    let ch = doc.slice(start, end).chars().next()?;
    Some((start, ch))
}

/// A line holding only whitespace (or nothing) separates paragraphs.
fn is_blank_line(doc: &Document, line: usize) -> bool {
    doc.line(line).trim().is_empty()
}

/// First line of the paragraph containing `line`.
fn paragraph_start(doc: &Document, line: usize) -> usize {
    let mut start = line;
    while start > 0 && !is_blank_line(doc, start - 1) {
        start -= 1;
    }
    start
}

/// Last line of the paragraph containing `line`.
fn paragraph_end(doc: &Document, line: usize) -> usize {
    let last = doc.line_count().saturating_sub(1);
    let mut end = line;
    while end < last && !is_blank_line(doc, end + 1) {
        end += 1;
    }
    end
}

/// Line reached by moving one paragraph up from `line`.
///
/// From a line inside a paragraph the start of that paragraph is reached;
/// from a blank line or a paragraph start the closest non-blank line above is
/// reached. The top of the document is reached from the first line.
fn paragraph_target_up(doc: &Document, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    if !is_blank_line(doc, line) {
        let start = paragraph_start(doc, line);
        if start < line {
            return start;
        }
    }
    let mut probe = line;
    while probe > 0 {
        probe -= 1;
        if !is_blank_line(doc, probe) {
            return probe;
        }
    }
    0
}

/// Line reached by moving one paragraph down from `line`.
fn paragraph_target_down(doc: &Document, line: usize) -> usize {
    let last = doc.line_count().saturating_sub(1);
    if line >= last {
        return last;
    }
    if !is_blank_line(doc, line) {
        let end = paragraph_end(doc, line);
        if end > line {
            return end;
        }
    }
    let mut probe = line;
    while probe < last {
        probe += 1;
        if !is_blank_line(doc, probe) {
            return probe;
        }
    }
    last
}

/// Selection covering the word around `byte`.
///
/// Returns an empty selection at `byte` when it sits on whitespace or on a
/// separator.
pub fn select_word_at(byte: usize, doc: &Document) -> Selection {
    let b = byte.min(doc.len_bytes());
    let Some((start, first)) = char_at(doc, b) else {
        return Selection::new(b);
    };
    if is_word_separator(first) {
        return Selection::new(b);
    }
    let mut word_start = start;
    while let Some((s, c)) = char_before(doc, word_start) {
        if is_word_separator(c) {
            break;
        }
        word_start = s;
    }
    let mut word_end = start + first.len_utf8();
    while let Some((s, c)) = char_at(doc, word_end) {
        if is_word_separator(c) {
            break;
        }
        word_end = s + c.len_utf8();
    }
    Selection {
        anchor: word_start,
        head: word_end,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn new(byte: usize) -> Self {
        Selection { anchor: byte, head: byte }
    }

    /// Ordered (start, end) of the selected byte range. Empty when collapsed.
    pub fn range(&self) -> (usize, usize) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    pub fn selected_text(&self, doc: &Document) -> String {
        let (s, e) = self.range();
        doc.slice(s, e)
    }

    /// Screen column (char index within line) of the caret.
    pub fn column(&self, doc: &Document) -> usize {
        let line = doc.byte_to_line(self.head);
        let line_start = doc.line_to_byte(line);
        doc.slice(line_start, self.head).chars().count()
    }

    pub fn line(&self, doc: &Document) -> usize {
        doc.byte_to_line(self.head)
    }

    pub fn collapse_to_head(&mut self) {
        self.anchor = self.head;
    }

    pub fn extend_to(&mut self, byte: usize) {
        self.head = byte.min(self.doc_len_bound_or_max(byte));
    }

    fn doc_len_bound_or_max(&self, _byte: usize) -> usize {
        usize::MAX
    }

    /// Move caret by `delta` bytes, dropping the selection.
    pub fn move_offset(&mut self, doc: &Document, delta: i64) {
        self.collapse_to_head();
        let target = (self.head as i64 + delta).clamp(0, doc.len_bytes() as i64) as usize;
        self.head = target;
        self.anchor = target;
    }

    pub fn move_to_line_start(&mut self, doc: &Document, extend: bool) {
        if !extend {
            self.collapse_to_head();
        }
        let line = doc.byte_to_line(self.head);
        let start = doc.line_to_byte(line);
        self.head = start;
    }

    pub fn move_to_line_end(&mut self, doc: &Document, extend: bool) {
        if !extend {
            self.collapse_to_head();
        }
        let line = doc.byte_to_line(self.head);
        let start = doc.line_to_byte(line);
        let text = doc.line(line);
        let without_eol = text.trim_end_matches(['\n', '\r']).len();
        self.head = start + without_eol;
    }

    /// Move the caret to the start of the previous word.
    ///
    /// Inside a word the caret jumps to the start of that word; on a word
    /// boundary the separator run is stepped over and the start of the
    /// preceding word is reached.
    pub fn move_to_word_left(&mut self, doc: &Document, extend: bool) {
        let mut pos = self.head.min(doc.len_bytes());
        if let Some((_, ch)) = char_before(doc, pos)
            && !is_word_separator(ch)
        {
            while let Some((start, c)) = char_before(doc, pos) {
                if is_word_separator(c) {
                    break;
                }
                pos = start;
            }
            self.set_head(pos, extend);
            return;
        }
        while let Some((start, c)) = char_before(doc, pos) {
            if !is_word_separator(c) {
                break;
            }
            pos = start;
        }
        while let Some((start, c)) = char_before(doc, pos) {
            if is_word_separator(c) {
                break;
            }
            pos = start;
        }
        self.set_head(pos, extend);
    }

    /// Move the caret to the end of the next word.
    ///
    /// Inside a word the caret jumps to the end of that word; on a word
    /// boundary the separator run is stepped over and the end of the following
    /// word is reached.
    pub fn move_to_word_right(&mut self, doc: &Document, extend: bool) {
        let mut pos = self.head.min(doc.len_bytes());
        if let Some((_, ch)) = char_at(doc, pos)
            && !is_word_separator(ch)
        {
            while let Some((start, c)) = char_at(doc, pos) {
                if is_word_separator(c) {
                    break;
                }
                pos = start + c.len_utf8();
            }
            self.set_head(pos, extend);
            return;
        }
        while let Some((start, c)) = char_at(doc, pos) {
            if !is_word_separator(c) {
                break;
            }
            pos = start + c.len_utf8();
        }
        while let Some((start, c)) = char_at(doc, pos) {
            if is_word_separator(c) {
                break;
            }
            pos = start + c.len_utf8();
        }
        self.set_head(pos, extend);
    }

    /// Move the caret to the first byte of the document.
    pub fn move_to_doc_start(&mut self, doc: &Document, extend: bool) {
        self.set_head(doc.line_to_byte(0), extend);
    }

    /// Move the caret to the last byte of the document.
    pub fn move_to_doc_end(&mut self, doc: &Document, extend: bool) {
        self.set_head(doc.len_bytes(), extend);
    }

    /// Move the caret one paragraph up.
    pub fn move_paragraph_up(&mut self, doc: &Document, extend: bool) {
        let line = doc.byte_to_line(self.head);
        let target = paragraph_target_up(doc, line);
        if target != line {
            self.set_head(doc.line_to_byte(target), extend);
        }
    }

    /// Move the caret one paragraph down.
    pub fn move_paragraph_down(&mut self, doc: &Document, extend: bool) {
        let line = doc.byte_to_line(self.head);
        let target = paragraph_target_down(doc, line);
        if target != line {
            self.set_head(doc.line_to_byte(target), extend);
        }
    }

    fn set_head(&mut self, byte: usize, extend: bool) {
        self.head = byte;
        if !extend {
            self.anchor = byte;
        }
    }

    pub fn move_vertical(&mut self, doc: &Document, lines_down: i64, sticky_col: Option<usize>, extend: bool) {
        if !extend {
            self.collapse_to_head();
        }
        let cur_line = doc.byte_to_line(self.head) as i64;
        let target = (cur_line + lines_down).clamp(0, doc.line_count().saturating_sub(1) as i64) as usize;
        let start = doc.line_to_byte(target);
        let line_text = doc.line(target);
        let without_eol = line_text.trim_end_matches(['\n', '\r']);
        let col = sticky_col.unwrap_or_else(|| self.column(doc));
        let byte_col = without_eol
            .char_indices()
            .nth(col)
            .map(|(i, _)| i)
            .unwrap_or_else(|| without_eol.len());
        self.head = start + byte_col;
    }

    /// Current sticky column (chars from line start), used for vertical movement.
    pub fn sticky_column(&self, doc: &Document) -> usize {
        self.column(doc)
    }

    /// Selection covering the word around `byte`.
    pub fn select_word_at(byte: usize, doc: &Document) -> Selection {
        self::select_word_at(byte, doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Document {
        Document::with_text(text)
    }

    #[test]
    fn word_left_moves_to_word_start() {
        let d = doc("hello world");
        let mut s = Selection::new(5);
        s.move_to_word_left(&d, false);
        assert_eq!(s.head, 0);

        let mut s = Selection::new(11);
        s.move_to_word_left(&d, false);
        assert_eq!(s.head, 6);

        let mut s = Selection::new(6);
        s.move_to_word_left(&d, false);
        assert_eq!(s.head, 0);
    }

    #[test]
    fn word_right_moves_to_word_end() {
        let d = doc("hello world");
        let mut s = Selection::new(0);
        s.move_to_word_right(&d, false);
        assert_eq!(s.head, 5);

        let mut s = Selection::new(5);
        s.move_to_word_right(&d, false);
        assert_eq!(s.head, 11);

        let mut s = Selection::new(6);
        s.move_to_word_right(&d, false);
        assert_eq!(s.head, 11);
    }

    #[test]
    fn word_movement_steps_over_separator_runs() {
        let d = doc("foo  /bar-baz_qux");
        assert_eq!(d.len_bytes(), 17);

        let mut s = Selection::new(0);
        s.move_to_word_right(&d, false);
        assert_eq!(s.head, 3);

        let mut s = Selection::new(3);
        s.move_to_word_right(&d, false);
        assert_eq!(s.head, 9);

        let mut s = Selection::new(9);
        s.move_to_word_right(&d, false);
        assert_eq!(s.head, 13);

        let mut s = Selection::new(13);
        s.move_to_word_right(&d, false);
        assert_eq!(s.head, 17);

        let mut s = Selection::new(17);
        s.move_to_word_left(&d, false);
        assert_eq!(s.head, 14);

        let mut s = Selection::new(14);
        s.move_to_word_left(&d, false);
        assert_eq!(s.head, 10);

        let mut s = Selection::new(10);
        s.move_to_word_left(&d, false);
        assert_eq!(s.head, 6);

        let mut s = Selection::new(6);
        s.move_to_word_left(&d, false);
        assert_eq!(s.head, 0);
    }

    #[test]
    fn word_movement_stops_at_document_edges() {
        let d = doc("hello");
        let mut s = Selection::new(0);
        s.move_to_word_left(&d, false);
        assert_eq!(s.head, 0);

        let mut s = Selection::new(5);
        s.move_to_word_right(&d, false);
        assert_eq!(s.head, 5);

        let empty = doc("");
        let mut s = Selection::new(0);
        s.move_to_word_left(&empty, false);
        assert_eq!(s.head, 0);
        s.move_to_word_right(&empty, false);
        assert_eq!(s.head, 0);
    }

    #[test]
    fn word_movement_handles_multibyte_words() {
        let d = doc("你好-world");
        assert_eq!(d.len_bytes(), 12);

        let mut s = Selection::new(12);
        s.move_to_word_left(&d, false);
        assert_eq!(s.head, 7);

        let mut s = Selection::new(7);
        s.move_to_word_left(&d, false);
        assert_eq!(s.head, 0);

        let mut s = Selection::new(0);
        s.move_to_word_right(&d, false);
        assert_eq!(s.head, 6);

        let mut s = Selection::new(6);
        s.move_to_word_right(&d, false);
        assert_eq!(s.head, 12);
    }

    #[test]
    fn word_movement_can_extend_the_selection() {
        let d = doc("hello world");
        let mut s = Selection { anchor: 0, head: 3 };
        s.move_to_word_right(&d, true);
        assert_eq!((s.anchor, s.head), (0, 5));
        assert_eq!(s.selected_text(&d), "hello");

        let mut s = Selection { anchor: 0, head: 3 };
        s.move_to_word_right(&d, false);
        assert_eq!((s.anchor, s.head), (5, 5));
    }

    #[test]
    fn doc_start_and_end_move_the_caret() {
        let d = doc("hello\nworld");
        let mut s = Selection::new(8);
        s.move_to_doc_start(&d, false);
        assert_eq!((s.anchor, s.head), (0, 0));

        let mut s = Selection::new(8);
        s.move_to_doc_end(&d, false);
        assert_eq!((s.anchor, s.head), (11, 11));

        let mut s = Selection { anchor: 2, head: 8 };
        s.move_to_doc_start(&d, true);
        assert_eq!((s.anchor, s.head), (2, 0));

        let mut s = Selection { anchor: 2, head: 8 };
        s.move_to_doc_end(&d, true);
        assert_eq!((s.anchor, s.head), (2, 11));
    }

    #[test]
    fn paragraph_up_moves_to_the_previous_paragraph() {
        let d = doc("alpha\nbeta\n\ngamma\n\ndelta\n");
        assert_eq!(d.line_count(), 7);

        let mut s = Selection::new(0);
        s.move_paragraph_up(&d, false);
        assert_eq!(s.line(&d), 0);

        let mut s = Selection::new(d.line_to_byte(1));
        s.move_paragraph_up(&d, false);
        assert_eq!(s.line(&d), 0);

        let mut s = Selection::new(d.line_to_byte(2));
        s.move_paragraph_up(&d, false);
        assert_eq!(s.line(&d), 1);

        let mut s = Selection::new(d.line_to_byte(3));
        s.move_paragraph_up(&d, false);
        assert_eq!(s.line(&d), 1);

        let mut s = Selection::new(d.line_to_byte(5));
        s.move_paragraph_up(&d, false);
        assert_eq!(s.line(&d), 3);
    }

    #[test]
    fn paragraph_down_moves_to_the_next_paragraph() {
        let d = doc("alpha\nbeta\n\ngamma\n\ndelta\n");

        let mut s = Selection::new(0);
        s.move_paragraph_down(&d, false);
        assert_eq!(s.line(&d), 1);

        let mut s = Selection::new(d.line_to_byte(1));
        s.move_paragraph_down(&d, false);
        assert_eq!(s.line(&d), 3);

        let mut s = Selection::new(d.line_to_byte(2));
        s.move_paragraph_down(&d, false);
        assert_eq!(s.line(&d), 3);

        let mut s = Selection::new(d.line_to_byte(3));
        s.move_paragraph_down(&d, false);
        assert_eq!(s.line(&d), 5);

        let mut s = Selection::new(d.line_to_byte(6));
        s.move_paragraph_down(&d, false);
        assert_eq!(s.line(&d), 6);
    }

    #[test]
    fn paragraph_movement_always_changes_the_line() {
        let d = doc("alpha\nbeta\n\ngamma\n\ndelta\n");
        for line in 0..d.line_count() {
            let mut up = Selection::new(d.line_to_byte(line));
            up.move_paragraph_up(&d, false);
            if line > 0 {
                assert!(up.line(&d) < line, "up from line {line} did not move");
            }

            let mut down = Selection::new(d.line_to_byte(line));
            down.move_paragraph_down(&d, false);
            if line + 1 < d.line_count() {
                assert!(down.line(&d) > line, "down from line {line} did not move");
            }
        }
    }

    #[test]
    fn select_word_at_covers_the_whole_word() {
        let d = doc("hello world");
        let s = select_word_at(2, &d);
        assert_eq!((s.anchor, s.head), (0, 5));
        assert_eq!(s.selected_text(&d), "hello");

        let s = select_word_at(8, &d);
        assert_eq!((s.anchor, s.head), (6, 11));
        assert_eq!(s.selected_text(&d), "world");

        let s = Selection::select_word_at(8, &d);
        assert_eq!(s.selected_text(&d), "world");
    }

    #[test]
    fn select_word_at_is_empty_on_separators() {
        let d = doc("hello world");
        let s = select_word_at(5, &d);
        assert!(s.is_empty());
        assert_eq!(s.head, 5);

        let s = select_word_at(11, &d);
        assert!(s.is_empty());
        assert_eq!(s.head, 11);

        let s = select_word_at(999, &d);
        assert!(s.is_empty());
        assert_eq!(s.head, 11);
    }

    #[test]
    fn select_word_at_handles_multibyte_and_separators() {
        let d = doc("你好-world");
        let s = select_word_at(1, &d);
        assert_eq!((s.anchor, s.head), (0, 6));
        assert_eq!(s.selected_text(&d), "你好");

        let s = select_word_at(9, &d);
        assert_eq!((s.anchor, s.head), (7, 12));
        assert_eq!(s.selected_text(&d), "world");

        let s = select_word_at(6, &d);
        assert!(s.is_empty());
        assert_eq!(s.head, 6);
    }
}
