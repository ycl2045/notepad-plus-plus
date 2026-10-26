//! Line → screen row mapping, gutter width, and soft-wrap budgeting.
//!
//! Contract (plan §3.4): the cache is keyed on `Document::epoch`; it is rebuilt
//! lazily on demand. Without wrapping each buffer line maps to exactly one row.
//! With wrapping, a line's row count is computed from the available pixel width.
//!
//! OWNED BY: U1 (contract) → U6 (hardening: incremental updates, wrap budgeting).

use crate::ui::editor::buffer::Document;

#[derive(Debug, Clone)]
pub struct RowMap {
    epoch: u64,
    /// Buffer line index → first screen row.
    line_first_row: Vec<usize>,
    /// Screen row → buffer line index.
    row_line: Vec<usize>,
    line_rows: usize,
    wrap: bool,
    max_cols: usize,
}

impl RowMap {
    pub fn new() -> Self {
        RowMap {
            epoch: u64::MAX,
            line_first_row: vec![0],
            row_line: vec![0],
            line_rows: 0,
            wrap: false,
            max_cols: 0,
        }
    }

    fn rebuild(&mut self, doc: &Document, wrap: bool, max_cols: usize) {
        self.epoch = doc.epoch;
        self.wrap = wrap;
        self.max_cols = max_cols;
        let lines = doc.line_count();
        self.line_first_row.clear();
        self.line_first_row.reserve(lines + 1);
        self.row_line.clear();
        self.row_line.reserve(lines + 1);
        let wrap_rows = wrap && max_cols > 0;
        for line in 0..lines {
            self.line_first_row.push(self.row_line.len());
            let rows = if wrap_rows {
                let text = doc.line(line);
                let chars = text.chars().filter(|c| *c != '\n' && *c != '\r').count();
                (chars / max_cols) + 1
            } else {
                1
            };
            for _ in 0..rows.max(1) {
                self.row_line.push(line);
            }
        }
        self.line_first_row.push(self.row_line.len());
        self.line_rows = self.row_line.len();
    }

    pub fn ensure(&mut self, doc: &Document, wrap: bool, max_cols: usize) {
        if self.epoch != doc.epoch || self.wrap != wrap || self.max_cols != max_cols {
            self.rebuild(doc, wrap, max_cols);
        }
    }

    pub fn total_rows(&self) -> usize {
        self.line_rows.max(1)
    }

    pub fn line_first_row(&self, line: usize) -> usize {
        self.line_first_row
            .get(line)
            .copied()
            .unwrap_or(self.total_rows())
    }

    pub fn row_to_line(&self, row: usize) -> usize {
        self.row_line
            .get(row)
            .copied()
            .unwrap_or_else(|| self.row_line.last().copied().unwrap_or(0))
    }

    pub fn line_row_count(&self, line: usize) -> usize {
        let start = self.line_first_row(line);
        let end = self.line_first_row(line + 1);
        (end - start).max(1)
    }

    /// Pixel width needed by the gutter for `line_count` lines at `digit_px` px
    /// per digit, with `pad` px padding on each side.
    pub fn gutter_width(line_count: usize, digit_px: i32, pad: i32) -> i32 {
        let digits = line_count.max(1).to_string().len() as i32;
        digits * digit_px + pad * 2
    }
}

impl Default for RowMap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn numbered_lines(count: usize) -> String {
        let mut text = String::with_capacity(count * 16);
        for i in 0..count {
            text.push_str("let value = ");
            text.push_str(&i.to_string());
            text.push_str(";\n");
        }
        text
    }

    #[test]
    fn no_wrap_is_one_row_per_line() {
        let doc = Document::with_text("a\nb\nc\n");
        let mut rm = RowMap::new();
        rm.ensure(&doc, false, 0);
        assert_eq!(rm.total_rows(), 4);
        assert_eq!(rm.line_first_row(2), 2);
        assert_eq!(rm.row_to_line(2), 2);
    }

    #[test]
    fn wrap_adds_rows() {
        let doc = Document::with_text("0123456789");
        let mut rm = RowMap::new();
        rm.ensure(&doc, true, 4);
        assert_eq!(rm.total_rows(), 3);
        assert_eq!(rm.row_to_line(2), 0);
    }

    #[test]
    fn gutter_width_scales_with_digits() {
        assert_eq!(RowMap::gutter_width(9, 8, 8), 24);
        assert_eq!(RowMap::gutter_width(1000, 8, 8), 48);
    }

    #[test]
    fn rebuild_of_50k_lines_is_fast() {
        let doc = Document::with_text(&numbered_lines(50_000));
        let mut rm = RowMap::new();

        let start = Instant::now();
        rm.ensure(&doc, false, 0);
        let elapsed = start.elapsed();

        eprintln!("rowmap rebuild of 50k lines: {elapsed:?}");
        assert_eq!(rm.total_rows(), doc.line_count());
        assert_eq!(rm.row_to_line(0), 0);
        assert_eq!(rm.row_to_line(rm.total_rows() - 1), doc.line_count() - 1);
        assert!(
            elapsed.as_millis() < 100,
            "50k-line rebuild took {elapsed:?}, budget is 100ms"
        );
    }

    #[test]
    fn wrap_budget_counts_soft_wrapped_rows() {
        let doc = Document::with_text(&numbered_lines(50_000));
        let mut rm = RowMap::new();
        rm.ensure(&doc, true, 8);
        assert!(
            rm.total_rows() > doc.line_count(),
            "wrapped rows must exceed the line count"
        );
        assert_eq!(rm.row_to_line(rm.total_rows() - 1), doc.line_count() - 1);
    }
}
