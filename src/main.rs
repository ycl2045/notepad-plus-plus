use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;

mod app;
mod fs;
mod syntax;
mod ui;

/// Delay between the startup and the action that the test hook activates.
const HOOK_ACTION_DELAY: Duration = Duration::from_millis(100);

/// Delay between the action and the capture. The capture has to follow the
/// frame that renders the effect of the action, so it cannot be part of the
/// same callback.
const HOOK_CAPTURE_DELAY: Duration = Duration::from_millis(180);

fn main() {
    gtk4::init().expect("gtk init failed");

    let application = gtk4::Application::builder()
        .application_id("dev.npad.Editor")
        .flags(gio::ApplicationFlags::empty())
        .build();

    application.connect_activate(move |app| {
        let prefs = ui::prefs::Prefs::load();
        let state = Rc::new(app::App::new(app, prefs));
        // The demo tabs only appear when the session restored no file, so a
        // restored workspace keeps its own active tab.
        if state.is_empty() {
            state.seed(SEED_TEXT);
            state.seed(NOTE_TEXT);
        }
        let shot = std::env::var("NPAD_SHOT").ok();
        let action = std::env::var("NPAD_ACTION").ok();
        let action_target = std::env::var("NPAD_ACTION_TARGET").ok();
        let find = std::env::var("NPAD_FIND").ok();
        let replace = std::env::var("NPAD_REPLACE").ok();
        let autoclose = std::env::var_os("NPAD_AUTOCLOSE").is_some();
        if shot.is_some() || action.is_some() || find.is_some() || replace.is_some() || autoclose {
            let window = state.window().clone();
            let action_state = state.clone();
            glib::timeout_add_local_once(HOOK_ACTION_DELAY, move || {
                if let Some(name) = action {
                    action_state.activate_with(&name, action_target.as_deref());
                }
                if let Some(pattern) = find {
                    action_state.search(&pattern);
                }
                if let Some(replacement) = replace {
                    action_state.replace_all(&replacement);
                }
                let capture_state = state.clone();
                glib::timeout_add_local_once(HOOK_CAPTURE_DELAY, move || {
                    if let Some(path) = shot {
                        app::capture_to_png(&window, &path);
                    }
                    if autoclose {
                        capture_state.request_close();
                    }
                });
            });
        }
    });

    std::process::exit(i32::from(application.run()));
}

const SEED_TEXT: &str = "use std::collections::HashMap;\n\nfn main() {\n    let mut m = HashMap::new();\n    m.insert(\"npad\", \"linux\");\n}\n";

const NOTE_TEXT: &str = "# npad\n\n- tabs, menus and a theme switch\n- Ctrl+D toggles the theme\n";
