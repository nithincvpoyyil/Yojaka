use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, Screen};

pub fn render(app: &App, f: &mut Frame, area: Rect) {
    let text = match app.screen {
        Screen::Dashboard => "No scan view yet. Run `yojaka scan <path>` and `yojaka stats <path>` (TUI scanning arrives in Iteration 3)".to_string(),
        other => format!("Arrives in Iteration {}", other.arrives_in()),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(app.screen.title());
    f.render_widget(
        Paragraph::new(text)
            .wrap(ratatui::widgets::Wrap { trim: true })
            .block(block),
        area,
    );
}
