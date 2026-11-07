use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Paragraph, Wrap};

use crate::app::App;
use crate::theme;

pub fn render(app: &App, f: &mut Frame, area: Rect) {
    let block = theme::panel(app.screen.title(), false);
    f.render_widget(
        Paragraph::new(format!("Arrives in Iteration {}", app.screen.arrives_in()))
            .wrap(Wrap { trim: true })
            .block(block),
        area,
    );
}
