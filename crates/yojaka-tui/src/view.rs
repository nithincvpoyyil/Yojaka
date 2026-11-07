use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::app::{App, Mode, Screen};
use crate::{fmt, screens, theme};

/// Layout of the Browser screen, shared by `update` (how many rows fit) and `render`.
pub struct BrowserLayout {
    pub tree: Rect,
    pub table: Rect,
    pub detail: Rect,
}

pub fn browser_layout(body: Rect) -> BrowserLayout {
    let tree_w = if body.width >= 120 { 32 } else { 24 };
    let detail_h = if body.height >= 28 { 9 } else { 8 };
    let [tree, right] =
        Layout::horizontal([Constraint::Length(tree_w), Constraint::Min(10)]).areas(body);
    let [table, detail] =
        Layout::vertical([Constraint::Min(4), Constraint::Length(detail_h)]).areas(right);
    BrowserLayout {
        tree,
        table,
        detail,
    }
}

/// (tree rows, table rows) that fit a terminal of `w` × `h` cells.
pub fn browser_rows(w: u16, h: u16) -> (u16, u16) {
    let body = body_area(Rect::new(0, 0, w, h));
    let l = browser_layout(body);
    // borders take 2 rows; the table also has a header row
    (
        l.tree.height.saturating_sub(2),
        l.table.height.saturating_sub(3),
    )
}

fn body_area(full: Rect) -> Rect {
    let [_, body, _] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(full);
    body
}

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
        Span::styled(format!(" · {}", app.status_line()), theme::dim()),
    ]);
    f.render_widget(Paragraph::new(head), header);

    match app.screen {
        Screen::Dashboard => screens::dashboard::render(app, f, body),
        Screen::Browser => screens::browser::render(app, f, body),
        _ => screens::placeholder::render(app, f, body),
    }
    if app.show_help {
        screens::help::render(f, body);
    }
    if app.debug {
        render_debug(app, f, body);
    }

    f.render_widget(
        Paragraph::new(footer_line(app)).style(theme::footer()),
        footer,
    );
}

fn footer_line(app: &App) -> String {
    if app.screen == Screen::Browser && app.browser.editing_filter {
        return format!(
            "Filter: {}▏   Enter keep · Esc clear",
            app.browser.query.text
        );
    }
    let scan = if app.dashboard.scan.running {
        "Esc stop scan"
    } else {
        "r rescan"
    };
    match app.screen {
        Screen::Browser => {
            format!("q quit · ? help · Tab pane · / filter · t kind · s/S sort · {scan}")
        }
        _ => format!("q quit · ? help · 1–5 screens · {scan}"),
    }
}

/// Frame and fetch times in the top-right corner (toggle with `D`).
fn render_debug(app: &App, f: &mut Frame, body: Rect) {
    let d = &app.debug_info;
    let total = app.browser.total.map_or("?".into(), fmt::thousands);
    let text = format!(
        " frame {:.2} ms (max {:.2}) · fetch {:.1} ms · rows {total} ",
        d.frame_ms, d.frame_max_ms, d.fetch_ms
    );
    let w = (text.chars().count() as u16).min(body.width);
    let rect = Rect::new(body.x + body.width - w, body.y, w, 1);
    f.render_widget(Clear, rect);
    f.render_widget(Paragraph::new(text).style(theme::selected()), rect);
}
