use gtk4::prelude::*;

mod app;
mod fs;
mod syntax;
mod ui;

fn main() {
    gtk4::init().expect("gtk init failed");
    app::install_css();

    let application = gtk4::Application::builder()
        .application_id("dev.npad.Editor")
        .flags(gio::ApplicationFlags::empty())
        .build();

    application.connect_activate(move |app| {
        let state = AppState::new(app);
        state.seed(SEED_TEXT);
        let shot = std::env::var("NPAD_SHOT").ok();
        if let Some(path) = shot {
            let editor = state.editor().clone();
            glib::idle_add_local_once(move || {
                crate::app::capture_to_png(&editor, &path);
            });
        }
    });

    std::process::exit(i32::from(application.run()));
}

use app::App as _App;
struct AppState {
    inner: _App,
}

impl AppState {
    fn new(app: &gtk4::Application) -> Self {
        AppState { inner: _App::new(app) }
    }
    fn seed(&self, text: &str) {
        self.inner.seed(text);
    }
    fn editor(&self) -> &crate::ui::editor::EditorView {
        self.inner.editor()
    }
}

const SEED_TEXT: &str = "use std::collections::HashMap;\n\nfn main() {\n    let mut m = HashMap::new();\n    m.insert(\"npad\", \"linux\");\n}\n";
