//! History: every plan that was run, newest first. `u` puts a run back.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use yojaka_core::exec::{RunStatus, RunSummary};

use crate::app::{App, Mode};
use crate::view::history_layout;
use crate::{fmt, theme};

pub fn render(app: &App, f: &mut Frame, area: Rect) {
    let l = history_layout(area);
    render_list(app, f, l.list);
    render_detail(app, f, l.detail);
}

/// `2026-11-09 14:03` in the local time zone, falling back to UTC.
pub fn when(ts: i64) -> String {
    use time::macros::format_description;
    let fmt = format_description!("[year]-[month]-[day] [hour]:[minute]");
    let utc = time::OffsetDateTime::from_unix_timestamp(ts).ok();
    let local = utc.and_then(|t| {
        time::UtcOffset::local_offset_at(t)
            .ok()
            .map(|o| t.to_offset(o))
    });
    local
        .or(utc)
        .and_then(|t| t.format(&fmt).ok())
        .unwrap_or_else(|| ts.to_string())
}

fn status_style(s: RunStatus) -> ratatui::style::Style {
    match s {
        RunStatus::Applied => theme::good(),
        RunStatus::Incomplete => theme::error(),
        RunStatus::PartlyUndone | RunStatus::Undone => theme::dim(),
    }
}

fn render_list(app: &App, f: &mut Frame, area: Rect) {
    let h = &app.history;
    let block = theme::panel("Runs", true);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if h.runs.is_empty() {
        let msg = if h.loaded {
            "Nothing has been run yet. Find duplicates (3), mark them, then press a to review."
        } else {
            "Loading…"
        };
        f.render_widget(Paragraph::new(msg).style(theme::dim()), inner);
        return;
    }
    let rows = inner.height as usize;
    let top = h.selected.saturating_sub(rows.saturating_sub(1));
    let width = inner.width as usize;
    let lines: Vec<Line> = h
        .runs
        .iter()
        .enumerate()
        .skip(top)
        .take(rows)
        .map(|(i, r)| {
            let fixed = format!(
                "{}  {:<13} {:>7} files  {:>9}  ",
                when(r.started),
                r.status.label(),
                fmt::thousands(r.done as u64),
                fmt::bytes(r.bytes)
            );
            let rest = width.saturating_sub(fixed.chars().count());
            let folder = fmt::truncate_start(&r.root.display().to_string(), rest);
            let selected = i == h.selected;
            let mut spans = vec![
                Span::raw(format!("{}  ", when(r.started))),
                Span::styled(
                    format!("{:<13} ", r.status.label()),
                    if selected {
                        theme::selected()
                    } else {
                        status_style(r.status)
                    },
                ),
                Span::raw(format!(
                    "{:>7} files  {:>9}  {folder}",
                    fmt::thousands(r.done as u64),
                    fmt::bytes(r.bytes)
                )),
            ];
            if selected {
                for s in &mut spans {
                    s.style = theme::selected();
                }
            }
            Line::from(spans)
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

fn render_detail(app: &App, f: &mut Frame, area: Rect) {
    let block = theme::panel("Selected run", false);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let Some(r) = app.history.current() else {
        return;
    };
    f.render_widget(Paragraph::new(detail_lines(r, app.mode)), inner);
}

fn detail_lines(r: &RunSummary, mode: Mode) -> Vec<Line<'static>> {
    let mut v = vec![
        Line::raw(format!("Folder: {}", r.root.display())),
        Line::raw(format!(
            "{} of {} steps done · {} left alone · {} failed · {} put back",
            r.done, r.ops, r.skipped, r.failed, r.undone
        )),
        Line::styled(format!("Run {}", r.plan_id), theme::dim()),
    ];
    if r.status == RunStatus::Incomplete {
        v.push(Line::styled(
            "Stopped before the end. Finish it with `yojaka apply <plan> --resume --write`.",
            theme::error(),
        ));
    } else if crate::history::can_undo(r) {
        v.push(Line::styled(
            if mode == Mode::Write {
                "u  put the files back"
            } else {
                "W turns write mode on, then u puts the files back"
            },
            theme::accent(),
        ));
    }
    v
}
