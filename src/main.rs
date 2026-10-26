use gtk4::prelude::*;

mod app;
mod fs;
mod syntax;
mod ui;

fn main() {
    gtk4::init().expect("gtk init failed");

    let application = gtk4::Application::builder()
        .application_id("dev.npad.Editor")
        .flags(gio::ApplicationFlags::empty())
        .build();

    application.connect_activate(move |app| {
        let prefs = ui::prefs::Prefs::load();
        let state = app::App::new(app, prefs);
        // The demo tabs only appear when the session restored no file, so a
        // restored workspace keeps its own active tab.
        if state.is_empty() {
            state.seed(SEED_TEXT);
            state.seed(NOTE_TEXT);
        }
        let shot = std::env::var("NPAD_SHOT").ok();
        let autoclose = std::env::var_os("NPAD_AUTOCLOSE").is_some();
        if shot.is_some() || autoclose {
            let window = state.window().clone();
            glib::idle_add_local_once(move || {
                if let Some(path) = shot {
                    app::capture_to_png(&window, &path);
                }
                if autoclose {
                    state.request_close();
                }
            });
        }
    });

    std::process::exit(i32::from(application.run()));
}

const SEED_TEXT: &str = "use std::collections::HashMap;\n\nfn main() {\n    let mut m = HashMap::new();\n    m.insert(\"npad\", \"linux\");\n}\n";

const NOTE_TEXT: &str = "# npad\n\n- tabs, menus and a theme switch\n- Ctrl+D toggles the theme\n";
