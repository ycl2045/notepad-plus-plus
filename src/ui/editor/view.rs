//! Editor view: GObject subclass, self-managed scrolling, snapshot rendering.
//!
//! This is the crown-jewel module. Everything the user sees is drawn here.
//! Verified facts it relies on (see AGENTS.md): the crate is `gtk4`;
//! `Snapshot` is `gtk4::Snapshot`; `measure` returns a 4-tuple; IME goes through
//! `EventControllerKey::set_im_context`; `glib::wrapper!` puts `@extends` first.
//!
//! Keyboard + IME wiring (U7). The GTK 4.22 sources settle the event order:
//!
//! * `GtkEventControllerKey` runs `gtk_im_context_filter_keypress()` *before*
//!   it emits `key-pressed`. When the IM context consumed the key, GTK emits
//!   `im-update` instead and `key-pressed` never fires for that press.
//! * `GtkIMContextSimple` commits printable characters and filters the key
//!   press, so plain typing arrives through `IMContext::commit`; editing keys
//!   (Return/Tab/Backspace/Delete/arrows/Home/End/PageUp/PageDown) and
//!   Ctrl/Alt chords are *not* filtered and do reach `key-pressed`.
//!
//! Both text paths therefore exist and both are needed: `commit` inserts what
//! the IME produced, `key-pressed` inserts plain characters when no IM context
//! consumed them. `commit_count` + `im_filtered` keep the two paths from
//! inserting the same character twice.
//!
//! OWNED BY: U1 (structure + verified spike) → U7 (input, IME, caret scrolling).

use std::cell::RefCell;

use gtk4::gdk;
use gtk4::prelude::*;
use gtk4::{
    Accessible, Buildable, ConstraintTarget, Snapshot, graphene, pango, subclass::prelude::*,
};

use crate::ui::editor::buffer::{Document, EditKind};
use crate::ui::editor::cursor::Selection;
use crate::ui::editor::highlight::Highlighter;
use crate::ui::editor::rowmap::RowMap;
use crate::ui::editor::scroll::ScrollState;
use crate::ui::search::{SearchError, SearchOptions};
use crate::ui::theme::Theme;

/// Sample used to measure the advance width of one character. The editor
/// font is monospace, so the width of a run of characters divided by their
/// count is the advance of a single one.
const CHAR_WIDTH_SAMPLE: &str = "MMMMMMMMMM";

glib::wrapper! {
    pub struct EditorView(ObjectSubclass<imp::EditorView>)
        @extends gtk4::Widget,
        @implements Accessible, Buildable, ConstraintTarget;
}

impl Default for EditorView {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorView {
    pub fn new() -> Self {
        let view: Self = glib::Object::new();
        let imp = view.imp();
        *imp.highlighter.borrow_mut() = Some(Highlighter::new());

        // --- key input + IME ---------------------------------------------------
        // `constructed()` already created the IM context, so it can be attached
        // here. GTK routes the IM filter before the `key-pressed` signal.
        let key_ctrl = gtk4::EventControllerKey::new();
        if let Some(im) = imp.im_context.borrow().as_ref() {
            key_ctrl.set_im_context(Some(im));
        }
        let weak = view.downgrade();
        let weak_filter = weak.clone();
        key_ctrl.connect_key_pressed(move |_ctrl, keyval, _keycode, state| {
            let Some(view) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if view.handle_key(keyval, state) {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        key_ctrl.connect_im_update(move |_ctrl| {
            // The IM context swallowed that key press; any commit it produced
            // belongs to that press and not to the next one.
            if let Some(view) = weak_filter.upgrade() {
                view.imp().im_filtered.set(true);
            }
        });
        view.add_controller(key_ctrl);
        view.set_focusable(true);
        view.set_can_focus(true);

        // --- wheel scrolling ---------------------------------------------------
        let scroll_ctrl = gtk4::EventControllerScroll::new(gtk4::EventControllerScrollFlags::NONE);
        let weak = view.downgrade();
        scroll_ctrl.connect_scroll(move |_ctrl, _dx, dy| {
            let Some(view) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let imp = view.imp();
            let row_h = imp.scroll.borrow().row_h;
            imp.scroll.borrow_mut().scroll_by(-dy * row_h as f64);
            imp.obj().queue_draw();
            glib::Propagation::Stop
        });
        view.add_controller(scroll_ctrl);

        view
    }

    pub fn insert_text_at_caret(&self, text: &str) {
        let imp = self.imp();
        let mut sel = *imp.selection.borrow();
        let (start, end) = sel.range();
        imp.doc
            .borrow_mut()
            .replace(start, end - start, text, EditKind::Insert);
        let new_pos = start + text.len();
        sel.anchor = new_pos;
        sel.head = new_pos;
        *imp.selection.borrow_mut() = sel;
        imp.sticky_col.set(None);
        imp.ensure_caret_visible();
        self.queue_draw();
    }

    pub fn backspace(&self) {
        let imp = self.imp();
        let sel = *imp.selection.borrow();
        let (start, end) = sel.range();
        let (s, len) = if start == end {
            match self.char_before(start) {
                Some((off, len)) => (off, len),
                None => return,
            }
        } else {
            (start, end - start)
        };
        imp.doc.borrow_mut().replace(s, len, "", EditKind::Delete);
        *imp.selection.borrow_mut() = Selection::new(s);
        imp.sticky_col.set(None);
        imp.ensure_caret_visible();
        self.queue_draw();
    }

    pub fn newline(&self) {
        let doc = self.imp().doc.borrow();
        let eol = doc.eol.as_str().to_string();
        drop(doc);
        self.insert_text_at_caret(&eol);
    }

    pub fn load_text(&self, text: &str) {
        let imp = self.imp();
        imp.doc.borrow_mut().load(text);
        *imp.selection.borrow_mut() = Selection::new(0);
        imp.sticky_col.set(None);
        imp.ensure_caret_visible();
        self.queue_draw();
    }

    pub fn doc_text(&self) -> String {
        self.imp().doc.borrow().text()
    }

    pub fn set_theme(&self, theme: Theme) {
        *self.imp().theme.borrow_mut() = theme;
        self.queue_draw();
    }

    /// Set the syntect syntax used to highlight the text (`Rust`, `Plain Text`, ...).
    pub fn set_language(&self, name: &str) {
        let imp = self.imp();
        if let Some(highlighter) = imp.highlighter.borrow_mut().as_mut() {
            highlighter.set_language(name);
        }
        self.queue_draw();
    }

    pub fn selection(&self) -> Selection {
        *self.imp().selection.borrow()
    }

    /// Byte offset of the caret (the head of the selection).
    pub fn caret(&self) -> usize {
        self.imp().selection.borrow().head
    }

    /// Put the caret on the byte offset `offset`.
    ///
    /// The offset is clamped to the end of the document, so a stale value
    /// from a session file cannot point past the end of the text.
    pub fn set_caret(&self, offset: usize) {
        let imp = self.imp();
        let len = imp.doc.borrow().len_bytes();
        *imp.selection.borrow_mut() = Selection::new(offset.min(len));
        imp.sticky_col.set(None);
        imp.ensure_caret_visible();
        self.queue_draw();
    }

    /// Select the whole document.
    ///
    /// The caret ends up at the end of the text, the way every editor does
    /// it; the viewport is left where it is.
    pub fn select_all(&self) {
        let imp = self.imp();
        let len = imp.doc.borrow().len_bytes();
        let mut sel = *imp.selection.borrow();
        sel.anchor = 0;
        sel.head = len;
        *imp.selection.borrow_mut() = sel;
        imp.sticky_col.set(None);
        self.queue_draw();
    }

    /// Select the byte range `[start, end)`.
    pub fn select_range(&self, start: usize, end: usize) {
        let imp = self.imp();
        let len = imp.doc.borrow().len_bytes();
        let start = start.min(len);
        let end = end.min(len).max(start);
        let mut sel = *imp.selection.borrow();
        sel.anchor = start;
        sel.head = end;
        *imp.selection.borrow_mut() = sel;
        imp.sticky_col.set(None);
        imp.ensure_caret_visible();
        self.queue_draw();
    }

    /// Number of lines in the document.
    pub fn line_count(&self) -> usize {
        self.imp().doc.borrow().line_count()
    }

    /// The 1-based line the caret is on.
    pub fn caret_line(&self) -> usize {
        let imp = self.imp();
        let doc = imp.doc.borrow();
        let sel = *imp.selection.borrow();
        doc.byte_to_line(sel.head) + 1
    }

    /// Replace the byte range `[start, end)` with `text`.
    pub fn replace_range(&self, start: usize, end: usize, text: &str) {
        let imp = self.imp();
        let len = imp.doc.borrow().len_bytes();
        let start = start.min(len);
        let end = end.min(len).max(start);
        imp.doc
            .borrow_mut()
            .replace(start, end - start, text, EditKind::Structural);
        imp.sticky_col.set(None);
        self.queue_draw();
    }

    /// Move the caret to the start of the 1-based `line`.
    ///
    /// A line past the end of the document moves the caret to its last line.
    pub fn goto_line(&self, line: usize) {
        let offset = {
            let doc = self.imp().doc.borrow();
            let line = line
                .saturating_sub(1)
                .min(doc.line_count().saturating_sub(1));
            doc.line_to_byte(line)
        };
        self.set_caret(offset);
    }

    /// Find the next match of `pattern` at or after `from`.
    pub fn find_next(
        &self,
        pattern: &str,
        options: &SearchOptions,
        from: usize,
    ) -> Result<Option<(usize, usize)>, SearchError> {
        let doc = self.imp().doc.borrow();
        Ok(crate::ui::search::find_next(&doc, pattern, options, from)?.map(|m| (m.start, m.end)))
    }

    /// Find the last match of `pattern` that starts before `from`.
    pub fn find_previous(
        &self,
        pattern: &str,
        options: &SearchOptions,
        from: usize,
    ) -> Result<Option<(usize, usize)>, SearchError> {
        let doc = self.imp().doc.borrow();
        Ok(
            crate::ui::search::find_previous(&doc, pattern, options, from)?
                .map(|m| (m.start, m.end)),
        )
    }

    /// Replace every match of `pattern` and return how many were replaced.
    pub fn replace_all(
        &self,
        pattern: &str,
        replacement: &str,
        options: &SearchOptions,
    ) -> Result<usize, SearchError> {
        let count = {
            let mut doc = self.imp().doc.borrow_mut();
            crate::ui::search::replace_all(&mut doc, pattern, replacement, options)?
        };
        self.queue_draw();
        Ok(count)
    }

    /// Current vertical scroll offset in pixels.
    pub fn scroll_offset(&self) -> f64 {
        let imp = self.imp();
        // A pending offset has not been applied to the scroll state yet, but it
        // is the value the view is about to scroll to.
        if let Some(pending) = imp.pending_scroll.get() {
            return pending;
        }
        imp.scroll.borrow().scroll_y
    }

    /// Scroll the viewport to the vertical offset `offset` (in pixels).
    ///
    /// The row height is only known after the first paint, so an offset that
    /// arrives before that is kept in `pending_scroll` and applied by
    /// `snapshot` once the geometry has been measured.
    pub fn set_scroll_offset(&self, offset: f64) {
        let imp = self.imp();
        let offset = offset.max(0.0);
        if imp.row_h_ready.get() {
            let mut scroll = imp.scroll.borrow_mut();
            scroll.scroll_y = offset;
            scroll.clamp();
        } else {
            imp.pending_scroll.set(Some(offset));
        }
        self.queue_draw();
    }

    // --- private helpers ----------------------------------------------------

    /// Text the IM context committed. The IME owns the caret placement, so the
    /// committed string goes in at the caret like any other insertion.
    fn handle_commit(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        let imp = self.imp();
        imp.commit_count.set(imp.commit_count.get() + 1);
        imp.last_commit.replace(text.to_string());
        self.insert_text_at_caret(text);
    }

    /// Handle one key press. Returns `true` when the key was consumed.
    fn handle_key(&self, keyval: gdk::Key, state: gdk::ModifierType) -> bool {
        let imp = self.imp();
        let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
        let alt = state.contains(gdk::ModifierType::ALT_MASK);

        // The IM context may already have committed text for this very press:
        // GTK runs the IM filter before emitting `key-pressed`.
        let commit_count = imp.commit_count.get();
        let filtered_before = imp.im_filtered.replace(false);
        let im_committed = !filtered_before && commit_count != imp.key_commit_count.get();
        imp.key_commit_count.set(commit_count);

        if ctrl && !alt {
            match keyval {
                gdk::Key::z => {
                    self.undo_edit();
                    return true;
                }
                gdk::Key::Z | gdk::Key::y | gdk::Key::Y => {
                    self.redo_edit();
                    return true;
                }
                gdk::Key::Home => {
                    self.goto_doc_edge(false);
                    return true;
                }
                gdk::Key::End => {
                    self.goto_doc_edge(true);
                    return true;
                }
                _ => {}
            }
        }
        if alt {
            // Leave Alt chords (and Alt+Tab style grabs) to the window.
            return false;
        }

        match keyval {
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter => {
                self.newline();
                true
            }
            gdk::Key::BackSpace => {
                self.backspace();
                true
            }
            gdk::Key::Delete => {
                self.delete_forward();
                true
            }
            gdk::Key::Tab => {
                self.insert_text_at_caret("\t");
                true
            }
            gdk::Key::Left => {
                self.move_char(-1, shift);
                true
            }
            gdk::Key::Right => {
                self.move_char(1, shift);
                true
            }
            gdk::Key::Up => {
                self.move_vertical(-1, shift);
                true
            }
            gdk::Key::Down => {
                self.move_vertical(1, shift);
                true
            }
            gdk::Key::Home => {
                self.move_line_edge(false, shift);
                true
            }
            gdk::Key::End => {
                self.move_line_edge(true, shift);
                true
            }
            gdk::Key::Page_Up => {
                self.move_vertical(-(self.page_rows() as i64), shift);
                true
            }
            gdk::Key::Page_Down => {
                self.move_vertical(self.page_rows() as i64, shift);
                true
            }
            _ => {
                if ctrl {
                    return false;
                }
                let Some(ch) = keyval.to_unicode() else {
                    return false;
                };
                if ch.is_control() {
                    return false;
                }
                let mut buf = [0u8; 4];
                let text = ch.encode_utf8(&mut buf);
                if im_committed && imp.last_commit.borrow().as_str() == text {
                    // The IM context already inserted this very character.
                    return true;
                }
                self.insert_text_at_caret(text);
                true
            }
        }
    }

    /// Byte range `[off, off + len)` of the character before `offset`.
    /// `None` when there is no character before it.
    fn char_before(&self, offset: usize) -> Option<(usize, usize)> {
        let imp = self.imp();
        let doc = imp.doc.borrow();
        let offset = offset.min(doc.len_bytes());
        let line = doc.byte_to_line(offset);
        let line_start = doc.line_to_byte(line);
        let text = doc.line(line);
        let col = doc.slice(line_start, offset).chars().count();
        if col > 0 {
            let (i, c) = text.char_indices().nth(col - 1)?;
            Some((line_start + i, c.len_utf8()))
        } else {
            // Start of the line: the EOL of the previous line is before us.
            if line == 0 {
                return None;
            }
            let prev = line - 1;
            let prev_start = doc.line_to_byte(prev);
            let prev_end = prev_start + doc.line(prev).len();
            let len = line_start.saturating_sub(prev_end);
            if len == 0 {
                None
            } else {
                Some((prev_end, len))
            }
        }
    }

    /// Byte range `[off, off + len)` of the character after `offset`.
    /// `None` when there is no character after it.
    fn char_after(&self, offset: usize) -> Option<(usize, usize)> {
        let imp = self.imp();
        let doc = imp.doc.borrow();
        let offset = offset.min(doc.len_bytes());
        let line = doc.byte_to_line(offset);
        let line_start = doc.line_to_byte(line);
        let text = doc.line(line);
        let col = doc.slice(line_start, offset).chars().count();
        if col < text.chars().count() {
            let (i, c) = text.char_indices().nth(col)?;
            Some((line_start + i, c.len_utf8()))
        } else {
            // End of the line: the EOL follows (if there is a next line).
            let next = line + 1;
            if next >= doc.line_count() {
                return None;
            }
            let next_start = doc.line_to_byte(next);
            let line_end = line_start + text.len();
            let len = next_start.saturating_sub(line_end);
            if len == 0 {
                None
            } else {
                Some((line_end, len))
            }
        }
    }

    /// Delete the character after the caret (or the selection).
    fn delete_forward(&self) {
        let imp = self.imp();
        let sel = *imp.selection.borrow();
        let (start, end) = sel.range();
        let (s, len) = if start == end {
            match self.char_after(start) {
                Some((off, len)) => (off, len),
                None => return,
            }
        } else {
            (start, end - start)
        };
        imp.doc.borrow_mut().replace(s, len, "", EditKind::Delete);
        *imp.selection.borrow_mut() = Selection::new(s);
        imp.sticky_col.set(None);
        imp.ensure_caret_visible();
        self.queue_draw();
    }

    /// Move the caret one character left/right.
    fn move_char(&self, dir: i64, extend: bool) {
        let imp = self.imp();
        let mut sel = *imp.selection.borrow();
        let head = sel.head;
        let new_head = if dir < 0 {
            self.char_before(head).map(|(off, _)| off).unwrap_or(head)
        } else {
            self.char_after(head)
                .map(|(off, len)| off + len)
                .unwrap_or(head)
        };
        sel.head = new_head;
        if !extend {
            sel.anchor = new_head;
        }
        *imp.selection.borrow_mut() = sel;
        imp.sticky_col.set(None);
        imp.ensure_caret_visible();
        self.queue_draw();
    }

    /// Move the caret one line up/down, keeping the sticky column.
    fn move_vertical(&self, delta: i64, extend: bool) {
        let imp = self.imp();
        let doc = imp.doc.borrow();
        let mut sel = *imp.selection.borrow();
        let sticky = imp
            .sticky_col
            .get()
            .unwrap_or_else(|| sel.sticky_column(&doc));
        sel.move_vertical(&doc, delta, Some(sticky), extend);
        if !extend {
            // `Selection::move_vertical` collapses before moving the head, so
            // the anchor still points at the old position: collapse again.
            sel.anchor = sel.head;
        }
        imp.sticky_col.set(Some(sticky));
        *imp.selection.borrow_mut() = sel;
        drop(doc);
        imp.ensure_caret_visible();
        self.queue_draw();
    }

    /// Move the caret to the start/end of its line.
    fn move_line_edge(&self, to_end: bool, extend: bool) {
        let imp = self.imp();
        let doc = imp.doc.borrow();
        let mut sel = *imp.selection.borrow();
        if to_end {
            sel.move_to_line_end(&doc, extend);
        } else {
            sel.move_to_line_start(&doc, extend);
        }
        if !extend {
            sel.anchor = sel.head;
        }
        *imp.selection.borrow_mut() = sel;
        drop(doc);
        imp.sticky_col.set(None);
        imp.ensure_caret_visible();
        self.queue_draw();
    }

    /// Move the caret to the very start/end of the document.
    fn goto_doc_edge(&self, to_end: bool) {
        let imp = self.imp();
        let doc = imp.doc.borrow();
        let mut sel = *imp.selection.borrow();
        if to_end {
            sel.move_to_doc_end(&doc, false);
        } else {
            sel.move_to_doc_start(&doc, false);
        }
        *imp.selection.borrow_mut() = sel;
        drop(doc);
        imp.sticky_col.set(None);
        imp.ensure_caret_visible();
        self.queue_draw();
    }

    /// Undo the last edit and put the caret back where that edit happened.
    fn undo_edit(&self) {
        let imp = self.imp();
        let Some(edit) = imp.doc.borrow_mut().undo() else {
            return;
        };
        let len = imp.doc.borrow().len_bytes();
        *imp.selection.borrow_mut() = Selection::new(edit.start.min(len));
        imp.sticky_col.set(None);
        imp.ensure_caret_visible();
        self.queue_draw();
    }

    /// Redo the last undone edit and put the caret back where it happened.
    fn redo_edit(&self) {
        let imp = self.imp();
        let Some(edit) = imp.doc.borrow_mut().redo() else {
            return;
        };
        let len = imp.doc.borrow().len_bytes();
        *imp.selection.borrow_mut() = Selection::new(edit.start.min(len));
        imp.sticky_col.set(None);
        imp.ensure_caret_visible();
        self.queue_draw();
    }

    /// Number of rows a PageUp/PageDown should move.
    fn page_rows(&self) -> i32 {
        let imp = self.imp();
        let scroll = imp.scroll.borrow();
        scroll.visible_row_count().max(1) as i32
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct EditorView {
        pub doc: RefCell<Document>,
        pub selection: RefCell<Selection>,
        pub theme: RefCell<Theme>,
        pub im_context: RefCell<Option<gtk4::IMMulticontext>>,
        pub highlighter: RefCell<Option<Highlighter>>,
        pub rowmap: RefCell<RowMap>,
        pub scroll: RefCell<ScrollState>,
        pub layout_cache: RefCell<Option<pango::Layout>>,
        pub caret_blink: std::cell::Cell<bool>,
        /// Sticky column (in chars) kept while moving vertically.
        pub sticky_col: std::cell::Cell<Option<usize>>,
        /// Number of texts the IM context committed so far.
        pub commit_count: std::cell::Cell<u64>,
        /// `commit_count` observed at the last key press.
        pub key_commit_count: std::cell::Cell<u64>,
        /// Set when the IM context filtered (swallowed) a key press.
        pub im_filtered: std::cell::Cell<bool>,
        /// Text of the most recent IM commit.
        pub last_commit: RefCell<String>,
        /// Scroll offset to apply once the row geometry has been measured.
        pub pending_scroll: std::cell::Cell<Option<f64>>,
        /// True once `snapshot` has measured the real row height.
        pub row_h_ready: std::cell::Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for EditorView {
        const NAME: &'static str = "NpadEditorView";
        type Type = super::EditorView;
        type ParentType = gtk4::Widget;
    }

    impl ObjectImpl for EditorView {
        fn constructed(&self) {
            self.parent_constructed();
            // `snapshot()` paints the gutter numbers and the text rows through
            // this layout; while it is `None` it silently draws neither.
            let layout = pango::Layout::new(&self.obj().pango_context());
            layout.set_font_description(Some(&pango::FontDescription::from_string("monospace 11")));
            *self.layout_cache.borrow_mut() = Some(layout);
            if self.im_context.borrow().is_none() {
                let im = gtk4::IMMulticontext::new();
                im.set_use_preedit(false);
                let weak = self.obj().downgrade();
                im.connect_commit(move |_im, text| {
                    let Some(view) = weak.upgrade() else {
                        return;
                    };
                    view.handle_commit(text);
                });
                *self.im_context.borrow_mut() = Some(im);
            }
        }
    }

    impl WidgetImpl for EditorView {
        fn realize(&self) {
            self.parent_realize();
            // Take the keyboard focus so the editor receives key presses.
            self.obj().grab_focus();
        }

        fn measure(&self, _orientation: gtk4::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            (600, 600, -1, -1)
        }

        fn size_allocate(&self, _width: i32, _height: i32, _baseline: i32) {
            self.parent_size_allocate(_width, _height, _baseline);
            let doc = self.doc.borrow();
            let rows = doc.line_count();
            drop(doc);
            let mut scroll = self.scroll.borrow_mut();
            scroll.viewport_h = _height.max(0);
            scroll.content_h = (rows as i32) * scroll.row_h;
            scroll.clamp();
        }

        fn snapshot(&self, snapshot: &Snapshot) {
            let widget = self.obj();
            let width = widget.width() as f32;
            let height = widget.height() as f32;

            let theme = *self.theme.borrow();
            let doc = self.doc.borrow();
            let sel = *self.selection.borrow();
            let mut scroll = self.scroll.borrow_mut();
            let mut rowmap = self.rowmap.borrow_mut();
            rowmap.ensure(&doc, false, 0);
            let total_rows = rowmap.total_rows();

            // The row height has to be measured before the content height:
            // otherwise the scroll range is computed from the placeholder
            // height and a restored scroll offset lands on the wrong row.
            let mut char_w = 8.0f32;
            if let Some(layout) = self.layout_cache.borrow().as_ref() {
                layout.set_text("Ag");
                let (_mw, mh) = layout.pixel_size();
                scroll.row_h = mh.max(16);
                layout.set_text(CHAR_WIDTH_SAMPLE);
                let (sw, _sh) = layout.pixel_size();
                let count = CHAR_WIDTH_SAMPLE.chars().count() as f32;
                if sw > 0 && count > 0.0 {
                    char_w = sw as f32 / count;
                }
            }
            let row_h = scroll.row_h;
            scroll.viewport_h = scroll.viewport_h.max(height as i32);
            scroll.content_h = (total_rows as i32) * row_h;
            let caret_line = doc.byte_to_line(sel.head);

            // A scroll offset restored from the session could not be applied
            // before the geometry was known; now it can.
            if let Some(pending) = self.pending_scroll.replace(None) {
                scroll.scroll_y = pending;
                // The restored offset can leave the caret off screen - a
                // hand-written session, or a file that changed since it was
                // saved. The caret wins, because that is where the user is.
                let first = scroll.first_row();
                let visible = scroll.visible_row_count();
                if caret_line < first || caret_line >= first + visible {
                    scroll.scroll_to_row(caret_line);
                }
            }
            self.row_h_ready.set(true);
            scroll.clamp();

            let first = scroll.first_row();
            let visible = scroll.visible_row_count();

            let gutter_w = RowMap::gutter_width(total_rows, 8, 8) as f32;
            let pad = 8.0f32;

            snapshot.append_color(&theme.bg, &graphene::Rect::new(0.0, 0.0, width, height));
            snapshot.append_color(
                &theme.gutter,
                &graphene::Rect::new(0.0, 0.0, gutter_w, height),
            );

            let (sel_start, sel_end) = sel.range();

            for row in first..(first + visible).min(total_rows) {
                let line = rowmap.row_to_line(row);
                let row_h_f = row_h as f32;
                let y = row as f32 * row_h_f - scroll.scroll_y as f32 + pad;

                if line == caret_line {
                    snapshot.append_color(
                        &theme.current_line,
                        &graphene::Rect::new(gutter_w, y - 4.0, width - gutter_w, row_h_f),
                    );
                }

                if sel_end > sel_start {
                    let line_start = doc.line_to_byte(line);
                    let line_end = doc.line_to_byte(line + 1);
                    let from = sel_start.max(line_start);
                    let to = sel_end.min(line_end);
                    if to > from {
                        let col_from = doc.slice(line_start, from).chars().count();
                        let col_to = col_from + doc.slice(from, to).chars().count();
                        let x_from = gutter_w + pad + col_from as f32 * char_w;
                        let x_to = gutter_w + pad + col_to as f32 * char_w;
                        snapshot.append_color(
                            &theme.selection,
                            &graphene::Rect::new(x_from, y, (x_to - x_from).max(1.0), row_h_f),
                        );
                    }
                }

                let num = format!("{}", line + 1);
                if let Some(layout) = self.layout_cache.borrow().as_ref() {
                    layout.set_text(&num);
                    snapshot.save();
                    snapshot.translate(&graphene::Point::new(gutter_w - 16.0, y));
                    snapshot.append_layout(layout, &theme.gutter_fg);
                    snapshot.restore();
                }

                let text = doc.line(line);
                let text = text.trim_end_matches(['\n', '\r']);
                if !text.is_empty() {
                    let layout_cache = self.layout_cache.borrow();
                    if let Some(layout) = layout_cache.as_ref() {
                        layout.set_text(text);
                        snapshot.save();
                        snapshot.translate(&graphene::Point::new(gutter_w + pad, y));
                        snapshot.append_layout(layout, &theme.fg);
                        snapshot.restore();
                    }
                }
            }

            let caret_y = caret_line as f32 * (row_h as f32) - scroll.scroll_y as f32 + pad;
            let caret_col = sel.column(&doc);
            let caret_x = gutter_w + pad + caret_col as f32 * char_w;
            snapshot.append_color(
                &theme.caret,
                &graphene::Rect::new(caret_x, caret_y, 1.5, row_h as f32 - 2.0),
            );
            self.caret_blink.set(!self.caret_blink.get());
        }
    }

    impl EditorView {
        /// Scroll the viewport so that the caret line stays visible.
        pub(super) fn ensure_caret_visible(&self) {
            let line = {
                let doc = self.doc.borrow();
                let sel = *self.selection.borrow();
                doc.byte_to_line(sel.head)
            };
            self.scroll.borrow_mut().scroll_to_row(line);
        }
    }
}
