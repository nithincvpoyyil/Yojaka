use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::app::Screen;
use crate::keymap::HELP;
use crate::theme;

/// The help lines that apply on `screen`: the global section plus the screen's own.
pub fn lines_for(screen: Screen) -> Vec<(&'static str, &'static str)> {
    let mut show = true;
    HELP.iter()
        .filter(|(k, d)| {
            if k.is_empty() {
                show = if d.starts_with("File browser") {
                    screen == Screen::Browser
                } else if d.starts_with("Duplicates") {
                    screen == Screen::Duplicates
                } else if d.starts_with("Organize") {
                    screen == Screen::Organize
                } else if d.starts_with("History") {
                    screen == Screen::History
                } else {
                    true
                };
            }
            show
        })
        .copied()
        .collect()
}

pub fn render(f: &mut Frame, area: Rect, screen: Screen) {
    let lines: Vec<Line> = lines_for(screen)
        .into_iter()
        .map(|(k, d)| {
            if k.is_empty() {
                Line::styled(d, theme::title())
            } else {
                Line::from(format!("  {k:<16}{d}"))
            }
        })
        .collect();
    let height = (lines.len() as u16 + 2).min(area.height);
    let width = 76.min(area.width);
    let rect = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    theme::clear(f, rect);
    let block = theme::panel("Help", true);
    f.render_widget(Paragraph::new(lines).block(block), rect);
}
