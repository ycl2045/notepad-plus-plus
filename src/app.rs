#![allow(dead_code)]
//! Application shell: window, menu, actions, accelerators, status bar.
//!
//! OWNED BY: U9 (notebook) → U11 (menu/actions) → U12 (theme/status bar).

use gtk4::prelude::*;
use gtk4::{gio, glib};

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use crate::ui::notebook::{CloseDecision, TabManager};
use crate::ui::prefs::Prefs;
use crate::ui::session::{SessionState, WindowState};
use crate::ui::theme::{Theme, ThemeMode};

/// How often the application looks at the visible tab to notice edits and to
/// refresh the status bar.
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// State shared between the window and the action callbacks.
struct Shared {
    window: gtk4::ApplicationWindow,
    tabs: TabManager,
    status: gtk4::Label,
    prefs: RefCell<Prefs>,
    theme_mode: Cell<ThemeMode>,
    css: gtk4::CssProvider,
    language_action: RefCell<Option<gio::SimpleAction>>,
    /// The session: the recent-files list and, after a close, the open files.
    session: RefCell<SessionState>,
    /// True while a close sequence is in flight, so a second close request
    /// does not start a second sequence.
    closing: Cell<bool>,
}

impl Shared {
    fn update_status(&self) {
        let text = self.tabs.status_text();
        self.status.set_text(&text);
        let name = self.tabs.current_name();
        if name.is_empty() {
            self.window.set_title(Some("npad"));
        } else {
            self.window.set_title(Some(&format!("{name} - npad")));
        }
        if let Some(action) = self.language_action.borrow().as_ref() {
            action.set_state(&glib::Variant::from(self.tabs.current_language().as_str()));
        }
    }

    fn toggle_theme(&self) {
        let next = match self.theme_mode.get() {
            ThemeMode::Dark => ThemeMode::Light,
            ThemeMode::Light => ThemeMode::Dark,
        };
        self.theme_mode.set(next);
        self.tabs.set_theme(Theme::from_mode(next));
        let prefs = self.prefs.borrow();
        self.css.load_from_string(&css_for(next, &prefs));
        drop(prefs);
        self.prefs.borrow_mut().theme = next;
        let snapshot = self.prefs.borrow().clone();
        let _ = snapshot.save();
        self.update_status();
    }

    fn zoom(&self, delta: i16) {
        {
            let mut prefs = self.prefs.borrow_mut();
            let size = prefs.font_size as i16 + delta;
            prefs.font_size = size.clamp(6, 48) as u16;
        }
        let prefs = self.prefs.borrow().clone();
        self.css
            .load_from_string(&css_for(self.theme_mode.get(), &prefs));
        let _ = prefs.save();
    }

    fn set_language(&self, language: &str) {
        if let Some(index) = self.tabs.current_index() {
            self.tabs.set_language(index, language);
        }
        self.update_status();
    }

    fn poll(&self) {
        let Some(index) = self.tabs.current_index() else {
            return;
        };
        if let Some(text) = self.tabs.refresh(index) {
            self.status.set_text(&text);
        }
    }
}

pub struct App {
    window: gtk4::ApplicationWindow,
    shared: Rc<Shared>,
}

impl App {
    pub fn new(app: &gtk4::Application, prefs: Prefs) -> Self {
        let theme_mode = prefs.theme;
        let css = install_css(theme_mode, &prefs);
        let tabs = TabManager::new(Theme::from_mode(theme_mode));

        // The session file is always read: it carries the recent-files list
        // even when the workspace itself is not restored.
        let mut session = SessionState::load();
        let restore = prefs.restore_session;
        if restore {
            // Files that disappeared since the last run must not be reopened.
            session.prune_missing();
        } else {
            session.open_files.clear();
            session.active_index = 0;
        }
        let (width, height) = if restore {
            (
                session.window.width.max(400),
                session.window.height.max(300),
            )
        } else {
            (1000, 700)
        };

        let window = gtk4::ApplicationWindow::builder()
            .application(app)
            .title("npad")
            .default_width(width)
            .default_height(height)
            .build();
        window.add_css_class("npad-window");

        let status = gtk4::Label::new(None);
        status.set_halign(gtk4::Align::Start);
        status.set_margin_start(12);
        status.set_margin_end(12);
        status.add_css_class("npad-status");

        let shared = Rc::new(Shared {
            window: window.clone(),
            tabs: tabs.clone(),
            status,
            prefs: RefCell::new(prefs),
            theme_mode: Cell::new(theme_mode),
            css,
            language_action: RefCell::new(None),
            session: RefCell::new(session),
            closing: Cell::new(false),
        });

        let menu = build_menu();
        let menu_bar = gtk4::PopoverMenuBar::from_model(Some(&menu));

        let notebook = tabs.notebook();
        notebook.set_hexpand(true);
        notebook.set_vexpand(true);

        let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        vbox.append(&menu_bar);
        vbox.append(&notebook);
        vbox.append(&shared.status);
        window.set_child(Some(&vbox));

        let actions = build_actions(&shared);
        window.insert_action_group("app", Some(&actions));
        install_accels(app);

        let switch_shared = shared.clone();
        notebook.connect_switch_page(move |_, _, _| {
            switch_shared.update_status();
        });

        if restore {
            let (active_index, files) = {
                let session = shared.session.borrow();
                (session.active_index, session.open_files.clone())
            };
            let mut active_page: Option<u32> = None;
            for (index, file) in files.iter().enumerate() {
                match tabs.open_path(&file.path) {
                    Ok(page) => {
                        if index == active_index {
                            active_page = Some(page as u32);
                        }
                        // The caret goes first: it scrolls the caret into view,
                        // and the restored scroll offset has to win over that.
                        if let Some(editor) = tabs.editor_at(page) {
                            editor.set_caret(file.caret);
                            editor.set_scroll_offset(file.scroll);
                        }
                    }
                    Err(err) => {
                        shared
                            .status
                            .set_text(&format!("Cannot open {:?}: {err}", file.path));
                    }
                }
            }
            if let Some(page) = active_page {
                notebook.set_current_page(Some(page));
            }
        }

        window.present();
        shared.update_status();
        start_poll(shared.clone());

        let close_shared = shared.clone();
        window.connect_close_request(move |_| {
            begin_close(&close_shared);
            glib::Propagation::Stop
        });

        App { window, shared }
    }

    /// Put `text` into a fresh first tab (demo content).
    pub fn seed(&self, text: &str) {
        self.shared.tabs.new_tab_with_text(text);
        self.shared.update_status();
    }

    /// Whether no file is open.
    pub fn is_empty(&self) -> bool {
        self.shared.tabs.is_empty()
    }

    /// Start the close sequence: ask about the unsaved changes, then persist
    /// the session and destroy the window.
    pub fn request_close(&self) {
        begin_close(&self.shared);
    }

    pub fn window(&self) -> &gtk4::ApplicationWindow {
        &self.window
    }
}

/// Build the menu model: File / Edit / View / Language.
fn build_menu() -> gio::Menu {
    let menu = gio::Menu::new();

    let file_menu = gio::Menu::new();
    file_menu.append(Some("New"), Some("app.new"));
    file_menu.append(Some("Open..."), Some("app.open"));
    file_menu.append(Some("Save"), Some("app.save"));
    file_menu.append(Some("Save As..."), Some("app.save-as"));
    file_menu.append(Some("Close Tab"), Some("app.close-tab"));
    let quit_menu = gio::Menu::new();
    quit_menu.append(Some("Quit"), Some("app.quit"));
    file_menu.append_section(None, &quit_menu);
    menu.append_submenu(Some("File"), &file_menu);

    let edit_menu = gio::Menu::new();
    edit_menu.append(Some("Undo"), Some("app.undo"));
    edit_menu.append(Some("Redo"), Some("app.redo"));
    let clip_menu = gio::Menu::new();
    clip_menu.append(Some("Cut"), Some("app.cut"));
    clip_menu.append(Some("Copy"), Some("app.copy"));
    clip_menu.append(Some("Paste"), Some("app.paste"));
    clip_menu.append(Some("Select All"), Some("app.select-all"));
    edit_menu.append_section(None, &clip_menu);
    let search_menu = gio::Menu::new();
    search_menu.append(Some("Find..."), Some("app.find"));
    search_menu.append(Some("Go to Line..."), Some("app.goto-line"));
    edit_menu.append_section(None, &search_menu);
    menu.append_submenu(Some("Edit"), &edit_menu);

    let view_menu = gio::Menu::new();
    view_menu.append(Some("Toggle Theme"), Some("app.toggle-theme"));
    view_menu.append(Some("Zoom In"), Some("app.zoom-in"));
    view_menu.append(Some("Zoom Out"), Some("app.zoom-out"));
    menu.append_submenu(Some("View"), &view_menu);

    let language_menu = gio::Menu::new();
    for (display, syntax) in crate::syntax::menu_entries() {
        let item = gio::MenuItem::new(Some(display), None);
        item.set_action_and_target_value(Some("app.language"), Some(&glib::Variant::from(syntax)));
        language_menu.append_item(&item);
    }
    menu.append_submenu(Some("Language"), &language_menu);

    menu
}

/// Register the keyboard shortcuts on the application.
fn install_accels(app: &gtk4::Application) {
    app.set_accels_for_action("app.new", &["<Primary>n"]);
    app.set_accels_for_action("app.open", &["<Primary>o"]);
    app.set_accels_for_action("app.save", &["<Primary>s"]);
    app.set_accels_for_action("app.save-as", &["<Primary><Shift>s"]);
    app.set_accels_for_action("app.close-tab", &["<Primary>w"]);
    app.set_accels_for_action("app.quit", &["<Primary>q"]);
    app.set_accels_for_action("app.undo", &["<Primary>z"]);
    app.set_accels_for_action("app.redo", &["<Primary>y"]);
    app.set_accels_for_action("app.find", &["<Primary>f"]);
    app.set_accels_for_action("app.goto-line", &["<Primary>g"]);
    app.set_accels_for_action("app.toggle-theme", &["<Primary>d"]);
    app.set_accels_for_action("app.zoom-in", &["<Primary>plus", "<Primary>equal"]);
    app.set_accels_for_action("app.zoom-out", &["<Primary>minus"]);
}

/// Build the action group that backs the menu and the shortcuts.
fn build_actions(shared: &Rc<Shared>) -> gio::SimpleActionGroup {
    let group = gio::SimpleActionGroup::new();

    {
        let shared = shared.clone();
        let action = gio::SimpleAction::new("new", None);
        action.connect_activate(move |_, _| {
            shared.tabs.new_tab();
            shared.update_status();
        });
        group.add_action(&action);
    }

    {
        let shared = shared.clone();
        let action = gio::SimpleAction::new("open", None);
        action.connect_activate(move |_, _| open_file(&shared));
        group.add_action(&action);
    }

    {
        let shared = shared.clone();
        let action = gio::SimpleAction::new("save", None);
        action.connect_activate(move |_, _| save_file(&shared, false));
        group.add_action(&action);
    }

    {
        let shared = shared.clone();
        let action = gio::SimpleAction::new("save-as", None);
        action.connect_activate(move |_, _| save_file(&shared, true));
        group.add_action(&action);
    }

    {
        let shared = shared.clone();
        let action = gio::SimpleAction::new("close-tab", None);
        action.connect_activate(move |_, _| close_tab(&shared));
        group.add_action(&action);
    }

    {
        let shared = shared.clone();
        let action = gio::SimpleAction::new("quit", None);
        action.connect_activate(move |_, _| {
            shared.window.close();
        });
        group.add_action(&action);
    }

    {
        let shared = shared.clone();
        let action = gio::SimpleAction::new("undo", None);
        action.connect_activate(move |_, _| {
            if let Some(index) = shared.tabs.current_index() {
                shared.tabs.undo(index);
            }
            shared.update_status();
        });
        group.add_action(&action);
    }

    {
        let shared = shared.clone();
        let action = gio::SimpleAction::new("redo", None);
        action.connect_activate(move |_, _| {
            if let Some(index) = shared.tabs.current_index() {
                shared.tabs.redo(index);
            }
            shared.update_status();
        });
        group.add_action(&action);
    }

    {
        let shared = shared.clone();
        let action = gio::SimpleAction::new("cut", None);
        action.connect_activate(move |_, _| cut_selection(&shared));
        group.add_action(&action);
    }

    {
        let shared = shared.clone();
        let action = gio::SimpleAction::new("copy", None);
        action.connect_activate(move |_, _| copy_selection(&shared));
        group.add_action(&action);
    }

    {
        let shared = shared.clone();
        let action = gio::SimpleAction::new("paste", None);
        action.connect_activate(move |_, _| paste_clipboard(&shared));
        group.add_action(&action);
    }

    {
        let action = gio::SimpleAction::new("select-all", None);
        action.connect_activate(|_, _| {
            eprintln!("npad: select all needs a public selection setter on EditorView");
        });
        group.add_action(&action);
    }

    {
        let action = gio::SimpleAction::new("find", None);
        action.connect_activate(|_, _| {
            eprintln!("npad: the find/replace panel is not part of this milestone");
        });
        group.add_action(&action);
    }

    {
        let action = gio::SimpleAction::new("goto-line", None);
        action.connect_activate(|_, _| {
            eprintln!("npad: go to line needs a public caret setter on EditorView");
        });
        group.add_action(&action);
    }

    {
        let shared = shared.clone();
        let action =
            gio::SimpleAction::new_stateful("toggle-theme", None, &glib::Variant::from(false));
        action.connect_activate(move |action, _| {
            shared.toggle_theme();
            let dark = matches!(shared.theme_mode.get(), ThemeMode::Dark);
            action.set_state(&glib::Variant::from(dark));
        });
        group.add_action(&action);
    }

    {
        let shared = shared.clone();
        let action = gio::SimpleAction::new("zoom-in", None);
        action.connect_activate(move |_, _| shared.zoom(1));
        group.add_action(&action);
    }

    {
        let shared = shared.clone();
        let action = gio::SimpleAction::new("zoom-out", None);
        action.connect_activate(move |_, _| shared.zoom(-1));
        group.add_action(&action);
    }

    {
        let shared = shared.clone();
        let action = gio::SimpleAction::new_stateful(
            "language",
            Some(glib::VariantTy::STRING),
            &glib::Variant::from("Plain Text"),
        );
        let owner = shared.clone();
        action.connect_activate(move |action, parameter| {
            let language = parameter
                .and_then(|value| value.get::<String>())
                .unwrap_or_else(|| String::from("Plain Text"));
            owner.set_language(&language);
            action.set_state(&glib::Variant::from(language.as_str()));
        });
        group.add_action(&action);
        *shared.language_action.borrow_mut() = Some(action);
    }

    group
}

/// Open the file chooser and load the picked file into a new tab.
fn open_file(shared: &Rc<Shared>) {
    let dialog = gtk4::FileDialog::new();
    dialog.set_title("Open File");
    let shared = shared.clone();
    let window = shared.window.clone();
    dialog.open(Some(&window), None::<&gio::Cancellable>, move |result| {
        let Ok(file) = result else {
            return;
        };
        let Some(path) = file.path() else {
            return;
        };
        if let Err(err) = shared.tabs.open_path(&path) {
            shared
                .status
                .set_text(&format!("Cannot open {path:?}: {err}"));
        } else {
            shared.session.borrow_mut().touch_recent(&path);
            let session = build_session(&shared);
            let _ = session.save();
        }
        shared.update_status();
    });
}

/// Save the visible tab; `force_dialog` always asks for a file name.
fn save_file(shared: &Rc<Shared>, force_dialog: bool) {
    let Some(index) = shared.tabs.current_index() else {
        return;
    };
    if !force_dialog && let Some(path) = shared.tabs.current_path() {
        if let Err(err) = shared.tabs.save_tab(index, &path) {
            shared
                .status
                .set_text(&format!("Cannot save {path:?}: {err}"));
        }
        shared.update_status();
        return;
    }
    save_as(shared, Some(index));
}

/// Ask for a file name and write the visible tab there.
fn save_as(shared: &Rc<Shared>, index: Option<usize>) {
    let Some(index) = index.or_else(|| shared.tabs.current_index()) else {
        return;
    };
    let dialog = gtk4::FileDialog::new();
    dialog.set_title("Save File");
    if let Some(path) = shared.tabs.tab_path(index)
        && let Some(name) = path.file_name()
    {
        dialog.set_initial_name(Some(&name.to_string_lossy()));
    }
    let shared = shared.clone();
    let window = shared.window.clone();
    dialog.save(Some(&window), None::<&gio::Cancellable>, move |result| {
        let Ok(file) = result else {
            return;
        };
        let Some(path) = file.path() else {
            return;
        };
        if let Err(err) = shared.tabs.save_tab(index, &path) {
            shared
                .status
                .set_text(&format!("Cannot save {path:?}: {err}"));
        }
        shared.update_status();
    });
}

/// Close the visible tab, asking about unsaved changes.
fn close_tab(shared: &Rc<Shared>) {
    let Some(index) = shared.tabs.current_index() else {
        return;
    };
    let shared_for_dialog = shared.clone();
    shared
        .tabs
        .request_close(index, &shared.window, move |decision| {
            match decision {
                CloseDecision::Save => {
                    let path = shared_for_dialog.tabs.tab_path(index);
                    match path {
                        Some(path) => {
                            if shared_for_dialog.tabs.save_tab(index, &path).is_ok() {
                                shared_for_dialog.tabs.remove_tab(index);
                            }
                        }
                        None => save_as_then_close(&shared_for_dialog, index),
                    }
                }
                CloseDecision::Close | CloseDecision::Cancel => {}
            }
            shared_for_dialog.update_status();
        });
}

/// Save the tab at `index` into a new file and close it afterwards.
fn save_as_then_close(shared: &Rc<Shared>, index: usize) {
    let dialog = gtk4::FileDialog::new();
    dialog.set_title("Save File");
    let shared = shared.clone();
    let window = shared.window.clone();
    dialog.save(Some(&window), None::<&gio::Cancellable>, move |result| {
        let Ok(file) = result else {
            return;
        };
        let Some(path) = file.path() else {
            return;
        };
        if shared.tabs.save_tab(index, &path).is_ok() {
            shared.tabs.remove_tab(index);
        }
        shared.update_status();
    });
}

/// The session state to persist: the open files, the recent-files list, the
/// window geometry and the preferences.
fn build_session(shared: &Rc<Shared>) -> SessionState {
    let (open_files, active_index) = shared.tabs.session_snapshot();
    let mut session = shared.session.borrow().clone();
    session.open_files = open_files;
    session.active_index = active_index;
    session.window = WindowState {
        width: shared.window.width(),
        height: shared.window.height(),
    };
    session.prefs = shared.prefs.borrow().clone();
    session
}

/// Handle a close request: ask about the unsaved changes first, then persist
/// the session and destroy the window.
fn begin_close(shared: &Rc<Shared>) {
    if shared.closing.get() {
        // A close sequence is already running: a second request must not
        // start another one on top of it.
        return;
    }
    shared.closing.set(true);
    let unsaved = shared.tabs.unsaved_indices();
    if unsaved.is_empty() {
        finish_close(shared);
        return;
    }
    confirm_close_next(shared, unsaved, 0);
}

/// Ask about the unsaved tab at `unsaved[position]`, then go on with the next
/// one. `Cancel` leaves the window open.
fn confirm_close_next(shared: &Rc<Shared>, unsaved: Vec<usize>, position: usize) {
    let Some(&index) = unsaved.get(position) else {
        finish_close(shared);
        return;
    };
    let name = shared.tabs.tab_name(index);
    let dialog = Rc::new(
        gtk4::AlertDialog::builder()
            .message(format!("Save changes to {name}?"))
            .detail("Your changes will be lost if you do not save them.")
            .buttons(gtk4::glib::StrV::from(["Cancel", "Save", "Discard"]))
            .cancel_button(0)
            .default_button(1)
            .build(),
    );
    let shared = shared.clone();
    let window = shared.window.clone();
    let keep_alive = dialog.clone();
    dialog.choose(Some(&window), None::<&gio::Cancellable>, move |result| {
        let _keep_alive = keep_alive;
        match result {
            // Button 1 is "Save".
            Ok(1) => match shared.tabs.tab_path(index) {
                Some(path) => {
                    if let Err(err) = shared.tabs.save_tab(index, &path) {
                        shared
                            .status
                            .set_text(&format!("Cannot save {path:?}: {err}"));
                        shared.closing.set(false);
                        return;
                    }
                    confirm_close_next(&shared, unsaved, position + 1);
                }
                None => save_then_continue(&shared, index, unsaved, position + 1),
            },
            // Button 2 is "Discard": the changes are thrown away.
            Ok(2) => confirm_close_next(&shared, unsaved, position + 1),
            // Button 0 is "Cancel": the window stays open.
            _ => shared.closing.set(false),
        }
    });
}

/// Ask for a file name for an untitled tab, then continue the close sequence.
fn save_then_continue(shared: &Rc<Shared>, index: usize, unsaved: Vec<usize>, position: usize) {
    let dialog = gtk4::FileDialog::new();
    dialog.set_title("Save File");
    let shared = shared.clone();
    let window = shared.window.clone();
    dialog.save(Some(&window), None::<&gio::Cancellable>, move |result| {
        let Ok(file) = result else {
            shared.closing.set(false);
            return;
        };
        let Some(path) = file.path() else {
            shared.closing.set(false);
            return;
        };
        if let Err(err) = shared.tabs.save_tab(index, &path) {
            shared
                .status
                .set_text(&format!("Cannot save {path:?}: {err}"));
            shared.closing.set(false);
            return;
        }
        confirm_close_next(&shared, unsaved, position);
    });
}

/// Persist the session and destroy the window.
///
/// The destroy is deferred to an idle callback because the window can be
/// closed from inside its own `close-request` handler.
fn finish_close(shared: &Rc<Shared>) {
    let session = build_session(shared);
    let _ = session.save();
    let window = shared.window.clone();
    glib::idle_add_local_once(move || {
        window.destroy();
    });
}

/// Copy the selection of the visible tab to the clipboard.
fn copy_selection(shared: &Rc<Shared>) {
    let Some(editor) = shared.tabs.current_editor() else {
        return;
    };
    let selection = editor.selection();
    let (start, end) = selection.range();
    if start == end {
        return;
    }
    let text = editor.doc_text();
    let slice = text.get(start..end).unwrap_or_default().to_string();
    if let Some(display) = gtk4::gdk::Display::default() {
        display.clipboard().set_text(&slice);
    }
}

/// Copy the selection to the clipboard and delete it from the document.
fn cut_selection(shared: &Rc<Shared>) {
    let Some(editor) = shared.tabs.current_editor() else {
        return;
    };
    let selection = editor.selection();
    let (start, end) = selection.range();
    if start == end {
        return;
    }
    let text = editor.doc_text();
    let Some(slice) = text.get(start..end) else {
        return;
    };
    let slice = slice.to_string();
    if let Some(display) = gtk4::gdk::Display::default() {
        display.clipboard().set_text(&slice);
    }
    let mut remaining = String::with_capacity(text.len());
    remaining.push_str(text.get(..start).unwrap_or_default());
    remaining.push_str(text.get(end..).unwrap_or_default());
    editor.load_text(&remaining);
    shared.update_status();
}

/// Insert the clipboard content at the caret of the visible tab.
fn paste_clipboard(shared: &Rc<Shared>) {
    if shared.tabs.is_empty() {
        return;
    }
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let clipboard = display.clipboard();
    let shared = shared.clone();
    clipboard.read_text_async(None::<&gio::Cancellable>, move |result| {
        let Ok(Some(text)) = result else {
            return;
        };
        if let Some(editor) = shared.tabs.current_editor() {
            editor.insert_text_at_caret(&text);
        }
        shared.update_status();
    });
}

/// Look at the visible tab now and then: notices edits and refreshes the
/// status bar.
fn start_poll(shared: Rc<Shared>) {
    glib::timeout_add_local(POLL_INTERVAL, move || {
        shared.poll();
        glib::ControlFlow::Continue
    });
}

/// CSS for the window chrome; the editor paints itself.
fn css_for(mode: ThemeMode, prefs: &Prefs) -> String {
    let (bg, fg, panel) = match mode {
        ThemeMode::Dark => ("#1c1f24", "#d8dbe0", "#292b33"),
        ThemeMode::Light => ("#ffffff", "#1f1f1f", "#f0f1f3"),
    };
    format!(
        "window.npad-window {{ background: {bg}; color: {fg}; }}
         headerbar {{ min-height: 34px; background: {panel}; }}
         notebook header {{ background: {panel}; }}
         .npad-status {{ background: {panel}; color: {fg}; }}
         label {{ font-family: {family}; font-size: {size}pt; color: {fg}; }}",
        family = sanitise_family(&prefs.font_family),
        size = prefs.font_size,
    )
}

/// Keep only characters that are safe inside a CSS font-family value.
fn sanitise_family(family: &str) -> String {
    let cleaned: String = family
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
        .collect();
    if cleaned.trim().is_empty() {
        String::from("monospace")
    } else {
        cleaned
    }
}

/// Install the application CSS and return the provider so that the theme can
/// be switched later.
pub fn install_css(mode: ThemeMode, prefs: &Prefs) -> gtk4::CssProvider {
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(&css_for(mode, prefs));
    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    provider
}

/// Render the window into a PNG file.
pub fn capture_to_png(window: &gtk4::ApplicationWindow, path: &str) -> bool {
    let Some(content) = window.child() else {
        return false;
    };
    let w = content.width().max(1);
    let h = content.height().max(1);
    let snapshot = gtk4::Snapshot::new();
    let paintable = gtk4::WidgetPaintable::new(Some(&content));
    paintable.snapshot(&snapshot, w as f64, h as f64);
    let Some(node) = snapshot.to_node() else {
        return false;
    };
    let Some(native) = window.native() else {
        return false;
    };
    let Some(renderer) = native.renderer() else {
        return false;
    };
    let rect = gtk4::graphene::Rect::new(0.0, 0.0, w as f32, h as f32);
    let texture = renderer.render_texture(&node, Some(&rect));
    texture.save_to_png(std::path::Path::new(path)).is_ok()
}

/// Path of a file the user picked, used by the tests.
pub fn path_of(file: &gio::File) -> Option<PathBuf> {
    file.path()
}
