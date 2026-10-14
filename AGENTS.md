# npad — agent working agreement

New project: a Notepad++-like native Linux editor. Reference repo
`/home/ycl/github/notepad-plus-plus` is READ-ONLY (behavior reference only).
Never modify it from this project.

## Toolchain facts (verified — do not re-verify, do not fight them)
- Deps: gtk4 0.11.5 with `features=["v4_10"]`, ropey `=1.6.1` (NOT 2.0.0-beta.1),
  syntect 5.3.0 `default-features=false, features=["default-fancy","yaml-load"]`,
  regex, serde, encoding_rs, anyhow.
- The GTK4 Rust crate is named `gtk4` (not `gtk`).
- `Snapshot` lives at `gtk4::Snapshot`; methods: `append_layout(&pango::Layout,&gdk::RGBA)`,
  `append_color(&gdk::RGBA,&graphene::Rect)`, `translate(&graphene::Point)`, `save`, `restore`.
- `WidgetImpl::measure` returns a 4-tuple `(min, natural, min_baseline, natural_baseline)`.
- pango: no `font_metrics`; use `Layout::pixel_size()` / `pixel_extents()`.
- `glib::wrapper!` order: `@extends ... ,` FIRST, then `@implements ...;`.
- IME: `EventControllerKey::set_im_context(Some(&im))`; do NOT hand-write `filter_keypress`.
- GApplication eats argv as files — pass parameters via env vars (e.g. `NPAD_SHOT`).
- `Widget::width()/height()` are ambiguous through the glob prelude; use
  `allocated_width()/allocated_height()`.
- `app.run()` returns `glib::ExitCode`; convert with `i32::from(code)`.

## Conventions
- Rust 2024 edition, `cargo fmt` + `cargo clippy -- -D warnings` clean before commit.
- No `unwrap()`/`expect()` in non-test code paths; use `anyhow::Result` + context.
- Editor coordinates are BYTE offsets into the rope. Column math uses char counts.
- Scroll is self-managed inside `EditorView` (see plan §3.1). Do NOT hand content
  sizing to `GtkScrolledWindow`.
- Every UI change must be verified with `./tools/screenshot.sh /tmp/x.png` and a
  pixel check, not just a build.

## Verification gates (a change is not done until all pass)
1. `cargo build` clean (no new warnings)
2. `cargo test` clean
3. `cargo clippy --all-targets -- -D warnings` clean
4. If UI touched: `tools/screenshot.sh` PNG produced + manually inspected
