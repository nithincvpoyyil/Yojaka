//! Duplicates: groups of identical files (left), the copies of the selected group (right), and a
//! summary on top. Marks are a proposal only; this iteration never deletes anything.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use yojaka_core::dupes::DupGroup;

use crate::app::App;
use crate::dupes::{DupesState, Pane, Phase};
use crate::screens::dashboard::{SPINNER, indeterminate, ratio_bar};
use crate::view::dupes_layout;
use crate::{fmt, theme};

pub fn render(app: &App, f: &mut Frame, area: Rect) {
    let l = dupes_layout(area);
    render_summary(app, f, l.summary);
    render_groups(app, f, l.groups);
    render_copies(&app.dupes, f, l.copies);
}

fn render_summary(app: &App, f: &mut Frame, area: Rect) {
    let d = &app.dupes;
    let block = theme::panel("Exact duplicates", false);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let w = inner.width as usize;
    let line = match &d.phase {
        Phase::Searching => {
            let spin = SPINNER[app.ticks as usize % SPINNER.len()];
            let mut spans = vec![Span::styled(format!("{spin} "), theme::title())];
            let counts = match d.total {
                Some(t) => format!(
                    "comparing files {}/{} · {} read  ",
                    fmt::thousands(d.done),
                    fmt::thousands(t),
                    fmt::bytes(d.bytes)
                ),
                None => "looking for files of equal size  ".to_string(),
            };
            spans.push(Span::raw(counts));
            let bar = match d.ratio() {
                Some(r) => ratio_bar(12, r, theme::bar()),
                None => indeterminate(12, app.ticks),
            };
            spans.extend(bar.spans);
            Line::from(spans)
        }
        Phase::Done if d.groups.is_empty() => {
            let n = d.stats.as_ref().map_or(0, |s| s.candidate_files);
            Line::styled(
                format!(
                    "No identical files among the {} files that share a size.",
                    fmt::thousands(n)
                ),
                theme::good(),
            )
        }
        Phase::Done => {
            let mut spans = vec![
                Span::styled(
                    format!("{} groups", fmt::thousands(d.groups.len() as u64)),
                    theme::title(),
                ),
                Span::raw(format!(
                    " · {} files marked · frees {} · rule: {}",
                    fmt::thousands(d.marked_files() as u64),
                    fmt::bytes(d.total_reclaim()),
                    d.preset_label()
                )),
            ];
            if d.issues > 0 {
                spans.push(Span::styled(
                    format!(" · {} skipped", d.issues),
                    theme::error(),
                ));
            }
            Line::from(spans)
        }
        Phase::Cancelled => Line::styled("Search stopped. Press f to start again.", theme::dim()),
        Phase::Failed(e) => Line::styled(fmt::truncate(e, w), theme::error()),
        Phase::Idle if app.dashboard.no_index => {
            Line::styled("This folder has not been scanned yet.", theme::dim())
        }
        Phase::Idle => Line::from(vec![
            Span::styled("f", theme::key_hint()),
            Span::styled(" finds identical files. Nothing is deleted.", theme::dim()),
        ]),
    };
    f.render_widget(Paragraph::new(line), inner);
}

/// First row to show so that `selected` stays visible, roughly centered.
fn window(selected: usize, len: usize, rows: usize) -> usize {
    if len <= rows {
        0
    } else {
        selected.saturating_sub(rows / 2).min(len - rows)
    }
}

fn render_groups(app: &App, f: &mut Frame, area: Rect) {
    let d = &app.dupes;
    let focused = d.pane == Pane::Groups;
    let title = if d.groups.is_empty() {
        "Groups".to_string()
    } else {
        format!("Groups {}/{}", d.group + 1, d.groups.len())
    };
    let block = theme::panel(title, focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if d.groups.is_empty() {
        let msg = match d.phase {
            Phase::Searching => "searching…",
            Phase::Done => "nothing to review",
            _ => "no results yet",
        };
        f.render_widget(Paragraph::new(msg).style(theme::dim()), inner);
        return;
    }
    let rows = inner.height as usize;
    let top = window(d.group, d.groups.len(), rows);
    let lines: Vec<Line> = d
        .groups
        .iter()
        .enumerate()
        .skip(top)
        .take(rows)
        .map(|(i, g)| {
            let text = format!(
                "{:>5} {:>2}× {:>9}{}",
                g.id,
                g.members.len(),
                fmt::bytes(g.reclaim()),
                if g.members.iter().any(|m| m.is_pair()) {
                    " ◉"
                } else {
                    ""
                }
            );
            let text = fmt::truncate(&text, inner.width as usize);
            if i == d.group {
                let style = if focused {
                    theme::selected()
                } else {
                    theme::selected_dim()
                };
                Line::styled(format!("{text:<w$}", w = inner.width as usize), style)
            } else {
                Line::from(text)
            }
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

fn copy_line(g: &DupGroup, i: usize, width: usize) -> Vec<Span<'static>> {
    let m = &g.members[i];
    let marked = g.delete[i];
    let (tag, style) = if marked {
        ("✕ delete ", theme::error())
    } else {
        ("✓ keep   ", theme::good())
    };
    let size = format!(" {:>9}", fmt::bytes(m.total_size()));
    let suffix = if m.is_pair() { " +video" } else { "" };
    let path_w = width.saturating_sub(tag.chars().count() + size.chars().count() + suffix.len());
    let path = fmt::truncate_start(&m.primary().path.display().to_string(), path_w);
    vec![
        Span::styled(tag, style),
        Span::raw(format!("{path:<path_w$}")),
        Span::styled(suffix, theme::dim()),
        Span::raw(size),
    ]
}

fn render_copies(d: &DupesState, f: &mut Frame, area: Rect) {
    let focused = d.pane == Pane::Copies;
    let Some(g) = d.current() else {
        let block = theme::panel("Copies", focused);
        f.render_widget(block, area);
        return;
    };
    let title = format!(
        "Group {} · {} copies{}",
        g.id,
        g.members.len(),
        if g.members.iter().any(|m| m.is_pair()) {
            " · Live Photos (photo and video move together)"
        } else {
            ""
        }
    );
    let block = theme::panel(title, focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    // The last line shows the whole path of the selected copy, which the list may have cut.
    let list_rows = inner.height.saturating_sub(2) as usize;
    let top = window(d.copy, g.members.len(), list_rows.max(1));
    let width = inner.width as usize;
    let mut lines: Vec<Line> = (top..g.members.len())
        .take(list_rows)
        .map(|i| {
            let spans = copy_line(g, i, width);
            if i == d.copy {
                let style = if focused {
                    theme::selected()
                } else {
                    theme::selected_dim()
                };
                Line::from(
                    spans
                        .into_iter()
                        .map(|s| Span::styled(s.content, style))
                        .collect::<Vec<_>>(),
                )
            } else {
                Line::from(spans)
            }
        })
        .collect();
    while lines.len() < list_rows {
        lines.push(Line::raw(""));
    }
    if inner.height >= 2
        && let Some(m) = g.members.get(d.copy)
    {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            fmt::truncate_start(&m.primary().path.display().to_string(), width),
            Style::new().patch(theme::dim()),
        ));
    }
    f.render_widget(Paragraph::new(lines), inner);
}
