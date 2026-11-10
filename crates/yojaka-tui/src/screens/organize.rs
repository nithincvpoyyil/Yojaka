//! Organize: pick a folder pattern and destination, see the plan, then review and run it.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use yojaka_core::organize::Transfer;

use crate::app::{App, Mode};
use crate::organize::{Editing, OrganizeState, Phase};
use crate::screens::dashboard::SPINNER;
use crate::{fmt, theme};

pub fn render(app: &App, f: &mut Frame, area: Rect) {
    let [settings, examples, folders] = Layout::vertical([
        Constraint::Length(7),
        Constraint::Length(7),
        Constraint::Min(3),
    ])
    .areas(area);
    render_settings(app, f, settings);
    render_examples(&app.organize, f, examples);
    render_folders(&app.organize, f, folders);
}

fn field<'a>(label: &'a str, text: &'a str, editing: bool, tail: Span<'a>) -> Line<'a> {
    let mut spans = vec![Span::styled(format!("{label:<9}"), theme::dim())];
    if editing {
        spans.push(Span::styled(format!("{text}▏"), theme::accent()));
    } else {
        spans.push(Span::raw(text.to_string()));
    }
    spans.push(tail);
    Line::from(spans)
}

fn render_settings(app: &App, f: &mut Frame, area: Rect) {
    let o = &app.organize;
    let block = theme::panel("Organize", true);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let preset = Span::styled(format!("   ({})", o.preset_label()), theme::dim());
    let mode = match o.transfer {
        Transfer::Copy => "Copy (the originals stay)",
        Transfer::Move => "Move",
    };
    let mut lines = vec![
        field(
            "Pattern",
            &o.template,
            o.editing == Some(Editing::Template),
            preset,
        ),
        field(
            "Folder",
            &o.dest,
            o.editing == Some(Editing::Dest),
            Span::raw(""),
        ),
        Line::from(vec![
            Span::styled(format!("{:<9}", "Action"), theme::dim()),
            Span::raw(format!(
                "{mode}  ·  places for files without GPS: {}",
                if o.infer { "inferred (≈)" } else { "off" }
            )),
        ]),
    ];
    lines.push(status(app));
    if let Some(p) = &o.problem {
        lines.push(Line::styled(p.clone(), theme::error()));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn status(app: &App) -> Line<'static> {
    let o = &app.organize;
    match &o.phase {
        Phase::Idle => Line::styled(
            "Enter  work out the plan (nothing is changed)",
            theme::accent(),
        ),
        Phase::Planning => {
            let spin = SPINNER[app.ticks as usize % SPINNER.len()];
            Line::styled(format!("{spin} planning…  Esc stops"), theme::title())
        }
        Phase::Failed(e) => Line::styled(format!("Could not plan: {e}"), theme::error()),
        Phase::Ready => {
            let Some(p) = &o.plan else {
                return Line::raw("");
            };
            let s = &p.plan.summary;
            let mut text = format!(
                "{} files ({}) · {:.0}% have a place",
                fmt::thousands(p.plan.file_ops() as u64),
                fmt::bytes(s.bytes),
                p.stats.coverage() * 100.0
            );
            if s.inferred > 0 {
                text.push_str(&format!(
                    " · {} inferred",
                    fmt::thousands(s.inferred as u64)
                ));
            }
            if s.undated > 0 {
                text.push_str(&format!(" · {} undated", fmt::thousands(s.undated as u64)));
            }
            if s.skipped > 0 {
                text.push_str(&format!(
                    " · {} left alone",
                    fmt::thousands(s.skipped as u64)
                ));
            }
            if s.cross_volume > 0 {
                text.push_str(&format!(
                    " · {} cross volume",
                    fmt::thousands(s.cross_volume as u64)
                ));
            }
            let hint = if app.mode == Mode::Write {
                "  ·  a review and run"
            } else {
                "  ·  W write mode, then a"
            };
            Line::from(vec![
                Span::styled(text, theme::good()),
                Span::styled(hint, theme::accent()),
            ])
        }
    }
}

fn render_examples(o: &OrganizeState, f: &mut Frame, area: Rect) {
    let block = theme::panel("Examples", false);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let ex = o.examples(inner.height as usize);
    if ex.is_empty() {
        let msg = match o.phase {
            Phase::Ready => "Nothing to do: every file is already in place.",
            _ => "Examples of where files go appear here once the plan is worked out.",
        };
        f.render_widget(Paragraph::new(msg).style(theme::dim()), inner);
        return;
    }
    let w = inner.width as usize;
    let lines: Vec<Line> = ex
        .into_iter()
        .map(|(name, dest, inferred)| {
            let left = fmt::truncate(&name, w / 3);
            let mark = if inferred { " ≈" } else { "" };
            let rest = w.saturating_sub(left.chars().count() + 4 + mark.len());
            Line::from(vec![
                Span::raw(format!("{left} → ")),
                Span::styled(fmt::truncate_start(&dest, rest), theme::accent()),
                Span::styled(mark, theme::dim()),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

fn render_folders(o: &OrganizeState, f: &mut Frame, area: Rect) {
    let block = theme::panel("Folders that will be made", false);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let Some(p) = &o.plan else {
        return;
    };
    let lines: Vec<Line> = p
        .plan
        .destination_tree()
        .glimpse(inner.height as usize, 8)
        .into_iter()
        .map(|l| {
            let tail = format!(
                "  {} files ({})",
                fmt::thousands(l.files),
                fmt::bytes(l.bytes)
            );
            let indent = "  ".repeat(l.depth);
            let room = (inner.width as usize).saturating_sub(indent.len() + tail.chars().count());
            let text = format!("{indent}{}{tail}", fmt::truncate_start(&l.label, room));
            if l.is_more {
                Line::styled(text, theme::dim())
            } else {
                Line::raw(text)
            }
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}
