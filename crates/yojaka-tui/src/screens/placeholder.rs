use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::App;

pub fn render(app: &App, f: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(app.screen.title());
    f.render_widget(
        Paragraph::new(format!("Arrives in Iteration {}", app.screen.arrives_in()))
            .wrap(Wrap { trim: true })
            .block(block),
        area,
    );
}
