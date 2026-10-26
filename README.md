# npad

**npad** — a Notepad++-like native Linux text editor written in Rust with GTK4.

It is a from-scratch editor: the text view is a hand-written GTK4 widget (no
Scintilla/Lexilla), the buffer is a [ropey](https://crates.io/crates/ropey) rope,
and syntax highlighting comes from [syntect](https://crates.io/crates/syntect).

---

## Features

Status legend:

- **wired** — implemented and connected to the application window.
- **engine** — implemented as a self-contained module with unit tests, but not
  yet connected to the window (the calling UI is a later work unit).
- **planned** — not implemented yet.

| Feature | Status | Notes |
|---|---|---|
| Native Linux GTK4 | wired | `gtk4` 0.11 (Rust edition 2024), `HeaderBar` window, menu button, status bar. The editor is a custom `gtk4::Widget` subclass. |
| Self-drawn editor core | wired | Line-number gutter, current-line highlight, blinking caret, self-managed viewport and scrolling, mouse-wheel scrolling, caret auto-scrolled into view. |
| Keyboard input and IME | wired | `EventControllerKey` with a real `gtk4::IMContext`; typed characters, `Return`, `BackSpace`, `Delete`, `Tab`, arrows, `Home`/`End`, `Ctrl+Home`/`Ctrl+End`. |
| Undo / redo | wired | Structured edit stack (`Edit { start, old, new, kind }`) on top of the rope, with an 800 ms merge window for consecutive typing. Bound to `Ctrl+Z` / `Ctrl+Y`. |
| Light / dark theme | wired | `Theme` / `ThemeMode` palettes drive every editor colour (background, gutter, caret, selection, current line) and `EditorView::set_theme` repaints immediately. **View → Toggle Theme** (`Ctrl+D`) switches the palette at runtime and saves the choice to the preferences file. |
| Syntax highlighting (syntect) | engine | `src/syntax/mod.rs` maps 30+ extensions (plus `Makefile` / `Dockerfile` by name) onto syntect syntaxes, with plain-text fallback; `src/ui/editor/highlight.rs` holds a per-line LRU cache that is invalidated by the buffer epoch. The rendering path currently paints text in a single foreground colour, so the spans are computed but not yet drawn. |
| Regular-expression find & replace | engine | `src/ui/search.rs`: literal or regex matching, case sensitivity, wrap-around, find-next, and replace-all with `$1` backreferences. Every replacement is recorded in the undo stack. The find/replace panel itself is not in the UI yet. |
| Encoding & line endings | wired | `src/fs/io.rs`: BOM detection for UTF-8 / UTF-16 LE / UTF-16 BE, strict UTF-8 validation, GBK and Latin-1 fallback, bytes are never silently dropped. Text is `\n`-normalised in memory and the original EOL (`LF` / `CRLF` / `CR`) and BOM are restored on write. **Open...** and **Save As...** use GTK file dialogs, but there is no encoding or EOL chooser yet. |
| Session restore | wired | `src/ui/session.rs` defines open files, per-tab caret/scroll position, active tab, recent files and window size; `src/ui/prefs.rs` persists theme, font family/size, tab width, wrapping and the "restore session" flag. The session is written when the window closes and read back at startup: every open file is reopened, the caret and the scroll offset of each tab are reapplied, the active tab is selected again, and the window keeps its size. Closing a window with unsaved changes asks **Cancel** / **Save** / **Discard** per tab first. |
| Multiple tabs | wired | `src/ui/notebook.rs` owns a GTK notebook with one buffer and independent path, encoding, EOL, language, undo history and dirty state per tab. Closing a modified tab prompts **Cancel** / **Save** / **Discard**. |

---

## Build and run

```bash
cargo build --release
./target/release/npad
```

Requires a GTK4 development environment (GTK4 ≥ 4.12, pango, cairo, gdk-pixbuf)
and a Rust toolchain with edition 2024 support.

## Development

```bash
cargo build                                   # debug build
cargo test                                    # unit tests
cargo clippy --all-targets -- -D warnings    # lint gate
tools/verify.sh                               # full gate: release build + test + clippy + screenshot
tools/screenshot.sh /tmp/x.png                # render the window to a PNG (UI changes)
```

`tools/verify.sh` is the one-shot gate. It runs the release build, the test
suite, the clippy gate and the screenshot tool, checks that the produced PNG
exists, is larger than 1 KB and carries the PNG magic bytes, and prints
`VERIFY OK` — or `VERIFY FAILED: <step>` and a non-zero exit status.

Two environment variables drive the app without a user: `NPAD_SHOT=/tmp/x.png`
renders the window into a PNG, and `NPAD_AUTOCLOSE=1` runs the close sequence
right after startup — the unsaved-changes prompt, the session save and the
window destroy — so the save-on-close path can be smoke-tested.

## Architecture

| Module | Responsibility |
|---|---|
| `src/main.rs` | Entry point: `gtk4::Application` bootstrap, preference loading, initial demo buffers, and the optional `NPAD_SHOT` window capture and `NPAD_AUTOCLOSE` close-sequence hooks. |
| `src/app.rs` | Window, header bar, menu, status bar, CSS provider, session restore and save-on-close, offscreen PNG capture. |
| `src/ui/notebook.rs` | Tab manager: one buffer and independent metadata per tab, notebook wiring, per-tab history tracking, modified-tab close confirmation, and the per-tab snapshot the session file is built from. |
| `src/ui/editor/*` | Self-drawn editor kernel: rope buffer with undo/redo (`buffer.rs`), caret and selection (`cursor.rs`), line↔screen-row mapping and gutter metrics (`rowmap.rs`), self-managed viewport (`scroll.rs`), syntect highlight cache (`highlight.rs`) and the GTK4 widget itself (`view.rs`). |
| `src/ui/search.rs` | Find/replace engine: literal and regular-expression matching, case sensitivity, wrap-around, replace-all with backreferences. |
| `src/ui/session.rs` + `src/ui/prefs.rs` | Session and preference persistence (JSON under `$XDG_CONFIG_HOME/npad`). |
| `src/ui/theme.rs` | Light/dark colour palettes and `ThemeMode`. |
| `src/fs/io.rs` | Encoding detection and conversion, BOM handling, line-ending normalisation and restore. |
| `src/syntax/mod.rs` | Extension → syntect syntax mapping and the language list used by the language menu. |

Coordinates inside the editor are **byte offsets into the rope**; column numbers
are counted in characters, not bytes.

## Linux desktop integration

The Freedesktop integration files live in `data/`:

```
data/dev.npad.Editor.desktop        desktop entry
data/dev.npad.Editor.metainfo.xml    AppStream metadata
data/dev.npad.Editor.svg             application icon
```

Install them for the current user:

```bash
install -Dm644 data/dev.npad.Editor.desktop \
    "$HOME/.local/share/applications/dev.npad.Editor.desktop"
install -Dm644 data/dev.npad.Editor.svg \
    "$HOME/.local/share/icons/hicolor/scalable/apps/dev.npad.Editor.svg"
install -Dm644 data/dev.npad.Editor.metainfo.xml \
    "$HOME/.local/share/metainfo/dev.npad.Editor.metainfo.xml"

update-desktop-database "$HOME/.local/share/applications"  # optional
```

The desktop entry runs `npad`, so the binary has to be on `PATH` — either install
`target/release/npad` into `~/.local/bin`, or edit the `Exec=` line to point at
the absolute path of your build. `desktop-file-validate data/dev.npad.Editor.desktop`
and `appstreamcli validate data/dev.npad.Editor.metainfo.xml` check both files
if those tools are available.

## Known limitations

- No plugin system.
- No macro recording or playback.
- No hex editor.
- No code completion / IntelliSense.
- No code folding.
- No find/replace panel; the **Find**, **Go to Line**, and **Select All** menu
  actions are placeholders.
- No encoding or line-ending chooser; detection and restore are automatic.
- The recent-files list is persisted but has no menu entry yet; it is part of
  the session file only.
- No `.deb` / Flatpak packaging yet — only the release binary and the desktop
  integration files are provided.
