//! Theme colours (plan §3.8): dark/light, applied live via `queue_draw`.
//!
//! OWNED BY: U1 (contract) → U5/U12 (colour tuning, style-scheme parity).

use crate::ui::editor::highlight::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub bg: gtk4::gdk::RGBA,
    pub fg: gtk4::gdk::RGBA,
    pub gutter: gtk4::gdk::RGBA,
    pub gutter_fg: gtk4::gdk::RGBA,
    pub current_line: gtk4::gdk::RGBA,
    pub caret: gtk4::gdk::RGBA,
    pub selection: gtk4::gdk::RGBA,
    pub bookmark: gtk4::gdk::RGBA,
    pub match_bg: gtk4::gdk::RGBA,
}

impl Theme {
    fn rgba(c: Color) -> gtk4::gdk::RGBA {
        let mut out = gtk4::gdk::RGBA::new(0.0, 0.0, 0.0, 1.0);
        out.set_red(c.r as f32 / 255.0);
        out.set_green(c.g as f32 / 255.0);
        out.set_blue(c.b as f32 / 255.0);
        out
    }

    pub fn dark() -> Self {
        Theme {
            bg: Self::rgba(Color::new(0x1C, 0x1F, 0x24)),
            fg: Self::rgba(Color::new(0xD8, 0xDB, 0xE0)),
            gutter: Self::rgba(Color::new(0x29, 0x2B, 0x33)),
            gutter_fg: Self::rgba(Color::new(0x73, 0x78, 0x85)),
            current_line: Self::rgba(Color::new(0x26, 0x29, 0x30)),
            caret: Self::rgba(Color::new(0xD8, 0xDB, 0xE0)),
            selection: Self::rgba(Color::new(0x3A, 0x4A, 0x6B)),
            bookmark: Self::rgba(Color::new(0xE0, 0xA0, 0x40)),
            match_bg: Self::rgba(Color::new(0x4A, 0x5A, 0x7A)),
        }
    }

    pub fn light() -> Self {
        Theme {
            bg: Self::rgba(Color::new(0xFF, 0xFF, 0xFF)),
            fg: Self::rgba(Color::new(0x1F, 0x1F, 0x1F)),
            gutter: Self::rgba(Color::new(0xF2, 0xF2, 0xF2)),
            gutter_fg: Self::rgba(Color::new(0x8A, 0x8F, 0x98)),
            current_line: Self::rgba(Color::new(0xF6, 0xF8, 0xFA)),
            caret: Self::rgba(Color::new(0x1F, 0x1F, 0x1F)),
            selection: Self::rgba(Color::new(0xCD, 0xDD, 0xEE)),
            bookmark: Self::rgba(Color::new(0xB8, 0x6A, 0x00)),
            match_bg: Self::rgba(Color::new(0xE8, 0xE8, 0xE8)),
        }
    }

    pub fn mode(&self) -> ThemeMode {
        let bg = self.bg;
        if bg.red() < 0.5 {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        }
    }

    pub fn from_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Theme::dark(),
            ThemeMode::Light => Theme::light(),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::dark()
    }
}
