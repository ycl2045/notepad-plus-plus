//! Self-managed scrolling (plan §3.1).
//!
//! The EditorView owns its viewport: `size_allocate` feeds the viewport height
//! here, we publish `page_size`/`upper` onto a `GtkAdjustment`, and `snapshot`
//! reads `vadjustment.value()` to decide which rows to draw. Content sizing is
//! deliberately NOT handed to `GtkScrolledWindow`.
//!
//! OWNED BY: U1 (contract) → U6 (hardening: smooth scrolling, wheel/keyboard,
//! scroll-into-view for caret and search results).

use gtk4::prelude::*;

#[derive(Debug, Default)]
pub struct ScrollState {
    pub viewport_h: i32,
    pub content_h: i32,
    pub scroll_y: f64,
    pub row_h: i32,
}

impl ScrollState {
    pub fn new() -> Self {
        ScrollState {
            viewport_h: 0,
            content_h: 0,
            scroll_y: 0.0,
            row_h: 16,
        }
    }

    /// Maximum scroll offset that keeps the last page visible.
    pub fn max_scroll(&self) -> f64 {
        (self.content_h - self.viewport_h).max(0) as f64
    }

    pub fn clamp(&mut self) {
        self.scroll_y = self.scroll_y.clamp(0.0, self.max_scroll());
    }

    pub fn first_row(&self) -> usize {
        if self.row_h <= 0 {
            return 0;
        }
        (self.scroll_y / self.row_h as f64).floor() as usize
    }

    pub fn visible_row_count(&self) -> usize {
        if self.row_h <= 0 {
            return 0;
        }
        (self.viewport_h as f64 / self.row_h as f64).ceil() as usize + 1
    }

    pub fn scroll_to_row(&mut self, row: usize) {
        let y = row as f64 * self.row_h as f64;
        if y < self.scroll_y {
            self.scroll_y = y;
        } else if y + self.row_h as f64 > self.scroll_y + self.viewport_h as f64 {
            self.scroll_y = (y + self.row_h as f64 - self.viewport_h as f64).max(0.0);
        }
        self.clamp();
    }

    pub fn scroll_by(&mut self, delta: f64) {
        self.scroll_y += delta;
        self.clamp();
    }

    /// Scroll by whole viewports (PageUp / PageDown). Negative counts scroll
    /// back up; the result is always clamped to the scrollable range.
    pub fn scroll_pages(&mut self, n_pages: i32) {
        let delta = n_pages as f64 * self.viewport_h as f64;
        self.scroll_by(delta);
    }

    /// Scroll by whole rows. Negative counts scroll back up; the result is
    /// always clamped to the scrollable range.
    pub fn scroll_lines(&mut self, n_lines: i32) {
        let delta = n_lines as f64 * self.row_h as f64;
        self.scroll_by(delta);
    }

    /// Update the content height, then re-clamp `scroll_y` against it.
    pub fn clamp_to(&mut self, content_h: i32) {
        self.content_h = content_h.max(0);
        self.clamp();
    }

    /// Publish onto a `GtkAdjustment` (page_size = viewport, upper = content).
    pub fn publish(&self, adj: &gtk4::Adjustment) {
        adj.configure(
            self.scroll_y,
            0.0,
            self.content_h as f64,
            1.0,
            (self.viewport_h / 2).max(10) as f64,
            self.viewport_h as f64,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> ScrollState {
        let mut s = ScrollState::new();
        s.viewport_h = 200;
        s.content_h = 1000;
        s.row_h = 20;
        s
    }

    #[test]
    fn clamps_to_content_bounds() {
        let mut s = ScrollState::new();
        s.viewport_h = 300;
        s.content_h = 1000;
        s.row_h = 20;
        s.scroll_y = 5000.0;
        s.clamp();
        assert_eq!(s.scroll_y, 700.0);
    }

    #[test]
    fn scroll_to_row_keeps_row_visible() {
        let mut s = ScrollState::new();
        s.viewport_h = 200;
        s.content_h = 1000;
        s.row_h = 20;
        s.scroll_to_row(50);
        assert!(s.first_row() <= 50);
        assert!(s.first_row() + s.visible_row_count() > 50);
    }

    #[test]
    fn scroll_pages_moves_by_viewports() {
        let mut s = state();
        s.scroll_pages(2);
        assert_eq!(s.scroll_y, 400.0);
        s.scroll_pages(-1);
        assert_eq!(s.scroll_y, 200.0);
    }

    #[test]
    fn scroll_pages_clamps_at_both_ends() {
        let mut s = state();
        s.scroll_pages(100);
        assert_eq!(s.scroll_y, s.max_scroll());
        s.scroll_pages(-100);
        assert_eq!(s.scroll_y, 0.0);
    }

    #[test]
    fn scroll_lines_moves_by_rows() {
        let mut s = state();
        s.scroll_lines(3);
        assert_eq!(s.scroll_y, 60.0);
        s.scroll_lines(-2);
        assert_eq!(s.scroll_y, 20.0);
    }

    #[test]
    fn scroll_lines_clamps_at_both_ends() {
        let mut s = state();
        s.scroll_lines(1000);
        assert_eq!(s.scroll_y, s.max_scroll());
        s.scroll_lines(-1000);
        assert_eq!(s.scroll_y, 0.0);
    }

    #[test]
    fn clamp_to_updates_content_height_and_clamps_scroll() {
        let mut s = state();
        s.scroll_y = 700.0;
        s.clamp_to(500);
        assert_eq!(s.content_h, 500);
        assert_eq!(s.scroll_y, 300.0);

        s.clamp_to(2000);
        assert_eq!(s.content_h, 2000);
        assert_eq!(
            s.scroll_y, 300.0,
            "growing the content must not move the scroll"
        );
    }

    #[test]
    fn clamp_to_never_produces_a_negative_content_height() {
        let mut s = state();
        s.scroll_y = 700.0;
        s.clamp_to(-10);
        assert_eq!(s.content_h, 0);
        assert_eq!(s.scroll_y, 0.0);
    }
}
