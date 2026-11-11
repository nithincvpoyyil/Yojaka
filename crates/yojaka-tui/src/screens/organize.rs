//! Organize: a short form (layout, pattern, destination, copy or move, files without GPS), a
//! preview that follows it, then review and run.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use yojaka_core::plan::OpMode;

use crate::app::App;
use crate::organize::{Editing, OrganizeState, Phase, ROWS, Row, STOPPED};
use crate::screens::dashboard::SPINNER;
use crate::{fmt, theme};

/// Width of the label column: the cursor mark, the longest label, a gap.
const LABEL_W: usize = 15;
/// Text boxes stop here on wide terminals, so they read as fields, not bars.
const BOX_MAX_W: usize = 72;
/// Example lines under the folder overview.
const EXAMPLES: usize = 3;
/// The folder overview's counts end at this column on wide terminals.
const TREE_W: usize = 72;

pub fn render(app: &App, f: &mut Frame, area: Rect) {
    // The rows, one line for a problem, and the frame.
    let [settings, preview] = Layout::vertical([
        Constraint::Length(ROWS.len() as u16 + 3),
        Constraint::Min(3),
    ])
    .areas(area);
    render_settings(&app.organize, f, settings);
    render_preview(app, f, preview);
}

fn render_settings(o: &OrganizeState, f: &mut Frame, area: Rect) {
    let block = theme::panel("Organize", true);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let width = inner.width as usize;
    let mut lines: Vec<Line> = ROWS
        .iter()
        .enumerate()
        .map(|(i, row)| setting(o, *row, i == o.cursor, width))
        .collect();
    if let Some(p) = &o.problem {
        lines.push(Line::styled(p.clone(), theme::error()));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// One row: the label (highlighted when selected), then a text box or a choice.
fn setting(o: &OrganizeState, row: Row, selected: bool, width: usize) -> Line<'static> {
    let mark = if selected { "▶ " } else { "  " };
    let label = format!("{mark}{:<w$}", row.label(), w = LABEL_W - 3);
    let mut spans = vec![
        Span::styled(
            label,
            if selected {
                theme::selected()
            } else {
                theme::dim()
            },
        ),
        Span::raw(" "),
    ];
    let room = width.saturating_sub(LABEL_W).min(BOX_MAX_W);
    match row {
        Row::Layout => spans.push(choice(o.preset_label())),
        Row::Pattern => spans.extend(text_box(
            &o.template,
            o.editing == Some(Editing::Template),
            room,
        )),
        Row::Dest => spans.extend(text_box(&o.dest, o.editing == Some(Editing::Dest), room)),
        Row::Action => {
            let (word, note) = match o.transfer {
                yojaka_core::organize::Transfer::Copy => {
                    ("Copy", "the originals stay where they are")
                }
                yojaka_core::organize::Transfer::Move => {
                    ("Move", "the originals go to the new folders")
                }
            };
            spans.push(choice(word));
            spans.push(Span::styled(format!("  {note}"), theme::dim()));
        }
        Row::NoGps => {
            let (word, note) = if o.infer {
                (
                    "Guess the place",
                    "from photos the same camera took around then (≈)",
                )
            } else {
                ("No Location folder", "they go into one folder")
            };
            let note = if o.uses_place() {
                note
            } else {
                "not used: this pattern has no place folders"
            };
            spans.push(choice(word));
            spans.push(Span::styled(format!("  {note}"), theme::dim()));
        }
    }
    Line::from(spans)
}

fn choice(word: &str) -> Span<'static> {
    Span::styled(format!("‹ {word} ›"), theme::choice())
}

/// A shaded box `room` wide, so it reads as a field you can type in. While typing it is
/// brighter and shows the cursor. A long value shows its end, where you type.
fn text_box(text: &str, editing: bool, room: usize) -> Vec<Span<'static>> {
    // A space on each side inside the box.
    let inner = room.saturating_sub(2).max(1);
    if editing {
        let shown = fmt::truncate_start(text, inner.saturating_sub(1));
        let pad = inner.saturating_sub(shown.chars().count() + 1);
        vec![
            Span::styled(format!(" {shown}"), theme::input_editing()),
            Span::styled("▏", theme::cursor()),
            Span::styled(" ".repeat(pad + 1), theme::input_editing()),
        ]
    } else {
        let shown = fmt::truncate_start(text, inner);
        let pad = inner.saturating_sub(shown.chars().count());
        vec![Span::styled(
            format!(" {shown}{} ", " ".repeat(pad)),
            theme::input(),
        )]
    }
}

fn render_preview(app: &App, f: &mut Frame, area: Rect) {
    let o = &app.organize;
    let block = theme::panel("Preview", false);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let note = |text: &str| Line::styled(text.to_string(), theme::dim());
    let lines: Vec<Line> = if o.problem.is_some() {
        vec![note("Fix the setting above to see where the files go.")]
    } else {
        match &o.phase {
            Phase::Idle if o.editing.is_some() => {
                vec![note("The preview changes when you press Enter.")]
            }
            Phase::Idle => vec![note(app.plan_blocker().unwrap_or(""))],
            Phase::Planning => {
                let spin = SPINNER[app.ticks as usize % SPINNER.len()];
                vec![Line::styled(
                    format!("{spin} Working out where each file goes…  Esc stops"),
                    theme::title(),
                )]
            }
            Phase::Failed(e) if e == STOPPED => {
                vec![note("Stopped. Change a setting to work it out again.")]
            }
            Phase::Failed(e) => vec![Line::styled(format!("Could not plan: {e}"), theme::error())],
            Phase::Ready => ready(o, inner),
        }
    };
    f.render_widget(Paragraph::new(lines), inner);
}

/// The summary, the folders that will be made, and a few examples.
fn ready(o: &OrganizeState, inner: Rect) -> Vec<Line<'static>> {
    let Some(p) = &o.plan else {
        return vec![];
    };
    let n = p.plan.file_ops();
    if n == 0 {
        return vec![Line::styled(
            "Nothing to do: every file is already in place.",
            theme::dim(),
        )];
    }
    let s = &p.plan.summary;
    let verb = if p.plan.mode == OpMode::Move {
        "moved"
    } else {
        "copied"
    };
    let mut head = vec![Span::styled(
        format!(
            "{} ({}) will be {verb}",
            fmt::files(n as u64),
            fmt::bytes(s.bytes)
        ),
        theme::good(),
    )];
    let mut more: Vec<String> = Vec::new();
    if o.uses_place() {
        more.push(format!("{:.0}% have a place", p.stats.coverage() * 100.0));
    }
    if s.inferred > 0 {
        more.push(format!(
            "{} places guessed ≈",
            fmt::thousands(s.inferred as u64)
        ));
    }
    if s.undated > 0 {
        more.push(format!(
            "{} without a date",
            fmt::thousands(s.undated as u64)
        ));
    }
    if s.skipped > 0 {
        more.push(format!("{} left alone", fmt::thousands(s.skipped as u64)));
    }
    if s.cross_volume > 0 {
        more.push(format!(
            "{} to another disk",
            fmt::thousands(s.cross_volume as u64)
        ));
    }
    for m in more {
        head.push(Span::styled(format!(" · {m}"), theme::dim()));
    }

    let width = inner.width as usize;
    let height = inner.height as usize;
    let examples = o.examples(EXAMPLES);
    // A blank line and a heading above the examples, when there is room for them.
    let ex_rows = if height >= 12 && !examples.is_empty() {
        examples.len() + 2
    } else {
        0
    };
    let tree_rows = height.saturating_sub(3 + ex_rows).max(1);

    let mut lines = vec![
        Line::from(head),
        Line::raw(""),
        Line::styled("Folders", theme::dim()),
    ];
    // Counts end in one column, so they read as a table.
    let col = width.min(TREE_W);
    for l in p.plan.destination_tree().glimpse(tree_rows, 8) {
        let tail = format!("  {} ({})", fmt::files(l.files), fmt::bytes(l.bytes));
        let indent = "  ".repeat(l.depth + 1);
        let label_w = col.saturating_sub(indent.len() + tail.chars().count());
        let label = fmt::truncate_start(&l.label, label_w);
        let pad = " ".repeat(label_w.saturating_sub(label.chars().count()));
        let text = format!("{indent}{label}{pad}{tail}");
        lines.push(if l.is_more {
            Line::styled(text, theme::dim())
        } else {
            Line::raw(text)
        });
    }
    if ex_rows > 0 {
        lines.push(Line::raw(""));
        lines.push(Line::styled("For example", theme::dim()));
        for (name, dest, inferred) in examples {
            let left = fmt::truncate(&name, width / 3);
            let mark = if inferred { " ≈" } else { "" };
            let rest = width.saturating_sub(left.chars().count() + 5 + mark.len());
            lines.push(Line::from(vec![
                Span::raw(format!("  {left} → ")),
                Span::styled(fmt::truncate_start(&dest, rest), theme::accent()),
                Span::styled(mark, theme::dim()),
            ]));
        }
    }
    lines
}
