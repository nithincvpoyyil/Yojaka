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
/// A text box: lighter than the background (2.1:1), so it reads as a field; white text on it
/// stays at 7.3:1.
const INPUT: (u8, u8, u8) = (0x2a, 0x5a, 0x80);

fn truecolor() -> bool {
    static TRUE: OnceLock<bool> = OnceLock::new();
    *TRUE.get_or_init(|| {
        let ct = std::env::var("COLORTERM").unwrap_or_default();
        matches!(ct.as_str(), "truecolor" | "24bit")
            || std::env::var("TERM_PROGRAM").is_ok_and(|p| p == "iTerm.app" || p == "WarpTerminal")
    })
}

fn c(rgb: (u8, u8, u8)) -> Color {
    if truecolor() {
        return Color::Rgb(rgb.0, rgb.1, rgb.2);
    }
    Color::Indexed(xterm_index(rgb))
}

/// The six steps of each channel in xterm's 6×6×6 colour cube (indices 16..=231).
const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// The nearest of the 256 fixed colours. Each channel goes to its nearest cube step (the steps
/// are not evenly spaced). A colour that is nearly grey may use the 24-step grey ramp
/// (232..=255) instead, when that is closer; a coloured one keeps its hue.
pub(crate) fn xterm_index((r, g, b): (u8, u8, u8)) -> u8 {
    let step = |v: u8| {
        (0..CUBE.len())
            .min_by_key(|&i| CUBE[i].abs_diff(v))
            .unwrap_or(0) as u8
    };
    let (ri, gi, bi) = (step(r), step(g), step(b));
    let cube = 16 + 36 * ri + 6 * gi + bi;
    let dist = |x: (u8, u8, u8)| {
        [(x.0, r), (x.1, g), (x.2, b)]
            .iter()
            .map(|&(a, b)| u32::from(a.abs_diff(b)).pow(2))
            .sum::<u32>()
    };
    let cube_rgb = (CUBE[ri as usize], CUBE[gi as usize], CUBE[bi as usize]);
    let (lo, hi) = (r.min(g).min(b), r.max(g).max(b));
    if hi - lo > 24 {
        return cube;
    }
    let avg = (u16::from(r) + u16::from(g) + u16::from(b)) / 3;
    let gi = ((avg.saturating_sub(8) + 5) / 10).min(23) as u8;
    let level = 8 + 10 * gi;
    if dist((level, level, level)) < dist(cube_rgb) {
        232 + gi
    } else {
        cube
    }
}

/// Background and default text of the whole screen.
pub fn base() -> Style {
    Style::new().fg(c(FOREGROUND)).bg(c(BACKGROUND))
}

/// A bordered panel. Focused: yellow border and yellow title; otherwise a blue border and a
/// white title, so focus shows in two ways, not by colour alone.
pub fn panel<'a>(title: impl Into<ratatui::text::Line<'a>>, focused: bool) -> Block<'a> {
    let title: ratatui::text::Line = title.into();
    Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(border(focused))
        .title(title.patch_style(if focused { self::title() } else { header() }))
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
/// The one main next step in the hint line, drawn as a chip: ` a  review 77 files `.
pub fn key_chip() -> Style {
    Style::new()
        .fg(c(BLACK))
        .bg(c(YELLOW))
        .add_modifier(Modifier::BOLD)
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
/// A text box you can type in (Organize: pattern, destination).
pub fn input() -> Style {
    Style::new().fg(c(FOREGROUND)).bg(c(INPUT))
}
/// The text box while you type in it.
pub fn input_editing() -> Style {
    Style::new()
        .fg(c(FOREGROUND))
        .bg(c(SELECTION))
        .add_modifier(Modifier::BOLD)
}
/// The text cursor inside a box.
pub fn cursor() -> Style {
    Style::new().fg(c(YELLOW)).bg(c(SELECTION))
}
/// A value picked from a list (`‹ Copy ›`).
pub fn choice() -> Style {
    Style::new().fg(c(CYAN)).add_modifier(Modifier::BOLD)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG contrast ratio of two colours.
    fn contrast(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
        let lum = |(r, g, b): (u8, u8, u8)| {
            let f = |v: u8| {
                let x = f64::from(v) / 255.0;
                if x <= 0.03928 {
                    x / 12.92
                } else {
                    ((x + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
        };
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    /// The RGB value of a 256-colour index (cube and grey ramp only).
    fn rgb(i: u8) -> (u8, u8, u8) {
        if i >= 232 {
            let v = 8 + 10 * (i - 232);
            return (v, v, v);
        }
        let i = i - 16;
        (
            CUBE[usize::from(i / 36)],
            CUBE[usize::from(i / 6 % 6)],
            CUBE[usize::from(i % 6)],
        )
    }

    #[test]
    fn fixed_colours_keep_the_hue_and_exact_values() {
        assert_eq!(rgb(xterm_index(BACKGROUND)), (0, 0, 95), "navy, not teal");
        assert_eq!(rgb(xterm_index(FOREGROUND)), (255, 255, 255));
        assert_eq!(rgb(xterm_index(YELLOW)), (255, 215, 0));
        assert_eq!(
            rgb(xterm_index((0x30, 0x30, 0x30))),
            (48, 48, 48),
            "greys use the ramp"
        );
        assert_eq!(rgb(xterm_index((0, 0, 0))), (0, 0, 0));
    }

    #[test]
    fn text_stays_readable_with_256_colours() {
        let bg = rgb(xterm_index(BACKGROUND));
        for (name, fg) in [
            ("foreground", FOREGROUND),
            ("muted", MUTED),
            ("red", RED),
            ("green", GREEN),
            ("yellow", YELLOW),
            ("cyan", CYAN),
        ] {
            let ratio = contrast(rgb(xterm_index(fg)), bg);
            assert!(ratio >= 4.5, "{name}: {ratio:.2}");
        }
        let sel = rgb(xterm_index(SELECTION));
        assert!(contrast(rgb(xterm_index(FOREGROUND)), sel) >= 4.5);
    }

    #[test]
    fn text_boxes_stand_out_and_stay_readable() {
        assert!(contrast(INPUT, BACKGROUND) >= 2.0);
        assert!(contrast(FOREGROUND, INPUT) >= 4.5);
        assert!(contrast(MUTED, BACKGROUND) >= 4.5);
    }
}
