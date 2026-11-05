use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, Screen};

pub fn render(app: &App, f: &mut Frame, area: Rect) {
    let text = match app.screen {
        Screen::Dashboard => "No scan yet — press s to scan (Iteration 1)".to_string(),
        other => format!("Arrives in Iteration {}", other.arrives_in()),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(app.screen.title());
    f.render_widget(Paragraph::new(text).block(block), area);
}
