use std::cell::RefCell;

use gtk4::prelude::*;
use gtk4::{gdk, glib, graphene, pango, Accessible, Buildable, ConstraintTarget, subclass::prelude::*, Snapshot};

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
        let obj: Self = glib::Object::new();
        let imp = obj.imp();
        let im = gtk4::IMMulticontext::new();
        im.set_use_preedit(false);
        im.connect_commit(glib::clone!(
            #[weak]
            obj,
            move |_, text| {
                obj.imp().insert(text);
                obj.queue_draw();
            }
        ));
        *imp.im_context.borrow_mut() = Some(im);
        obj
    }

    pub fn insert_caret(&self) -> usize {
        self.imp().insert("x")
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct EditorView {
        pub lines: RefCell<Vec<String>>,
        pub cursor: RefCell<usize>,
        pub col: RefCell<usize>,
        pub im_context: RefCell<Option<gtk4::IMMulticontext>>,
        pub layout_cache: RefCell<Option<pango::Layout>>,
    }

    impl EditorView {
        pub fn ln(&self) -> usize {
            self.lines.borrow().len().max(1)
        }

        pub fn insert(&self, text: &str) -> usize {
            let mut lines = self.lines.borrow_mut();
            if lines.is_empty() {
                lines.push(String::new());
            }
            let mut col = self.col.borrow_mut();
            let cursor = *self.cursor.borrow();
            lines[cursor].insert_str(*col, text);
            *col += text.chars().count();
            *col
        }

        pub fn backspace(&self) {
            let mut col = self.col.borrow_mut();
            let cursor = self.cursor.borrow().clone();
            let mut lines = self.lines.borrow_mut();
            if *col == 0 {
                return;
            }
            lines[cursor].replace_range((*col - 1)..*col, "");
            *col -= 1;
        }

        pub fn newline(&self) {
            let mut lines = self.lines.borrow_mut();
            let col = *self.col.borrow();
            let row = *self.cursor.borrow();
            if col == 0 {
                lines.insert(row, String::new());
            } else {
                let rest = lines[row].split_off(col);
                lines.insert(row + 1, rest);
            }
            *self.cursor.borrow_mut() = row + 1;
            *self.col.borrow_mut() = 0;
        }

        pub fn pango_layout(&self, widget: &super::EditorView) -> pango::Layout {
            if let Some(l) = self.layout_cache.borrow().as_ref() {
                return l.clone();
            }
            let l = widget.create_pango_layout(Some(""));
            *self.layout_cache.borrow_mut() = Some(l.clone());
            l
        }
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
            self.lines.borrow_mut().push(String::new());
        }
    }

    impl WidgetImpl for EditorView {
        fn measure(&self, orientation: gtk4::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let obj = self.obj();
            let widget: &super::EditorView = &obj;
            let layout = self.pango_layout(widget);
            let (_iw, ih) = layout.pixel_size();
            let line_h = ih.max(16);
            match orientation {
                gtk4::Orientation::Horizontal => (800, 800, -1, -1),
                _ => {
                    let h = line_h * self.ln() as i32 + 24;
                    (h, h, -1, -1)
                }
            }
        }

        fn size_allocate(&self, _width: i32, height: i32, _baseline: i32) {
            self.parent_size_allocate(_width, height, _baseline);
        }

        fn snapshot(&self, snapshot: &Snapshot) {
            let obj = self.obj();
            let widget: &super::EditorView = &obj;
            let width = widget.allocated_width() as f32;
            let height = widget.allocated_height() as f32;
            let layout = self.pango_layout(widget);
            layout.set_text("Ag");
            let (_mw, mh) = layout.pixel_size();
            let line_h = mh.max(16) as f32;

            let bg = gdk::RGBA::new(0.11, 0.12, 0.14, 1.0);
            let fg = gdk::RGBA::new(0.85, 0.87, 0.9, 1.0);
            let gutter = gdk::RGBA::new(0.16, 0.17, 0.2, 1.0);
            let gutter_fg = gdk::RGBA::new(0.45, 0.47, 0.52, 1.0);
            let curline = gdk::RGBA::new(0.15, 0.16, 0.19, 1.0);
            let cursor_col = gdk::RGBA::new(0.85, 0.87, 0.9, 1.0);

            snapshot.append_color(&bg, &graphene::Rect::new(0.0, 0.0, width, height));

            let gutter_w = 62.0;
            let rows = self.ln();
            let cursor_row = *self.cursor.borrow();

            for i in 0..rows {
                let y = i as f32 * line_h + 8.0;
                if i == cursor_row {
                    snapshot.append_color(&curline, &graphene::Rect::new(0.0, y, width, line_h));
                }
                let text = &self.lines.borrow()[i];
                layout.set_text(text);
                snapshot.save();
                snapshot.translate(&graphene::Point::new(gutter_w, y));
                snapshot.append_layout(&layout, &fg);
                snapshot.restore();

                let n = format!("{}", i + 1);
                layout.set_text(&n);
                snapshot.save();
                snapshot.translate(&graphene::Point::new(gutter_w - 16.0, y));
                snapshot.append_layout(&layout, &gutter_fg);
                snapshot.restore();
            }

            snapshot.append_color(&gutter, &graphene::Rect::new(0.0, 0.0, gutter_w, height));

            let col = *self.col.borrow();
            let text_before = self.lines.borrow()[cursor_row][..col.min(self.lines.borrow()[cursor_row].len())].to_string();
            let w = if text_before.is_empty() {
                0.0
            } else {
                layout.set_text(&text_before);
                let (w, _) = layout.pixel_size();
                w as f32
            };
            let cy = cursor_row as f32 * line_h + 8.0;
            snapshot.append_color(
                &cursor_col,
                &graphene::Rect::new(gutter_w + w, cy + 2.0, 2.0, line_h - 4.0),
            );
        }
    }

}


fn capture_to_png(view: &EditorView, path: &str) -> bool {
    let snapshot = gtk4::Snapshot::new();
    let paintable = gtk4::WidgetPaintable::new(Some(view));
    let w = view.allocated_width().max(800);
    let h = view.allocated_height().max(400);
    paintable.snapshot(&snapshot, w as f64, h as f64);
    let Some(node) = snapshot.to_node() else {
        eprintln!("SHOT: no render node produced");
        return false;
    };
    let Some(native) = view.native() else {
        eprintln!("SHOT: no native surface");
        return false;
    };
    let Some(renderer) = native.renderer() else {
        eprintln!("SHOT: no renderer");
        return false;
    };
    let rect = graphene::Rect::new(0.0, 0.0, w as f32, h as f32);
    let texture = renderer.render_texture(&node, Some(&rect));
    let ok = texture.save_to_png(std::path::Path::new(path)).is_ok();
    eprintln!("SHOT: saved {} ({}x{}) ok={}", path, texture.width(), texture.height(), ok);
    ok
}

fn seed(view: &EditorView) {
    let lines = [
        "use std::collections::HashMap;",
        "",
        "pub fn main() {",
        "    let mut m = HashMap::new();",
        "    m.insert(\"rope\", \"buffer\");",
        "    println!(\"{:?}\", m);",
        "}",
        "",
        "# npad spike: line-number gutter + pango render",
    ];
    for l in lines {
        view.insert_text_caret(l);
        view.newline_caret();
    }
}

pub trait EditorViewExt {
    fn insert_text_caret(&self, text: &str);
    fn backspace_caret(&self);
    fn newline_caret(&self);
}

impl EditorViewExt for EditorView {
    fn insert_text_caret(&self, text: &str) {
        self.imp().insert(text);
        self.queue_draw();
    }
    fn backspace_caret(&self) {
        self.imp().backspace();
        self.queue_draw();
    }
    fn newline_caret(&self) {
        self.imp().newline();
        self.queue_draw();
    }
}

pub fn main() {
    gtk4::init().expect("gtk init failed");

    let app = gtk4::Application::builder()
        .application_id("dev.npad.Spike")
        .build();

    let shot_path: Option<String> = std::env::var("NPAD_SHOT").ok();

    app.connect_activate(move |app| {
        let win = gtk4::ApplicationWindow::builder()
            .application(app)
            .title("npad spike")
            .default_width(900)
            .default_height(600)
            .build();

        let view = EditorView::new();
        seed(&view);
        let shot = shot_path.clone();
        let view_for_shot = view.clone();
        glib::idle_add_local_once(move || {
            if let Some(path) = shot {
                capture_to_png(&view_for_shot, &path);
            }
        });
        view.set_focusable(true);
        view.set_hexpand(true);
        view.set_vexpand(true);

        let key = gtk4::EventControllerKey::new();
        if let Some(im) = view.imp().im_context.borrow().as_ref() {
            key.set_im_context(Some(im));
        }
        key.connect_key_pressed(glib::clone!(
            #[weak]
            view,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, keyval, _hw, _state| {
                match keyval {
                    gdk::Key::BackSpace => {
                        view.backspace_caret();
                        glib::Propagation::Stop
                    }
                    gdk::Key::Return => {
                        view.newline_caret();
                        glib::Propagation::Stop
                    }
                    _ => glib::Propagation::Proceed,
                }
            }
        ));
        view.add_controller(key);

        let box_ = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        box_.append(&view);
        win.set_child(Some(&box_));
        win.present();
        eprintln!("SPIKE: window presented");
    });

    eprintln!("SPIKE: running");
    let code = app.run();
    std::process::exit(i32::from(code));
}
