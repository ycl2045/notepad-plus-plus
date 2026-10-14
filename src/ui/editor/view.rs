//! Editor view: GObject subclass, self-managed scrolling, snapshot rendering.
//!
//! This is the crown-jewel module. Everything the user sees is drawn here.
//! Verified facts it relies on (see AGENTS.md): the crate is `gtk4`;
//! `Snapshot` is `gtk4::Snapshot`; `measure` returns a 4-tuple; IME goes through
//! `EventControllerKey::set_im_context`; `glib::wrapper!` puts `@extends` first.
//!
//! OWNED BY: U1 (structure + verified spike) → U7 (input, IME, caret scrolling).

use std::cell::RefCell;

use gtk4::prelude::*;
use gtk4::{graphene, pango, Accessible, Buildable, ConstraintTarget, Snapshot, subclass::prelude::*};

use crate::ui::editor::buffer::{Document, EditKind};
use crate::ui::editor::cursor::Selection;
use crate::ui::editor::highlight::Highlighter;
use crate::ui::editor::rowmap::RowMap;
use crate::ui::editor::scroll::ScrollState;
use crate::ui::theme::Theme;

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
        if let Some(im) = imp.im_context.borrow().as_ref() {
            let ctrl = gtk4::EventControllerKey::new();
            ctrl.set_im_context(Some(im));
            view.add_controller(ctrl);
        }
        view
    }

    pub fn insert_text_at_caret(&self, text: &str) {
        let imp = self.imp();
        let mut sel = *imp.selection.borrow();
        let (start, end) = sel.range();
        imp.doc.borrow_mut().replace(start, end - start, text, EditKind::Insert);
        imp.doc.borrow_mut().epoch;
        let new_pos = start + text.len();
        sel.anchor = new_pos;
        sel.head = new_pos;
        *imp.selection.borrow_mut() = sel;
        self.queue_draw();
    }

    pub fn backspace(&self) {
        let imp = self.imp();
        let sel = *imp.selection.borrow();
        let (start, end) = sel.range();
        let (s, len) = if start == end {
            if start == 0 {
                return;
            }
            let before = imp.doc.borrow().slice(0, start);
            let ch = before.chars().last().unwrap_or('\u{0}');
            (start - ch.len_utf8(), ch.len_utf8())
        } else {
            (start, end - start)
        };
        imp.doc.borrow_mut().replace(s, len, "", EditKind::Delete);
        *imp.selection.borrow_mut() = Selection::new(s);
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
        self.queue_draw();
    }

    pub fn doc_text(&self) -> String {
        self.imp().doc.borrow().text()
    }

    pub fn set_theme(&self, theme: Theme) {
        *self.imp().theme.borrow_mut() = theme;
        self.queue_draw();
    }

    pub fn selection(&self) -> Selection {
        *self.imp().selection.borrow()
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
            if self.im_context.borrow().is_none() {
                let im = gtk4::IMMulticontext::new();
                im.set_use_preedit(false);
                *self.im_context.borrow_mut() = Some(im);
            }
        }
    }

    impl WidgetImpl for EditorView {
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
            let width = widget.allocated_width() as f32;
            let height = widget.allocated_height() as f32;

            let theme = *self.theme.borrow();
            let doc = self.doc.borrow();
            let sel = *self.selection.borrow();
            let mut scroll = self.scroll.borrow_mut();
            let mut rowmap = self.rowmap.borrow_mut();
            let row_h = scroll.row_h;
            rowmap.ensure(&doc, false, 0);
            let total_rows = rowmap.total_rows();
            scroll.content_h = (total_rows as i32) * row_h;
            scroll.clamp();

            if let Some(layout) = self.layout_cache.borrow().as_ref() {
                layout.set_text("Ag");
                let (_mw, mh) = layout.pixel_size();
                scroll.row_h = mh.max(16);
            }
            let row_h = scroll.row_h;
            let first = scroll.first_row();
            let visible = scroll.visible_row_count();

            let gutter_w = RowMap::gutter_width(total_rows, 8, 8) as f32;
            let pad = 8.0f32;

            snapshot.append_color(&theme.bg, &graphene::Rect::new(0.0, 0.0, width, height));
            snapshot.append_color(&theme.gutter, &graphene::Rect::new(0.0, 0.0, gutter_w, height));

            let caret_line = doc.byte_to_line(sel.head);

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
                    if let Some(layout) = self.layout_cache.borrow().as_ref() {
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
            let x = gutter_w + pad + caret_col as f32 * 8.0;
            let _ = caret_col;
            let _ = x;
            snapshot.append_color(
                &theme.caret,
                &graphene::Rect::new(gutter_w + pad, caret_y, 1.5, row_h as f32 - 2.0),
            );
            self.caret_blink.set(!self.caret_blink.get());
        }
    }

    impl EditorView {}
}
