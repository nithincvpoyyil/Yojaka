//! History: every plan that was run, newest first. `d` deletes the history of the finished runs.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use yojaka_core::exec::{RunStatus, RunSummary};
use yojaka_core::plan::OpMode;

use crate::app::App;
use crate::view::history_layout;
use crate::{fmt, theme};

pub fn render(app: &App, f: &mut Frame, area: Rect) {
    if app.history.runs.is_empty() {
        // Nothing to select, so no details panel.
        render_list(app, f, area);
        return;
    }
    let l = history_layout(area);
    render_list(app, f, l.list);
    render_detail(app, f, l.detail);
}

/// `2026-11-09 14:03` in the local time zone (same as `yojaka history`).
pub fn when(ts: i64) -> String {
    yojaka_core::meta::format_run_time(ts)
}

fn status_style(s: RunStatus) -> ratatui::style::Style {
    match s {
        RunStatus::Applied => theme::good(),
        RunStatus::Incomplete => theme::error(),
    }
}

fn render_list(app: &App, f: &mut Frame, area: Rect) {
    let h = &app.history;
    let block = theme::panel("Runs", true);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if h.runs.is_empty() {
        let msg = if h.loaded {
            "Nothing has been changed yet. Every copy, move or trip to the Trash made from \
             Actions, Duplicates or Organize is listed here, newest first."
        } else {
            "Loading…"
        };
        f.render_widget(
            Paragraph::new(msg)
                .style(theme::dim())
                .wrap(ratatui::widgets::Wrap { trim: true }),
            inner,
        );
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
            let rest = width.saturating_sub(fixed.chars().count() + 2);
            let folder = fmt::truncate_start(&fmt::home(&r.root), rest);
            let selected = i == h.selected;
            let mut spans = vec![
                Span::raw(format!(
                    "{}{}  ",
                    if selected { "▶ " } else { "  " },
                    when(r.started)
                )),
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
    f.render_widget(Paragraph::new(detail_lines(r)), inner);
}

fn detail_lines(r: &RunSummary) -> Vec<Line<'static>> {
    let what = match r.mode {
        OpMode::Copy => "Copied",
        OpMode::Move => "Moved",
        OpMode::Trash => "Moved to the Trash",
        OpMode::DeletePermanent => "Deleted",
    };
    let mut v = vec![
        Line::raw(format!(
            "{what}: {} of {} steps done · {} left alone · {} failed",
            r.done, r.ops, r.skipped, r.failed
        )),
        Line::raw(format!("Folder: {}", fmt::home(&r.root))),
    ];
    if r.status == RunStatus::Incomplete {
        v.push(Line::styled(
            "Stopped before the end. Finish it with `yojaka apply <plan> --resume --write`.",
            theme::error(),
        ));
    }
    v.push(Line::styled(
        if r.mode == OpMode::Trash {
            "Yojaka has no undo; to get a file back, open the Trash in Finder and choose Put Back."
        } else {
            "Yojaka has no undo: a move or a copy is final."
        },
        theme::dim(),
    ));
    v.push(Line::styled(format!("Run {}", r.plan_id), theme::dim()));
    v
}
