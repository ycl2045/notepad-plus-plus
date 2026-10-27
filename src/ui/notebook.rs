//! Notebook: tabbed document management (plan §4 U9).
//!
//! A [`TabManager`] owns a `gtk4::Notebook`, one `EditorView` per tab and the
//! per-tab file metadata (path, encoding, EOL, language, modified state).
//!
//! The manager is cheap to clone: every piece of state lives behind a single
//! `Rc<RefCell<_>>`, so a GTK action callback can keep a clone alive while a
//! file dialog or an alert dialog is open and call back into the manager.
//!
//! OWNED BY: U9.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::prelude::*;

use crate::fs::io::{DetectedEncoding, read_path, write_path};
use crate::ui::editor::EditorView;
use crate::ui::editor::buffer::Eol;
use crate::ui::session::SessionFile;
use crate::ui::theme::Theme;

/// Maximum number of undo snapshots kept per tab.
pub const HISTORY_CAP: usize = 32;

/// What the user chose when a modified tab is closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseDecision {
    /// Close the tab now: it had no unsaved changes, or the user chose
    /// "Discard". The tab is already gone when this is delivered.
    Close,
    /// The user chose "Save": the caller has to save the tab and close it
    /// afterwards.
    Save,
    /// The user chose "Cancel": the tab stays open.
    Cancel,
}

/// One open document.
struct Tab {
    editor: EditorView,
    label: gtk4::Label,
    /// Display name (file name or `Untitled N`), without the dirty marker.
    name: String,
    path: Option<PathBuf>,
    encoding: DetectedEncoding,
    had_bom: bool,
    eol: Eol,
    language: String,
    /// Content as it was loaded or last saved. The dirty marker is
    /// `editor text != saved`.
    saved: String,
    /// Content observed by the last poll.
    last_text: String,
    /// Undo snapshots (previous contents).
    history: Vec<String>,
    /// Redo snapshots.
    redo: Vec<String>,
}

impl Tab {
    /// Whether the tab holds changes that were never written to disk.
    fn is_modified(&self) -> bool {
        self.editor.doc_text() != self.saved
    }

    /// Refresh the tab label: `*name` while the tab has unsaved changes.
    fn refresh_label(&self) {
        let marker = if self.is_modified() { "*" } else { "" };
        self.label.set_text(&format!("{marker}{}", self.name));
    }
}

/// Mutable state behind the `Rc`.
struct Inner {
    notebook: gtk4::Notebook,
    tabs: Vec<Tab>,
    /// Counter for the `Untitled N` names.
    untitled: usize,
    theme: Theme,
}

/// Tabbed container for the open documents.
#[derive(Clone)]
pub struct TabManager {
    inner: Rc<RefCell<Inner>>,
}

impl TabManager {
    /// Create an empty manager. The notebook has no pages yet.
    pub fn new(theme: Theme) -> Self {
        let notebook = gtk4::Notebook::new();
        notebook.set_scrollable(true);
        notebook.set_show_border(false);
        TabManager {
            inner: Rc::new(RefCell::new(Inner {
                notebook,
                tabs: Vec::new(),
                untitled: 0,
                theme,
            })),
        }
    }

    /// The notebook widget, to be put into the window.
    pub fn notebook(&self) -> gtk4::Notebook {
        self.inner.borrow().notebook.clone()
    }

    /// Number of open tabs.
    pub fn len(&self) -> usize {
        self.inner.borrow().tabs.len()
    }

    /// Whether no tab is open.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Index of the visible tab.
    pub fn current_index(&self) -> Option<usize> {
        let inner = self.inner.borrow();
        inner
            .notebook
            .current_page()
            .map(|page| page as usize)
            .filter(|page| *page < inner.tabs.len())
    }

    /// Editor of the visible tab.
    pub fn current_editor(&self) -> Option<EditorView> {
        let inner = self.inner.borrow();
        let index = inner.notebook.current_page()? as usize;
        inner.tabs.get(index).map(|tab| tab.editor.clone())
    }

    /// File path of the visible tab, if it has one.
    pub fn current_path(&self) -> Option<PathBuf> {
        let inner = self.inner.borrow();
        let index = inner.notebook.current_page()? as usize;
        inner.tabs.get(index).and_then(|tab| tab.path.clone())
    }

    /// Path of the tab at `index`, if it has one.
    pub fn tab_path(&self, index: usize) -> Option<PathBuf> {
        let inner = self.inner.borrow();
        inner.tabs.get(index).and_then(|tab| tab.path.clone())
    }

    /// Display name of the tab at `index`.
    pub fn tab_name(&self, index: usize) -> String {
        let inner = self.inner.borrow();
        inner
            .tabs
            .get(index)
            .map(|tab| tab.name.clone())
            .unwrap_or_default()
    }

    /// Indices of the tabs that hold unsaved changes, in tab order.
    pub fn unsaved_indices(&self) -> Vec<usize> {
        let inner = self.inner.borrow();
        inner
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, tab)| tab.is_modified())
            .map(|(index, _)| index)
            .collect()
    }

    /// Editor of the tab at `index`.
    pub fn editor_at(&self, index: usize) -> Option<EditorView> {
        let inner = self.inner.borrow();
        inner.tabs.get(index).map(|tab| tab.editor.clone())
    }

    /// The open files of the workspace, for the session file.
    ///
    /// Returns the per-file state (path, caret, scroll) and the index of the
    /// active file inside that list. Tabs without a path (untitled) are not
    /// part of a session because there is nothing to reopen.
    pub fn session_snapshot(&self) -> (Vec<SessionFile>, usize) {
        let inner = self.inner.borrow();
        let current = inner.notebook.current_page().map(|page| page as usize);
        let mut files = Vec::new();
        let mut active = None;
        for (index, tab) in inner.tabs.iter().enumerate() {
            let Some(path) = tab.path.clone() else {
                continue;
            };
            if current == Some(index) {
                active = Some(files.len());
            }
            files.push(SessionFile {
                path,
                caret: tab.editor.caret(),
                scroll: tab.editor.scroll_offset(),
                bookmarks: tab.editor.bookmarks(),
            });
        }
        let active = active.unwrap_or_else(|| files.len().saturating_sub(1));
        (files, active)
    }

    /// Display name of the visible tab.
    pub fn current_name(&self) -> String {
        let inner = self.inner.borrow();
        let index = inner.notebook.current_page().unwrap_or(0) as usize;
        inner
            .tabs
            .get(index)
            .map(|tab| tab.name.clone())
            .unwrap_or_default()
    }

    /// Syntax name (language) of the visible tab.
    pub fn current_language(&self) -> String {
        let inner = self.inner.borrow();
        let index = inner.notebook.current_page().unwrap_or(0) as usize;
        inner
            .tabs
            .get(index)
            .map(|tab| tab.language.clone())
            .unwrap_or_else(|| String::from("Plain Text"))
    }

    /// Open a new empty tab and make it the visible one.
    pub fn new_tab(&self) -> usize {
        self.new_tab_with_text("")
    }

    /// Open a new tab holding `text` and make it the visible one.
    pub fn new_tab_with_text(&self, text: &str) -> usize {
        // Read what we need from the shared state and release the borrow again
        // before touching the notebook: both `append_page` and
        // `set_current_page` emit `switch-page`, which calls back into this
        // manager and would otherwise re-enter the `RefCell`.
        let (theme, untitled) = {
            let mut inner = self.inner.borrow_mut();
            inner.untitled += 1;
            (inner.theme, inner.untitled)
        };
        let name = format!("Untitled {untitled}");
        let language = String::from("Plain Text");
        let editor = EditorView::new();
        editor.set_theme(theme);
        editor.set_language(&language);
        editor.set_hexpand(true);
        editor.set_vexpand(true);
        if !text.is_empty() {
            editor.load_text(text);
        }
        let label = gtk4::Label::new(Some(&name));
        let notebook = self.inner.borrow().notebook.clone();
        let page = notebook.append_page(&editor, Some(&label));
        let index = {
            let mut inner = self.inner.borrow_mut();
            inner.tabs.push(Tab {
                editor,
                label,
                name,
                path: None,
                encoding: DetectedEncoding::Utf8,
                had_bom: false,
                eol: Eol::Lf,
                language,
                saved: text.to_string(),
                last_text: text.to_string(),
                history: Vec::new(),
                redo: Vec::new(),
            });
            inner.tabs.len() - 1
        };
        notebook.set_current_page(Some(page));
        index
    }

    /// Open `path` in a new tab and make it the visible one.
    ///
    /// The file is decoded with `fs::io::read_path`, so the encoding, the BOM
    /// and the original end-of-line style are remembered for the save path.
    pub fn open_path(&self, path: &Path) -> Result<usize, String> {
        // Same rule as `new_tab_with_text`: the notebook calls below emit
        // `switch-page`, which calls back into this manager, so the `RefCell`
        // must not be borrowed while they run.
        let read = read_path(path).map_err(|err| err.to_string())?;
        let language = crate::syntax::ext_to_syntax(path);
        let theme = self.inner.borrow().theme;
        let editor = EditorView::new();
        editor.set_theme(theme);
        editor.set_language(&language);
        editor.set_hexpand(true);
        editor.set_vexpand(true);
        editor.load_text(&read.text);
        let name = file_name_of(path);
        let label = gtk4::Label::new(Some(&name));
        let notebook = self.inner.borrow().notebook.clone();
        let page = notebook.append_page(&editor, Some(&label));
        let index = {
            let mut inner = self.inner.borrow_mut();
            inner.tabs.push(Tab {
                editor,
                label,
                name,
                path: Some(path.to_path_buf()),
                encoding: read.encoding,
                had_bom: read.had_bom,
                eol: read.eol,
                language,
                saved: read.text.clone(),
                last_text: read.text,
                history: Vec::new(),
                redo: Vec::new(),
            });
            inner.tabs.len() - 1
        };
        notebook.set_current_page(Some(page));
        Ok(index)
    }

    /// Close the tab at `index` without asking anything.
    pub fn remove_tab(&self, index: usize) {
        // Same re-entrancy rule as `new_tab_with_text`: `remove_page` emits
        // `switch-page`, which calls back into this manager.
        let notebook = {
            let inner = self.inner.borrow();
            if index >= inner.tabs.len() {
                return;
            }
            inner.notebook.clone()
        };
        notebook.remove_page(Some(index as u32));
        let mut inner = self.inner.borrow_mut();
        if index < inner.tabs.len() {
            inner.tabs.remove(index);
        }
    }

    /// Write the tab at `index` to `path` and remember it as the new target.
    ///
    /// The original encoding, BOM and end-of-line style of the file are kept.
    pub fn save_tab(&self, index: usize, path: &Path) -> Result<(), String> {
        let (text, eol, encoding, had_bom) = {
            let inner = self.inner.borrow();
            let tab = inner
                .tabs
                .get(index)
                .ok_or_else(|| format!("no tab at index {index}"))?;
            (tab.editor.doc_text(), tab.eol, tab.encoding, tab.had_bom)
        };
        write_path(path, &text, eol, encoding, had_bom).map_err(|err| err.to_string())?;
        let mut inner = self.inner.borrow_mut();
        if let Some(tab) = inner.tabs.get_mut(index) {
            tab.path = Some(path.to_path_buf());
            tab.name = file_name_of(path);
            tab.saved = text;
            tab.last_text = tab.saved.clone();
            tab.refresh_label();
        }
        Ok(())
    }

    /// Set the syntax (language) of the tab at `index`.
    pub fn set_language(&self, index: usize, language: &str) {
        let mut inner = self.inner.borrow_mut();
        if let Some(tab) = inner.tabs.get_mut(index) {
            tab.language = language.to_string();
            tab.editor.set_language(language);
        }
    }

    /// Re-colour every tab.
    pub fn set_theme(&self, theme: Theme) {
        let mut inner = self.inner.borrow_mut();
        inner.theme = theme;
        for tab in &inner.tabs {
            tab.editor.set_theme(theme);
        }
    }

    /// Look at the tab at `index`: record new content in the undo history,
    /// refresh its dirty marker and return the status bar text.
    ///
    /// This is the cheap poll the application runs on a timer; it is the only
    /// way to notice edits, because `EditorView` has no "changed" signal.
    pub fn refresh(&self, index: usize) -> Option<String> {
        let mut inner = self.inner.borrow_mut();
        let tab = inner.tabs.get_mut(index)?;
        let current = tab.editor.doc_text();
        if current != tab.last_text {
            let previous = std::mem::replace(&mut tab.last_text, current);
            tab.history.push(previous);
            if tab.history.len() > HISTORY_CAP {
                tab.history.remove(0);
            }
            tab.redo.clear();
        }
        tab.refresh_label();
        Some(status_text_of(tab, &tab.last_text))
    }

    /// Undo the last change of the tab at `index`.
    ///
    /// The editor keeps its own undo stack for the keyboard shortcuts; this
    /// history backs the menu action, which cannot reach into the editor.
    pub fn undo(&self, index: usize) -> bool {
        let mut inner = self.inner.borrow_mut();
        let Some(tab) = inner.tabs.get_mut(index) else {
            return false;
        };
        let Some(previous) = tab.history.pop() else {
            return false;
        };
        let current = tab.editor.doc_text();
        tab.editor.load_text(&previous);
        tab.last_text = previous;
        tab.redo.push(current);
        tab.refresh_label();
        true
    }

    /// Redo the last undone change of the tab at `index`.
    pub fn redo(&self, index: usize) -> bool {
        let mut inner = self.inner.borrow_mut();
        let Some(tab) = inner.tabs.get_mut(index) else {
            return false;
        };
        let Some(next) = tab.redo.pop() else {
            return false;
        };
        let current = tab.editor.doc_text();
        tab.editor.load_text(&next);
        tab.last_text = next;
        tab.history.push(current);
        tab.refresh_label();
        true
    }

    /// Status bar text of the visible tab.
    pub fn status_text(&self) -> String {
        let inner = self.inner.borrow();
        let index = inner.notebook.current_page().unwrap_or(0) as usize;
        let Some(tab) = inner.tabs.get(index) else {
            return String::new();
        };
        let text = tab.editor.doc_text();
        status_text_of(tab, &text)
    }

    /// Ask the user what to do with the unsaved changes of the tab at `index`.
    ///
    /// An unmodified tab is closed right away. A modified tab opens a
    /// `gtk4::AlertDialog` with *Cancel* / *Save* / *Discard*; the answer is
    /// handed to `on_decision`.
    pub fn request_close(
        &self,
        index: usize,
        parent: &gtk4::ApplicationWindow,
        on_decision: impl FnOnce(CloseDecision) + 'static,
    ) {
        let modified = {
            let inner = self.inner.borrow();
            inner
                .tabs
                .get(index)
                .map(|tab| tab.is_modified())
                .unwrap_or(false)
        };
        if !modified {
            self.remove_tab(index);
            on_decision(CloseDecision::Close);
            return;
        }

        let name = {
            let inner = self.inner.borrow();
            inner
                .tabs
                .get(index)
                .map(|tab| tab.name.clone())
                .unwrap_or_else(|| String::from("this document"))
        };
        let dialog = Rc::new(
            gtk4::AlertDialog::builder()
                .message(format!("Save changes to {name}?"))
                .detail("Your changes will be lost if you do not save them.")
                .buttons(gtk4::glib::StrV::from(["Cancel", "Save", "Discard"]))
                .cancel_button(0)
                .default_button(1)
                .build(),
        );
        let tabs = self.clone();
        let keep_alive = dialog.clone();
        dialog.choose(Some(parent), None::<&gio::Cancellable>, move |result| {
            let _keep_alive = keep_alive;
            match result {
                // Button 1 is "Save".
                Ok(1) => on_decision(CloseDecision::Save),
                // Button 2 is "Discard".
                Ok(2) => {
                    tabs.remove_tab(index);
                    on_decision(CloseDecision::Close);
                }
                _ => on_decision(CloseDecision::Cancel),
            }
        });
    }
}

/// File name of `path`, or the whole path when it has none.
fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

/// Short name of an end-of-line style.
fn eol_label(eol: Eol) -> &'static str {
    match eol {
        Eol::Lf => "LF",
        Eol::Crlf => "CRLF",
        Eol::Cr => "CR",
    }
}

/// Line (1-based) and column (1-based) of the byte offset `byte` in `text`.
fn line_column(text: &str, byte: usize) -> (usize, usize) {
    let Some(head) = text.get(..byte.min(text.len())) else {
        return (1, 1);
    };
    let line = head.matches('\n').count();
    let line_start = head.rfind('\n').map(|index| index + 1).unwrap_or(0);
    let column = head[line_start..].chars().count();
    (line + 1, column + 1)
}

/// `Ln X, Col Y | encoding | EOL | language` for one tab.
fn status_text_of(tab: &Tab, text: &str) -> String {
    let selection = tab.editor.selection();
    let (line, column) = line_column(text, selection.head);
    format!(
        "Ln {}, Col {}  |  {}  |  {}  |  {}",
        line,
        column,
        tab.encoding.label(),
        eol_label(tab.eol),
        tab.language
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_column_counts_lines_and_chars() {
        assert_eq!(line_column("abc", 0), (1, 1));
        assert_eq!(line_column("abc", 3), (1, 4));
        assert_eq!(line_column("ab\ncd", 3), (2, 1));
        assert_eq!(line_column("ab\ncd", 5), (2, 3));
        assert_eq!(line_column("ab\ncd", 99), (2, 3));
    }

    #[test]
    fn line_column_handles_multibyte() {
        // `head` is a byte offset: "你好" is six bytes, so byte 6 sits on the
        // newline itself (end of line 1) and line 2 starts at byte 7.
        assert_eq!(line_column("你好\nworld", 6), (1, 3));
        assert_eq!(line_column("你好\nworld", 7), (2, 1));
        assert_eq!(line_column("你好\nworld", 9), (2, 3));
    }

    #[test]
    fn eol_labels_are_stable() {
        assert_eq!(eol_label(Eol::Lf), "LF");
        assert_eq!(eol_label(Eol::Crlf), "CRLF");
        assert_eq!(eol_label(Eol::Cr), "CR");
    }

    #[test]
    fn file_name_of_falls_back_to_the_path() {
        assert_eq!(file_name_of(Path::new("/tmp/a.rs")), "a.rs");
        assert_eq!(file_name_of(Path::new("/")), "/");
    }
}
