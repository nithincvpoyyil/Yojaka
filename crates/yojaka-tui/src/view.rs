use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

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

/// Layout of the Duplicates screen: a summary box, then groups (left) and copies (right).
pub struct DupesLayout {
    pub summary: Rect,
    pub groups: Rect,
    pub copies: Rect,
}

pub fn dupes_layout(body: Rect) -> DupesLayout {
    // The summary has two lines: identical groups, then the groups to decide by hand.
    let [summary, rest] = Layout::vertical([Constraint::Length(4), Constraint::Min(3)]).areas(body);
    // Room for `▶ 1234 same name  12×  123.4 MB`.
    let left_w = if body.width >= 120 { 40 } else { 34 };
    let [groups, copies] =
        Layout::horizontal([Constraint::Length(left_w), Constraint::Min(10)]).areas(rest);
    DupesLayout {
        summary,
        groups,
        copies,
    }
}

/// Layout of the History screen: the runs, then details of the selected one.
pub struct HistoryLayout {
    pub list: Rect,
    pub detail: Rect,
}

pub fn history_layout(body: Rect) -> HistoryLayout {
    let [list, detail] = Layout::vertical([Constraint::Min(3), Constraint::Length(6)]).areas(body);
    HistoryLayout { list, detail }
}

/// Rows the run list shows at `w` × `h` (borders excluded).
pub fn history_rows(w: u16, h: u16) -> u16 {
    history_layout(body_area(Rect::new(0, 0, w, h)))
        .list
        .height
        .saturating_sub(2)
}

/// Rows the group list shows at `w` × `h` (borders and the header row excluded).
pub fn dupes_rows(w: u16, h: u16) -> u16 {
    dupes_layout(body_area(Rect::new(0, 0, w, h)))
        .groups
        .height
        .saturating_sub(3)
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
    let [_, _, body, _] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(TAB_ROWS),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(full);
    body
}

pub fn render(app: &App, f: &mut Frame) {
    f.render_widget(Block::default().style(theme::base()), f.area());
    let [header, tabs, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(TAB_ROWS),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(f.area());

    let (label, style) = match app.mode {
        Mode::ReadOnly => ("[READ-ONLY]", theme::badge_read_only()),
        Mode::Write => ("[WRITE]", theme::badge_write()),
    };
    // Right side: the loader while a scan runs (so every screen shows it), else the status.
    let scan = &app.dashboard.scan;
    let status = app.status_line();
    let mut right: Vec<Span> = vec![];
    let ratio = if scan.running {
        scan.ratio()
    } else {
        app.dupes.ratio()
    };
    if scan.running || app.dupes.searching() {
        let spin =
            screens::dashboard::SPINNER[app.ticks as usize % screens::dashboard::SPINNER.len()];
        right.push(Span::styled(format!("{spin} "), theme::title()));
        right.push(Span::styled(format!("{status} "), theme::header()));
        let bar = match ratio {
            Some(r) => screens::dashboard::ratio_bar(12, r, theme::bar()),
            None => screens::dashboard::indeterminate(12, app.ticks),
        };
        right.extend(bar.spans);
    } else {
        right.push(Span::styled(status, theme::dim()));
    }
    let right_w: usize = right.iter().map(|s| s.content.chars().count()).sum();
    let [left_area, right_area] = Layout::horizontal([
        Constraint::Min(1),
        Constraint::Length((right_w as u16 + 1).min(header.width)),
    ])
    .areas(header);
    let mut left = vec![Span::styled("Yojaka · ", theme::header())];
    left.push(Span::styled(label, style));
    f.render_widget(Paragraph::new(Line::from(left)), left_area);
    f.render_widget(Paragraph::new(Line::from(right)), right_area);
    f.render_widget(
        Paragraph::new(tab_bar(app.screen, tabs.width, app.tab_mode)),
        tabs,
    );

    match app.screen {
        Screen::Actions => screens::actions::render(app, f, body),
        Screen::Dashboard => screens::dashboard::render(app, f, body),
        Screen::Browser => screens::browser::render(app, f, body),
        Screen::Duplicates => screens::duplicates::render(app, f, body),
        Screen::Organize => screens::organize::render(app, f, body),
        Screen::History => screens::history::render(app, f, body),
    }
    if app.show_help {
        screens::help::render(f, body, app.screen);
    }
    if app.confirm_flush {
        render_confirm(f, body, app.stale_index);
    }
    if let Some(m) = &app.modal {
        screens::modal::render(f, body, m);
    }
    if app.debug {
        render_debug(app, f, body);
    }

    f.render_widget(Paragraph::new(footer_line(app, footer.width)), footer);
}

const SCREENS: [Screen; 6] = Screen::ALL;

/// Height of the tab strip: top edge, labels, and the line the tabs stand on.
const TAB_ROWS: u16 = 3;

/// The screens as folder tabs; the current one is yellow and open at the bottom, so it joins
/// the panels below. Narrow terminals keep the numbers and name only the current screen.
pub fn tab_bar(current: Screen, width: u16, active: bool) -> Vec<Line<'static>> {
    let width = usize::from(width);
    let label = |i: usize, s: Screen, named: bool| {
        if named || s == current {
            format!(" {} {} ", i + 1, s.title())
        } else {
            format!(" {} ", i + 1)
        }
    };
    let full: usize = SCREENS
        .iter()
        .enumerate()
        .map(|(i, s)| label(i, *s, true).chars().count() + 2)
        .sum::<usize>()
        + 1;
    let named = full <= width;

    let (mut top, mut mid, mut low) = (vec![Span::raw(" ")], vec![Span::raw(" ")], vec![]);
    low.push(Span::styled("─", theme::tab_border()));
    let mut used = 1;
    for (i, s) in SCREENS.iter().enumerate() {
        let text = label(i, *s, named);
        let w = text.chars().count();
        used += w + 2;
        let on = *s == current;
        let edge = if on {
            theme::tab_border_selected()
        } else {
            theme::tab_border()
        };
        top.push(Span::styled(format!("╭{}╮", "─".repeat(w)), edge));
        mid.push(Span::styled("│", edge));
        mid.push(Span::styled(
            text,
            if on && active {
                theme::tab_active()
            } else if on {
                theme::tab_selected()
            } else {
                theme::tab()
            },
        ));
        mid.push(Span::styled("│", edge));
        if on {
            low.push(Span::styled(format!("┘{}└", " ".repeat(w)), edge));
        } else {
            low.push(Span::styled(format!("┴{}┴", "─".repeat(w)), edge));
        }
    }
    low.push(Span::styled(
        "─".repeat(width.saturating_sub(used)),
        theme::tab_border(),
    ));
    vec![Line::from(top), Line::from(mid), Line::from(low)]
}

fn render_confirm(f: &mut Frame, body: Rect, stale: bool) {
    let mut lines = vec![
        Line::styled("Scan this folder from scratch?", theme::title()),
        Line::raw(""),
    ];
    if stale {
        lines.extend([
            Line::styled(
                "This folder's saved data was made by another Yojaka build,",
                theme::accent(),
            ),
            Line::styled("and this one cannot use it.", theme::accent()),
            Line::raw(""),
        ]);
    }
    lines.extend([
        Line::raw("Yojaka forgets what it saved about this folder and reads it all"),
        Line::raw("again, so it takes longer than a normal scan. Your photos and"),
        Line::raw("videos are not touched."),
        Line::raw(""),
        Line::styled(
            "y / Enter  scan from scratch      n / Esc  cancel",
            theme::accent(),
        ),
    ]);
    let height = (lines.len() as u16 + 2).min(body.height);
    let width = 66.min(body.width);
    let rect = Rect::new(
        body.x + body.width.saturating_sub(width) / 2,
        body.y + body.height.saturating_sub(height) / 2,
        width,
        height,
    );
    theme::clear(f, rect);
    let block = theme::panel("Scan from scratch", true);
    f.render_widget(Paragraph::new(lines).block(block), rect);
}

/// One key in the footer: the key, what it does, and whether it is the one main next step
/// (drawn as a chip).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub key: &'static str,
    pub what: String,
    pub main: bool,
}

/// A key and what it does.
pub fn hint(key: &'static str, what: impl Into<String>) -> Hint {
    Hint {
        key,
        what: what.into(),
        main: false,
    }
}

/// The one main next step on a screen.
pub fn main_hint(key: &'static str, what: impl Into<String>) -> Hint {
    Hint {
        main: true,
        ..hint(key, what)
    }
}

/// The keys of the current state: this screen's keys (main one first), and whether the keys
/// that work everywhere (`1-6 tabs  ? help  q quit`) follow on the right.
pub fn footer_hints(app: &App) -> (Vec<Hint>, bool) {
    if let Some(m) = &app.modal {
        use crate::modal::Modal;
        let keys = match m {
            Modal::Running(_) => vec![hint("Ctrl-C", "stop after this file")],
            Modal::Report(_) => vec![hint("any key", "close")],
            Modal::Review(r) if r.needs_typing() => vec![
                hint("Enter", "confirm (once the phrase matches)"),
                hint("Esc", "cancel"),
            ],
            _ => vec![hint("y", "yes"), hint("n", "cancel")],
        };
        return (keys, false);
    }
    if app.confirm_flush {
        return (
            vec![hint("y", "scan from scratch"), hint("n", "cancel")],
            false,
        );
    }
    if app.tab_mode {
        return (
            vec![
                hint("← →", "switch tab"),
                hint("Enter", "back to the screen"),
                hint("Esc", "back"),
            ],
            false,
        );
    }
    if app
        .plan_state_of(app.screen)
        .is_some_and(|o| o.editing.is_some())
    {
        return (
            vec![
                hint("Enter", "keep"),
                hint("Esc", "undo the edit"),
                hint("Ctrl-U", "clear"),
            ],
            false,
        );
    }
    let scan = if app.dashboard.scan.running {
        hint("Esc", "stop scan")
    } else {
        hint("r", "rescan")
    };
    let keys = match app.screen {
        Screen::Actions => return crate::screens::actions::footer(app),
        Screen::Dashboard => vec![scan, hint("R", "full rescan")],
        Screen::Browser => vec![
            hint("Tab", "folders / files"),
            hint("Enter", "open"),
            hint("/", "filter"),
            hint("t", "photos / videos"),
            hint("s", "sort"),
            scan,
        ],
        Screen::Duplicates => dupes_hints(app),
        Screen::Organize => {
            use crate::organize::Row;
            let change = match app.organize.row() {
                Row::Pattern | Row::Dest => hint("Enter", "edit"),
                _ => hint("Enter ← →", "change"),
            };
            vec![main_hint("a", "review & run"), hint("↑↓", "choose"), change]
        }
        Screen::History if app.history.runs.is_empty() => vec![],
        Screen::History => vec![hint("↑↓", "move"), hint("d", "delete history")],
    };
    (keys, true)
}

fn dupes_hints(app: &App) -> Vec<Hint> {
    use crate::dupes::{Pane, Phase};
    let d = &app.dupes;
    if d.searching() {
        return vec![hint("c", "stop the search")];
    }
    if d.phase != Phase::Done || d.groups.is_empty() {
        return vec![
            main_hint("f", "look for duplicates"),
            hint("F", "also compare pictures"),
        ];
    }
    let mut v = vec![];
    let marked = d.marked_files();
    if marked > 0 {
        v.push(main_hint(
            "a",
            format!("review {}", fmt::files(marked as u64)),
        ));
    }
    v.push(hint("Tab", "groups / copies"));
    v.push(hint("Space", "trash / keep"));
    v.push(if d.pane == Pane::Groups {
        hint("Enter", "open group")
    } else {
        hint("Enter", "keep only this")
    });
    v.push(hint("p", "keep rule"));
    v.push(hint("v", "show"));
    v.push(hint("↑↓", "move"));
    v.push(hint("f", "look again"));
    v
}

/// The keys that work everywhere, on the right of the footer.
fn global_hints() -> Vec<Hint> {
    vec![hint("1-6", "tabs"), hint("?", "help"), hint("q", "quit")]
}

fn hint_spans(hints: &[Hint]) -> Vec<Span<'static>> {
    let mut spans = vec![];
    for (i, h) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        if h.main {
            spans.push(Span::styled(
                format!(" {} {} ", h.key, h.what),
                theme::key_chip(),
            ));
        } else {
            spans.push(Span::styled(h.key, theme::key_hint()));
            spans.push(Span::styled(format!(" {}", h.what), theme::footer()));
        }
    }
    spans
}

fn hints_width(hints: &[Hint]) -> usize {
    hint_spans(hints)
        .iter()
        .map(|s| s.content.chars().count())
        .sum()
}

/// Bottom hint line, `width` cells wide: this screen's keys on the left, the keys that work
/// everywhere on the right. When it does not fit, the least useful keys go first: `1-6 tabs`
/// (the tab strip shows the numbers), then this screen's last keys (never the main one), then
/// `? help`. `q quit` stays.
fn footer_line(app: &App, width: u16) -> Line<'static> {
    if let Some((msg, _)) = &app.notice {
        return Line::styled(msg.clone(), theme::accent());
    }
    if app.screen == Screen::Browser && app.browser.editing_filter {
        return Line::from(vec![
            Span::styled("Filter: ", theme::key_hint()),
            Span::raw(format!("{}▏", app.browser.query.text)),
            Span::styled("   ", theme::footer()),
            Span::styled("Enter", theme::key_hint()),
            Span::styled(" keep  ", theme::footer()),
            Span::styled("Esc", theme::key_hint()),
            Span::styled(" clear", theme::footer()),
        ]);
    }
    let width = usize::from(width);
    let (mut left, global) = footer_hints(app);
    let mut right = if global { global_hints() } else { vec![] };
    let fits = |l: &[Hint], r: &[Hint]| {
        let gap = if l.is_empty() || r.is_empty() { 0 } else { 3 };
        hints_width(l) + gap + hints_width(r) <= width
    };
    while !fits(&left, &right) {
        if right.len() > 2 {
            right.remove(0);
        } else if let Some(i) = left.iter().rposition(|h| !h.main) {
            left.remove(i);
        } else if right.len() > 1 {
            right.remove(0);
        } else {
            break;
        }
    }
    let mut spans = hint_spans(&left);
    if !right.is_empty() {
        let used = hints_width(&left) + hints_width(&right);
        spans.push(Span::raw(" ".repeat(width.saturating_sub(used).max(3))));
        spans.extend(hint_spans(&right));
    }
    Line::from(spans)
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
    theme::clear(f, rect);
    f.render_widget(Paragraph::new(text).style(theme::selected()), rect);
}
