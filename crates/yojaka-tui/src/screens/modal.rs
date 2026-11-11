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
        Modal::EnableWrite { then_review } => popup(
            f,
            body,
            "Turn on write mode?",
            vec![
                Line::raw("Write mode lets Yojaka copy, move or trash files, but only after"),
                Line::raw("you review a plan and confirm it. Nothing is deleted for good here."),
                Line::raw("Yojaka has no undo: a move or a copy is your decision, and final."),
                Line::raw(""),
                Line::styled(
                    if *then_review {
                        "y / Enter  turn on and review      n / Esc  cancel"
                    } else {
                        "y / Enter  turn on      n / Esc  cancel"
                    },
                    theme::accent(),
                ),
            ],
            68,
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
    use yojaka_core::plan::OpMode;
    let n = fmt::thousands(p.file_ops() as u64);
    let head = match p.mode {
        OpMode::Copy => format!(
            "Copy {n} files ({}) to {}. The originals stay.",
            fmt::bytes(p.summary.bytes),
            p.dest
                .as_deref()
                .map(|d| d.display().to_string())
                .unwrap_or_default()
        ),
        OpMode::Move => format!(
            "Move {n} files ({}) to {}.",
            fmt::bytes(p.summary.bytes),
            p.dest
                .as_deref()
                .map(|d| d.display().to_string())
                .unwrap_or_default()
        ),
        _ => format!(
            "Move {n} files ({}) to the Trash, keeping one copy of each of {} groups.",
            fmt::bytes(p.summary.bytes),
            fmt::thousands(p.summary.groups as u64)
        ),
    };
    let mut v = vec![Line::styled(head, theme::title())];
    if p.summary.live_pairs > 0 {
        v.push(Line::raw(format!(
            "{} Live Photo pairs: both halves go together.",
            p.summary.live_pairs
        )));
    }
    let lookalikes = p.lookalike_ops();
    if lookalikes > 0 {
        v.push(Line::styled(
            format!(
                "{} of these are not identical to the copy that stays: you matched them by eye.",
                fmt::thousands(lookalikes as u64)
            ),
            theme::error(),
        ));
    }
    for w in &p.warnings {
        v.push(Line::styled(
            format!("warning: {}", w.message),
            theme::error(),
        ));
    }
    v.push(Line::styled(
        if p.dest.is_some() {
            "Files by new folder:"
        } else {
            "Files removed, by folder:"
        },
        theme::dim(),
    ));
    // Leave room for the frame, the lines above and the confirmation below.
    let room = (body.height as usize).saturating_sub(v.len() + 7).max(1);
    let shown = r.glimpse.len().min(room);
    let width = 84.min(body.width) as usize;
    for l in r.glimpse.iter().take(shown) {
        let sign = if p.dest.is_some() { "+" } else { "−" };
        let tail = format!(
            "  {sign}{} files ({})",
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
            if p.dest.is_some() {
                "y / Enter  go ahead      n / Esc  cancel"
            } else {
                "y / Enter  move to the Trash      n / Esc  cancel"
            },
            theme::accent(),
        ));
    }
    popup(f, body, "Review", v, 84);
}
