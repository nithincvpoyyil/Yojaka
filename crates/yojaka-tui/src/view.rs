use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, Mode};
use crate::{screens, theme};

pub fn render(app: &App, f: &mut Frame) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(f.area());

    let (label, style) = match app.mode {
        Mode::ReadOnly => ("[READ-ONLY]", theme::badge_read_only()),
        Mode::Write => ("[WRITE]", theme::badge_write()),
    };
    let head = Line::from(vec![
        Span::styled(
            format!("Yojaka · {} · ", app.root.display()),
            theme::header(),
        ),
        Span::styled(label, style),
    ]);
    f.render_widget(Paragraph::new(head), header);

    screens::dashboard::render(app, f, body);
    if app.show_help {
        screens::help::render(f, body);
    }

    f.render_widget(
        Paragraph::new("q quit · ? help · 1–5 screens").style(theme::footer()),
        footer,
    );
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::app::Screen;

    fn draw(app: &App, w: u16, h: u16) -> String {
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| render(app, f)).unwrap();
        t.backend().to_string()
    }

    fn app() -> App {
        App::new("/photos".into(), Mode::ReadOnly)
    }

    #[test]
    fn dashboard_80x24() {
        insta::assert_snapshot!(draw(&app(), 80, 24));
    }

    #[test]
    fn dashboard_160x48() {
        insta::assert_snapshot!(draw(&app(), 160, 48));
    }

    #[test]
    fn help_80x24() {
        let mut a = app();
        a.show_help = true;
        insta::assert_snapshot!(draw(&a, 80, 24));
    }

    #[test]
    fn help_160x48() {
        let mut a = app();
        a.show_help = true;
        insta::assert_snapshot!(draw(&a, 160, 48));
    }

    #[test]
    fn placeholder_screen() {
        let mut a = app();
        a.screen = Screen::Duplicates;
        assert!(draw(&a, 80, 24).contains("Arrives in Iteration 4"));
    }
}
