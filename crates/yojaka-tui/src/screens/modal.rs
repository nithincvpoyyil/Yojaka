//! Renders the pop-ups from `modal.rs`.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::modal::{Modal, Review, RunView};
use crate::screens::dashboard::ratio_bar;
use crate::{fmt, theme};

pub fn render(f: &mut Frame, body: Rect, modal: &Modal) {
    match modal {
        Modal::EnableWrite => popup(
            f,
            body,
            "Turn on write mode?",
            vec![
                Line::raw("Write mode lets Yojaka move files to the Trash, but only after you"),
                Line::raw("have reviewed a plan. Nothing is ever deleted for good from here,"),
                Line::raw("and every run can be put back from the History screen."),
                Line::raw(""),
                Line::styled("y / Enter  turn on      n / Esc  cancel", theme::accent()),
            ],
            64,
        ),
        Modal::Undo(run) => popup(
            f,
            body,
            "Put the files back?",
            vec![
                Line::raw(format!(
                    "{} files ({}) go back where they were.",
                    fmt::thousands(run.done.saturating_sub(run.undone) as u64),
                    fmt::bytes(run.bytes)
                )),
                Line::raw("A file is never put over one that is already there."),
                Line::raw(""),
                Line::styled("y / Enter  put back      n / Esc  cancel", theme::accent()),
            ],
            64,
        ),
        Modal::Running(r) => running(f, body, r),
        Modal::Report(lines) => {
            let mut v: Vec<Line> = lines
                .iter()
                .map(|l| {
                    if l.bad {
                        Line::styled(l.text.clone(), theme::error())
                    } else {
                        Line::raw(l.text.clone())
                    }
                })
                .collect();
            v.push(Line::raw(""));
            v.push(Line::styled("Press any key", theme::accent()));
            popup(f, body, "Done", v, 78);
        }
        Modal::Review(r) => review(f, body, r),
    }
}

fn popup(f: &mut Frame, body: Rect, title: &str, lines: Vec<Line<'_>>, want_w: u16) {
    let width = want_w.min(body.width);
    // Wrapped lines can take more than one row each.
    let inner_w = width.saturating_sub(2).max(1) as usize;
    let rows: usize = lines
        .iter()
        .map(|l| l.width().div_ceil(inner_w).max(1))
        .sum();
    let height = (rows as u16 + 2).min(body.height);
    let rect = Rect::new(
        body.x + body.width.saturating_sub(width) / 2,
        body.y + body.height.saturating_sub(height) / 2,
        width,
        height,
    );
    theme::clear(f, rect);
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(theme::panel(title.to_string(), true)),
        rect,
    );
}

fn running(f: &mut Frame, body: Rect, r: &RunView) {
    let mut spans = vec![Span::raw(format!(
        "{}/{}  ",
        fmt::thousands(r.done),
        r.total.map_or_else(|| "?".into(), fmt::thousands)
    ))];
    if let Some(ratio) = r.ratio() {
        spans.extend(ratio_bar(24, ratio, theme::bar()).spans);
    }
    let hint = if r.cancelling {
        "Stopping after the current file…"
    } else {
        "Ctrl-C stops after the current file; nothing is left half done."
    };
    popup(
        f,
        body,
        r.label,
        vec![
            Line::from(spans),
            Line::raw(""),
            Line::styled(hint, theme::dim()),
        ],
        64,
    );
}

fn review(f: &mut Frame, body: Rect, r: &Review) {
    let p = &r.plan;
    let mut v = vec![Line::styled(
        format!(
            "Move {} files ({}) to the Trash, keeping one copy of each of {} groups.",
            fmt::thousands(p.ops.len() as u64),
            fmt::bytes(p.summary.bytes),
            fmt::thousands(p.summary.groups as u64)
        ),
        theme::title(),
    )];
    if p.summary.live_pairs > 0 {
        v.push(Line::raw(format!(
            "{} Live Photo pairs: both halves go together.",
            p.summary.live_pairs
        )));
    }
    for w in &p.warnings {
        v.push(Line::styled(
            format!("warning: {}", w.message),
            theme::error(),
        ));
    }
    v.push(Line::styled("Files removed, by folder:", theme::dim()));
    // Leave room for the frame, the lines above and the confirmation below.
    let room = (body.height as usize).saturating_sub(v.len() + 7).max(1);
    let shown = r.glimpse.len().min(room);
    let width = 84.min(body.width) as usize;
    for l in r.glimpse.iter().take(shown) {
        let tail = format!(
            "  −{} files ({})",
            fmt::thousands(l.files),
            fmt::bytes(l.bytes)
        );
        let indent = "  ".repeat(l.depth + 1);
        let label_w = width.saturating_sub(4 + indent.len() + tail.chars().count());
        let label = fmt::truncate_start(&l.label, label_w);
        let style = if l.is_more {
            theme::dim()
        } else {
            ratatui::style::Style::new()
        };
        v.push(Line::styled(format!("{indent}{label}{tail}"), style));
    }
    if shown < r.glimpse.len() {
        v.push(Line::styled(
            format!(
                "  … {} more lines (yojaka plan show --full-tree)",
                r.glimpse.len() - shown
            ),
            theme::dim(),
        ));
    }
    v.push(Line::raw(""));
    if r.needs_typing() {
        v.push(Line::raw(format!(
            "Type \"{}\" and press Enter:",
            p.confirm_phrase()
        )));
        v.push(Line::styled(format!("> {}▏", r.typed), theme::accent()));
    } else {
        v.push(Line::styled(
            "y / Enter  move to the Trash      n / Esc  cancel",
            theme::accent(),
        ));
    }
    popup(f, body, "Review", v, 84);
}
