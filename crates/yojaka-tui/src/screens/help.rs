use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::keymap::HELP;

pub fn render(f: &mut Frame, area: Rect) {
    let lines: Vec<String> = HELP.iter().map(|(k, d)| format!("{k:<16}{d}")).collect();
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
    f.render_widget(Paragraph::new(lines.join("\n")).block(block), rect);
}
