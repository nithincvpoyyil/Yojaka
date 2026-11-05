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
