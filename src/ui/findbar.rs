//! The find and replace bar of the window.
//!
//! The bar owns the widgets; the application wires the buttons to the engine
//! in `ui::search` and to the current editor.
//!
//! OWNED BY: U11 (menu + actions).

use gtk4::prelude::*;

use crate::ui::search::SearchOptions;

/// The state of the bar: the two entries and the three checkboxes.
///
/// Cloning it only bumps the reference count of the widgets, so a button
/// handler can hold on to it without keeping the whole bar alive.
#[derive(Clone)]
pub struct FindState {
    /// Entry with the search pattern.
    pub pattern: gtk4::Entry,
    /// Entry with the replacement text.
    pub replacement: gtk4::Entry,
    /// Whether the pattern is a regular expression.
    pub regex: gtk4::CheckButton,
    /// Whether the search distinguishes case.
    pub case_sensitive: gtk4::CheckButton,
    /// Whether the search continues at the other end of the document.
    pub wrap: gtk4::CheckButton,
}

impl FindState {
    /// The search options the checkboxes describe.
    pub fn options(&self) -> SearchOptions {
        SearchOptions {
            case_sensitive: self.case_sensitive.is_active(),
            regex: self.regex.is_active(),
            wrap: self.wrap.is_active(),
        }
    }

    /// The text the search looks for.
    pub fn pattern_text(&self) -> String {
        self.pattern.text().to_string()
    }

    /// The text that replaces a match.
    pub fn replacement_text(&self) -> String {
        self.replacement.text().to_string()
    }
}

/// The find and replace bar.
pub struct FindBar {
    /// The bar itself. It starts hidden and is revealed by a search.
    pub widget: gtk4::Box,
    /// The entries and the checkboxes.
    pub state: FindState,
    /// Finds the next match.
    pub find_next: gtk4::Button,
    /// Finds the previous match.
    pub find_previous: gtk4::Button,
    /// Replaces the current match.
    pub replace: gtk4::Button,
    /// Replaces every match.
    pub replace_all: gtk4::Button,
    /// Hides the bar.
    pub close: gtk4::Button,
    /// Shows the outcome of the last search.
    pub message: gtk4::Label,
}

impl FindBar {
    /// Build the bar with the search options an editor starts with.
    pub fn new() -> Self {
        let pattern = gtk4::Entry::new();
        pattern.set_placeholder_text(Some("Search for"));
        pattern.set_hexpand(true);

        let replacement = gtk4::Entry::new();
        replacement.set_placeholder_text(Some("Replace with"));
        replacement.set_hexpand(true);

        let regex = gtk4::CheckButton::with_label("Regex");
        let case_sensitive = gtk4::CheckButton::with_label("Match case");
        let wrap = gtk4::CheckButton::with_label("Wrap around");
        wrap.set_active(true);

        let find_next = gtk4::Button::with_label("Find Next");
        let find_previous = gtk4::Button::with_label("Find Previous");
        let replace = gtk4::Button::with_label("Replace");
        let replace_all = gtk4::Button::with_label("Replace All");
        let close = gtk4::Button::with_label("Close");

        let find_label = gtk4::Label::new(Some("Find:"));
        let replace_label = gtk4::Label::new(Some("Replace:"));

        let top = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        top.append(&find_label);
        top.append(&pattern);
        top.append(&regex);
        top.append(&case_sensitive);
        top.append(&wrap);
        top.append(&find_next);
        top.append(&find_previous);
        top.append(&close);

        let bottom = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        bottom.append(&replace_label);
        bottom.append(&replacement);
        bottom.append(&replace);
        bottom.append(&replace_all);

        let message = gtk4::Label::new(None);
        message.set_xalign(0.0);
        message.set_hexpand(true);
        message.set_margin_start(12);
        bottom.append(&message);

        let widget = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        widget.set_margin_top(4);
        widget.set_margin_bottom(4);
        widget.set_margin_start(8);
        widget.set_margin_end(8);
        widget.append(&top);
        widget.append(&bottom);
        widget.set_visible(false);

        FindBar {
            widget,
            state: FindState {
                pattern,
                replacement,
                regex,
                case_sensitive,
                wrap,
            },
            find_next,
            find_previous,
            replace,
            replace_all,
            close,
            message,
        }
    }

    /// A snapshot of the entries and the checkboxes.
    pub fn state(&self) -> FindState {
        self.state.clone()
    }

    /// Show the bar and put the cursor into the pattern entry.
    pub fn reveal(&self) {
        self.widget.set_visible(true);
        self.state.pattern.grab_focus();
    }

    /// Hide the bar.
    pub fn conceal(&self) {
        self.widget.set_visible(false);
    }

    /// Show the outcome of the last search.
    pub fn set_message(&self, text: &str) {
        self.message.set_text(text);
    }

    /// Put `text` into the pattern entry.
    pub fn prefill(&self, text: &str) {
        self.state.pattern.set_text(text);
    }
}

impl Default for FindBar {
    fn default() -> Self {
        Self::new()
    }
}
