//! Colors and styles. The default theme is Cobalt2; every color is named here and nowhere else,
//! so another theme only has to change this file.
//!
//! Terminals without 24-bit color (macOS Terminal.app) get the nearest of the 256 fixed colors.

use std::sync::OnceLock;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Clear};

// Cobalt2 (terminalcolors.com/themes/cobalt2/default).
const BACKGROUND: (u8, u8, u8) = (0x12, 0x27, 0x38);
const FOREGROUND: (u8, u8, u8) = (0xff, 0xff, 0xff);
const SELECTION: (u8, u8, u8) = (0x00, 0x50, 0xa4);
const BLACK: (u8, u8, u8) = (0x00, 0x00, 0x00);
const RED: (u8, u8, u8) = (0xff, 0x62, 0x8c);
const GREEN: (u8, u8, u8) = (0x3a, 0xd9, 0x00);
const YELLOW: (u8, u8, u8) = (0xff, 0xc6, 0x00);
const BLUE: (u8, u8, u8) = (0x00, 0x88, 0xff);
const MAGENTA: (u8, u8, u8) = (0xfb, 0x94, 0xff);
const CYAN: (u8, u8, u8) = (0x80, 0xfc, 0xff);
/// Secondary text: the background lightened towards the selection blue (not in the palette).
const MUTED: (u8, u8, u8) = (0x7f, 0x9d, 0xb8);

fn truecolor() -> bool {
    static TRUE: OnceLock<bool> = OnceLock::new();
    *TRUE.get_or_init(|| {
        let ct = std::env::var("COLORTERM").unwrap_or_default();
        matches!(ct.as_str(), "truecolor" | "24bit")
            || std::env::var("TERM_PROGRAM").is_ok_and(|p| p == "iTerm.app" || p == "WarpTerminal")
    })
}

fn c((r, g, b): (u8, u8, u8)) -> Color {
    if truecolor() {
        return Color::Rgb(r, g, b);
    }
    // Nearest entry of the 6×6×6 color cube (indices 16..=231).
    let level = |v: u8| ((u16::from(v) * 5 + 127) / 255) as u8;
    Color::Indexed(16 + 36 * level(r) + 6 * level(g) + level(b))
}

/// Background and default text of the whole screen.
pub fn base() -> Style {
    Style::new().fg(c(FOREGROUND)).bg(c(BACKGROUND))
}

/// A bordered panel with a yellow title; the border is yellow when `focused`.
pub fn panel<'a>(title: impl Into<ratatui::text::Line<'a>>, focused: bool) -> Block<'a> {
    let title: ratatui::text::Line = title.into();
    Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(border(focused))
        .title(title.patch_style(self::title()))
}

/// Empties `area` (for popups) and repaints it with the base colors.
pub fn clear(f: &mut Frame, area: Rect) {
    f.render_widget(Clear, area);
    f.render_widget(Block::default().style(base()), area);
}

pub fn badge_read_only() -> Style {
    Style::new()
        .fg(c(BLACK))
        .bg(c(GREEN))
        .add_modifier(Modifier::BOLD)
}
pub fn badge_write() -> Style {
    Style::new()
        .fg(c(BLACK))
        .bg(c(YELLOW))
        .add_modifier(Modifier::BOLD)
}
pub fn header() -> Style {
    Style::new().fg(c(FOREGROUND)).add_modifier(Modifier::BOLD)
}
pub fn footer() -> Style {
    Style::new().fg(c(MUTED))
}
/// A key in the hint line: `q`, `Tab`, `R`.
pub fn key_hint() -> Style {
    Style::new().fg(c(YELLOW)).add_modifier(Modifier::BOLD)
}
pub fn dim() -> Style {
    Style::new().fg(c(MUTED))
}
pub fn accent() -> Style {
    Style::new().fg(c(CYAN))
}
pub fn bar() -> Style {
    Style::new().fg(c(BLUE))
}
pub fn title() -> Style {
    Style::new().fg(c(YELLOW)).add_modifier(Modifier::BOLD)
}
pub fn error() -> Style {
    Style::new().fg(c(RED))
}
pub fn good() -> Style {
    Style::new().fg(c(GREEN))
}
pub fn live_badge() -> Style {
    Style::new()
        .fg(c(BLACK))
        .bg(c(MAGENTA))
        .add_modifier(Modifier::BOLD)
}
/// The row/node under the cursor in the focused pane.
pub fn selected() -> Style {
    Style::new()
        .fg(c(FOREGROUND))
        .bg(c(SELECTION))
        .add_modifier(Modifier::BOLD)
}
/// The row/node under the cursor in an unfocused pane.
pub fn selected_dim() -> Style {
    Style::new().fg(c(CYAN))
}
pub fn border(focused: bool) -> Style {
    if focused {
        Style::new().fg(c(YELLOW))
    } else {
        Style::new().fg(c(BLUE))
    }
}
/// Outline of the current tab.
pub fn tab_border_selected() -> Style {
    Style::new().fg(c(YELLOW))
}
/// Outline of the other tabs, and the line the tabs stand on.
pub fn tab_border() -> Style {
    Style::new().fg(c(BLUE))
}
pub fn tab_selected() -> Style {
    Style::new().fg(c(YELLOW)).add_modifier(Modifier::BOLD)
}
pub fn tab() -> Style {
    Style::new().fg(c(MUTED))
}
/// The current tab while the tab strip has the keyboard.
pub fn tab_active() -> Style {
    Style::new()
        .fg(c(FOREGROUND))
        .bg(c(SELECTION))
        .add_modifier(Modifier::BOLD)
}
