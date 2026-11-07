use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::keymap::HELP;
use crate::theme;

pub fn render(f: &mut Frame, area: Rect) {
    let lines: Vec<Line> = HELP
        .iter()
        .map(|(k, d)| {
            if k.is_empty() {
                Line::styled(*d, theme::title())
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
    f.render_widget(Clear, rect);
    let block = Block::default().borders(Borders::ALL).title("Help");
    f.render_widget(Paragraph::new(lines).block(block), rect);
}
