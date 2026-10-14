//! Cursor and selection model.
//!
//! Contract (plan §3.3): `anchor` and `head` are BYTE offsets. `head` is the
//! caret position; when `anchor == head` there is no selection. All movement
//! helpers take the `Document` so line addressing stays correct with wrapping
//! and multi-byte text. Column arithmetic uses char counts, never bytes.
//!
//! OWNED BY: U1 (contract) → U3 (hardening: word movement, block selection).

use crate::ui::editor::buffer::Document;

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
}
