use gtk4::prelude::*;
use gtk4::{gio, glib};

use std::path::Path;

use crate::ui::editor::EditorView;
use crate::ui::theme::Theme;

pub struct App {
    window: gtk4::ApplicationWindow,
    editor: EditorView,
    theme: Theme,
}

impl App {
    pub fn new(app: &gtk4::Application) -> Self {
        let theme = Theme::dark();

        let window = gtk4::ApplicationWindow::builder()
            .application(app)
            .title("npad")
            .default_width(1000)
            .default_height(700)
            .build();

        window.add_css_class("npad-window");

        let editor = EditorView::new();
        editor.set_theme(theme);
        editor.set_hexpand(true);
        editor.set_vexpand(true);
        editor.set_focusable(true);
        editor.set_can_focus(true);

        let header = gtk4::HeaderBar::new();
        header.set_title_widget(Some(&gtk4::Label::new(Some("npad"))));
        window.set_titlebar(Some(&header));

        let status = gtk4::Label::new(Some("Ln 1, Col 1  |  UTF-8  |  LF  |  Plain Text"));
        status.set_halign(gtk4::Align::Start);
        status.set_margin_start(12);

        let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        vbox.append(&editor);
        vbox.append(&status);

        let scroller = gtk4::ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Automatic)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .child(&vbox)
            .build();

        window.set_child(Some(&scroller));
        window.present();

        let menu = gio::Menu::new();
        menu.append(Some("Toggle Theme"), Some("app.theme"));
        menu.append(Some("Quit"), Some("app.quit"));

        let menu_btn = gtk4::MenuButton::new();
        menu_btn.set_icon_name("open-menu-symbolic");
        menu_btn.set_menu_model(Some(&menu));
        header.pack_end(&menu_btn);

        let actions = gio::SimpleActionGroup::new();
        let theme_action = gio::SimpleAction::new("theme", None);
        theme_action.connect_activate(|_, _| {});
        actions.add_action(&theme_action);

        let quit_action = gio::SimpleAction::new("quit", None);
        let app_weak = app.downgrade();
        quit_action.connect_activate(move |_, _| {
            if let Some(app) = app_weak.upgrade() {
                app.quit();
            }
        });
        actions.add_action(&quit_action);
        window.insert_action_group("app", Some(&actions));

        App { window, editor, theme }
    }

    pub fn seed(&self, text: &str) {
        self.editor.load_text(text);
    }

    pub fn editor(&self) -> &EditorView {
        &self.editor
    }

    pub fn window(&self) -> &gtk4::ApplicationWindow {
        &self.window
    }
}

pub fn install_css() {
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(
        "window.npad-window { background: @theme_bg_color; }
         headerbar { min-height: 34px; }
         label { font-family: monospace; font-size: 11pt; }",
    );
    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    let _ = glib::markup_escape_text("npad");
}

pub fn capture_to_png(view: &EditorView, path: &str) -> bool {
    let w = view.width().max(1);
    let h = view.height().max(1);
    let snapshot = gtk4::Snapshot::new();
    let paintable = gtk4::WidgetPaintable::new(Some(view));
    paintable.snapshot(&snapshot, w as f64, h as f64);
    let Some(node) = snapshot.to_node() else {
        return false;
    };
    let Some(native) = view.native() else {
        return false;
    };
    let Some(renderer) = native.renderer() else {
        return false;
    };
    let rect = gtk4::graphene::Rect::new(0.0, 0.0, w as f32, h as f32);
    let texture = renderer.render_texture(&node, Some(&rect));
    texture.save_to_png(Path::new(path)).is_ok()
}
