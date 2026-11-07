use ratatui::style::{Color, Modifier, Style};

pub fn badge_read_only() -> Style {
    Style::new()
        .fg(Color::Black)
        .bg(Color::Green)
        .add_modifier(Modifier::BOLD)
}
pub fn badge_write() -> Style {
    Style::new()
        .fg(Color::Black)
        .bg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}
pub fn header() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}
pub fn footer() -> Style {
    Style::new().fg(Color::DarkGray)
}
pub fn dim() -> Style {
    Style::new().fg(Color::DarkGray)
}
pub fn accent() -> Style {
    Style::new().fg(Color::Cyan)
}
pub fn bar() -> Style {
    Style::new().fg(Color::Blue)
}
pub fn title() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}
pub fn error() -> Style {
    Style::new().fg(Color::Red)
}
pub fn good() -> Style {
    Style::new().fg(Color::Green)
}
pub fn live_badge() -> Style {
    Style::new()
        .fg(Color::Black)
        .bg(Color::Magenta)
        .add_modifier(Modifier::BOLD)
}
/// The row/node under the cursor in the focused pane.
pub fn selected() -> Style {
    Style::new().add_modifier(Modifier::REVERSED)
}
/// The row/node under the cursor in an unfocused pane.
pub fn selected_dim() -> Style {
    Style::new().fg(Color::Cyan)
}
pub fn border(focused: bool) -> Style {
    if focused {
        Style::new().fg(Color::Cyan)
    } else {
        Style::new()
    }
}
