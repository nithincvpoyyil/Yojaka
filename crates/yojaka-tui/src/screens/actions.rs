//! Actions: a short list of jobs, then the chosen job as a column of steps, like a CI run
//! (plan `10-actions-plan.md` §10). A finished step keeps its result on one line, the current
//! step opens below its line, the steps after it wait in grey.
//!
//! Above the steps, the job's name and status (Running, Waiting for you, Done, Failed, with the
//! time so far) and its stages as boxes joined by arrows, coloured by state (§10.1).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use yojaka_core::actions::{ACTIONS, Action};
use yojaka_core::organize::Transfer;
use yojaka_core::plan::OpMode;

use crate::actions::{DestRow, JOBS, KEEP_CHOICES, Pipeline, Status, Step};
use crate::app::App;
use crate::modal::Review;
use crate::organize::{Editing, Phase};
use crate::screens::dashboard::{SPINNER, ratio_bar};
use crate::screens::modal::{review_folders, review_head};
use crate::screens::organize::{choice, text_box};
use crate::{fmt, theme};

/// Width of the step-name column (after the icon).
const LABEL_W: usize = 17;
/// Where the body of the current step starts.
const BODY: &str = "   │ ";
/// Text boxes stop here on wide terminals.
const BOX_MAX_W: usize = 64;
/// Progress bars are this wide.
const BAR_W: u16 = 28;
/// A stage box is at least this wide inside, so a percentage fits under a short name.
const STAGE_MIN_W: usize = 7;
/// Joins two stage boxes.
const ARROW: &str = "─▶";

pub fn render(app: &App, f: &mut Frame, area: Rect) {
    let lines = match &app.actions.pipeline {
        None => list_lines(app, area.width as usize),
        Some(p) => pipeline_lines(app, p, area.width as usize, area.height as usize),
    };
    f.render_widget(Paragraph::new(lines), area);
}

/// The jobs, each with what it does.
fn list_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let text_w = width.saturating_sub(2).max(10);
    let root = fmt::truncate_start(&app.root.display().to_string(), 40);
    let intro = format!(
        "Ready-made jobs for {root}. Each one scans the folder, shows what is in it, asks a few \
         questions, then runs with a progress bar. Nothing changes until you confirm."
    );
    let mut v: Vec<Line> = wrap(&intro, text_w)
        .into_iter()
        .map(|l| Line::raw(format!(" {l}")))
        .collect();
    for (i, job) in JOBS.iter().enumerate() {
        v.push(Line::raw(""));
        let chosen = i == app.actions.cursor;
        let title = format!(" {}{}", if chosen { "▶ " } else { "  " }, job.title());
        let pad = " ".repeat(text_w.min(36).saturating_sub(title.chars().count()));
        v.push(Line::styled(
            format!("{title}{pad}"),
            if chosen {
                theme::selected()
            } else {
                theme::title()
            },
        ));
        for l in wrap(job.description(), width.saturating_sub(6).max(10)) {
            v.push(Line::styled(format!("     {l}"), theme::dim()));
        }
        let flow: Vec<&str> = job.steps().iter().map(|s| s.label()).collect();
        v.push(Line::styled(
            format!(
                "     {}",
                fmt::truncate(&flow.join(" → "), width.saturating_sub(6))
            ),
            theme::accent(),
        ));
    }
    let scan = &app.dashboard.scan;
    if scan.running {
        v.push(Line::raw(""));
        v.push(Line::styled(
            format!(
                " {} Scanning this folder: a job you start now follows the scan.",
                spinner(app)
            ),
            theme::dim(),
        ));
    }
    v
}

/// The job's steps under its title and stage boxes, fitted to `height`. When room is short the
/// rule goes first, then the second result lines, then the boxes become one line, then the top
/// goes; Confirm's folder list shrinks before any of that.
fn pipeline_lines(app: &App, p: &Pipeline, width: usize, height: usize) -> Vec<Line<'static>> {
    let steps = p.job.steps();
    let open = steps
        .iter()
        .enumerate()
        .position(|(i, _)| p.status(i) == Status::Current)
        .or(p.finished.then_some(p.at))
        .or(p.failed.is_some().then_some(p.at));
    let seconds: usize = (0..steps.len())
        .map(|i| p.results[i].len().saturating_sub(1))
        .sum();
    let tail = tail_lines(p);
    let body_of = |room: usize| open.map_or_else(Vec::new, |_| body_lines(app, p, width, room));
    let natural = body_of(usize::MAX).len();
    // What the open step cannot do without: all of it, except Confirm's folder list.
    let need = match &app.actions.review {
        Some(r) if p.going(Step::Confirm) => natural.min(1 + confirm_tail(app, r).len()),
        _ => natural,
    };

    // The tops to try, richest first, each with or without the second result lines.
    let title = title_line(app, p, width);
    let chain = stage_chain(app, p);
    let mut boxed = vec![title.clone()];
    boxed.extend(stage_boxes(app, p, width).unwrap_or_else(|| vec![chain.clone()]));
    let mut ruled = boxed.clone();
    ruled.push(Line::styled(
        format!(" {}", "─".repeat(width.saturating_sub(2))),
        theme::dim(),
    ));
    let mut tops = vec![
        (ruled, true),
        (boxed.clone(), true),
        (boxed, false),
        (vec![title, chain], false),
        (vec![], false),
    ];
    let used =
        |top: &[Line], s: bool| top.len() + steps.len() + usize::from(s) * seconds + tail.len();
    let first = |tries: std::ops::Range<usize>, body: usize| {
        tries.into_iter().find(|&i| {
            let (t, s) = &tops[i];
            used(t, *s) + body <= height
        })
    };
    // The boxes stay from step to step: Confirm's folder list shrinks before they go.
    let pick = first(0..3, natural)
        .or_else(|| first(2..3, need))
        .or_else(|| first(3..5, natural))
        .or_else(|| first(3..5, need))
        .unwrap_or(tops.len() - 1);
    let (top, show_seconds) = tops.swap_remove(pick);
    let room = height.saturating_sub(used(&top, show_seconds));
    let mut body = body_of(room);
    body.truncate(room);

    let mut v = top;
    for (i, step) in steps.iter().enumerate() {
        v.push(step_line(app, p, i, *step, width));
        if show_seconds {
            for extra in p.results[i].iter().skip(1) {
                v.push(Line::raw(format!("{}{extra}", " ".repeat(3 + LABEL_W))));
            }
        }
        if Some(i) == open {
            v.append(&mut body);
        }
    }
    v.extend(tail);
    v
}

/// `✔ Scan            12,871 files · nothing changed            0:06`
fn step_line(app: &App, p: &Pipeline, i: usize, step: Step, width: usize) -> Line<'static> {
    let status = p.status(i);
    let (icon, icon_style) = icon(app, p, i);
    let (text, text_style) = match status {
        Status::Done => (
            p.results[i].first().cloned().unwrap_or_default(),
            theme::base(),
        ),
        Status::Failed => (p.failed.clone().unwrap_or_default(), theme::error()),
        Status::Current => (current_text(app, p, step), theme::dim()),
        Status::Waiting | Status::Skipped => (String::new(), theme::dim()),
    };
    let took = match (status, step, p.took[i]) {
        (Status::Done, Step::Scan | Step::FindIdentical | Step::Run, Some(d)) => {
            fmt::clock(d.as_millis() as u64)
        }
        _ => String::new(),
    };
    let lead = 3 + LABEL_W;
    let room = width.saturating_sub(lead + 1 + took.chars().count() + 2);
    let text = fmt::truncate(&text, room);
    let gap = width
        .saturating_sub(lead + text.chars().count() + took.chars().count() + 1)
        .max(1);
    let mut spans = vec![
        Span::raw(" "),
        Span::styled(icon, icon_style),
        Span::raw(" "),
        Span::styled(
            format!("{:<w$}", step.label(), w = LABEL_W),
            label_style(status),
        ),
        Span::styled(text, text_style),
    ];
    if !took.is_empty() {
        spans.push(Span::raw(" ".repeat(gap)));
        spans.push(Span::styled(took, theme::dim()));
    }
    Line::from(spans)
}

/// Step `i`'s icon: `✔` done, `●` waits for a choice, a spinner while it works, `✖` failed,
/// `○` still to come, `–` not needed.
fn icon(app: &App, p: &Pipeline, i: usize) -> (String, Style) {
    let status = p.status(i);
    let icon = match status {
        Status::Done => "✔".to_string(),
        Status::Failed => "✖".to_string(),
        Status::Waiting => "○".to_string(),
        Status::Skipped => "–".to_string(),
        Status::Current if waits_for_you(app, p) => "●".to_string(),
        Status::Current => spinner(app).to_string(),
    };
    let style = match status {
        Status::Done => theme::good(),
        Status::Failed => theme::error(),
        Status::Current => theme::accent(),
        Status::Waiting | Status::Skipped => theme::dim(),
    };
    (icon, style)
}

fn label_style(status: Status) -> Style {
    match status {
        Status::Waiting | Status::Skipped => theme::dim(),
        Status::Current => theme::title(),
        Status::Done | Status::Failed => theme::base(),
    }
}

/// The current step waits for a choice (Confirm while it works out the plan does not).
fn waits_for_you(app: &App, p: &Pipeline) -> bool {
    p.going(p.step()) && p.step().asks() && !app.actions.wants_plan()
}

/// ` Organize into folders · ~/Pictures                    ● Waiting for you · 0:41`
fn title_line(app: &App, p: &Pipeline, width: usize) -> Line<'static> {
    let clock = fmt::clock(p.elapsed().as_millis() as u64);
    let (status, style) = if p.failed.is_some() {
        ("✖ Failed".to_string(), theme::error())
    } else if p.finished {
        ("✔ Done".to_string(), theme::good())
    } else if waits_for_you(app, p) {
        ("● Waiting for you".to_string(), theme::accent())
    } else {
        (format!("{} Running", spinner(app)), theme::accent())
    };
    let badge = format!("{status} · {clock}");
    let mut name = format!(" {}", p.job.title());
    // The folder fills what the name and the badge leave; on a narrow screen it goes.
    let room = width.saturating_sub(name.chars().count() + 3 + badge.chars().count() + 2);
    if room >= 8 {
        name.push_str(" · ");
        name.push_str(&fmt::truncate_start(&app.root.display().to_string(), room));
    }
    let gap = width
        .saturating_sub(name.chars().count() + badge.chars().count() + 1)
        .max(1);
    Line::from(vec![
        Span::styled(name, theme::title()),
        Span::raw(" ".repeat(gap)),
        Span::styled(badge, style),
    ])
}

/// The job's stages as boxes joined by arrows, each coloured by its state; the step at work
/// shows how far it has got on its lower edge:
///
/// ```text
///  ╭───────╮  ╭─────────╮  ╭─────────────╮
///  │✔ Scan │─▶│✔ Summary│─▶│⠹ Find       │
///  ╰───────╯  ╰─────────╯  ╰─────42%─────╯
/// ```
///
/// With room, the names get a space either side; without, the long ones are shortened
/// (`Find`, `Keep`). `None` when even that is too wide.
fn stage_boxes(app: &App, p: &Pipeline, width: usize) -> Option<Vec<Line<'static>>> {
    let steps = p.job.steps();
    let name = |s: Step, short: bool| if short { s.short_label() } else { s.label() };
    let inner = |s: Step, short: bool, pad: usize| {
        (name(s, short).chars().count() + 2).max(STAGE_MIN_W) + 2 * pad
    };
    let (short, pad) = [(false, 1), (false, 0), (true, 0)]
        .into_iter()
        .find(|&(short, pad)| {
            let boxes: usize = steps.iter().map(|&s| inner(s, short, pad) + 2).sum();
            1 + boxes + (steps.len() - 1) * ARROW.chars().count() <= width
        })?;
    let ratio = step_ratio(app, p);
    let gap = " ".repeat(ARROW.chars().count());
    let (mut top, mut mid, mut bot) = (
        vec![Span::raw(" ")],
        vec![Span::raw(" ")],
        vec![Span::raw(" ")],
    );
    for (i, &step) in steps.iter().enumerate() {
        let status = p.status(i);
        if i > 0 {
            let lit = p.status(i - 1) == Status::Done;
            top.push(Span::raw(gap.clone()));
            mid.push(Span::styled(
                ARROW,
                if lit { theme::good() } else { theme::dim() },
            ));
            bot.push(Span::raw(gap.clone()));
        }
        let edge = match status {
            Status::Current => theme::border(true),
            _ => icon(app, p, i).1,
        };
        let w = inner(step, short, pad);
        let label = name(step, short);
        let (glyph, glyph_style) = icon(app, p, i);
        top.push(Span::styled(format!("╭{}╮", "─".repeat(w)), edge));
        mid.extend([
            Span::styled("│", edge),
            Span::raw(" ".repeat(pad)),
            Span::styled(glyph, glyph_style),
            Span::raw(" "),
            Span::styled(label, label_style(status)),
            Span::raw(" ".repeat(w - pad - 2 - label.chars().count())),
            Span::styled("│", edge),
        ]);
        let lower = match ratio {
            Some(r) if status == Status::Current => {
                let pct = format!("{:.0}%", r * 100.0);
                let left = w.saturating_sub(pct.chars().count()) / 2;
                let right = w.saturating_sub(left + pct.chars().count());
                format!("{}{pct}{}", "─".repeat(left), "─".repeat(right))
            }
            _ => "─".repeat(w),
        };
        bot.push(Span::styled(format!("╰{lower}╯"), edge));
    }
    Some(vec![Line::from(top), Line::from(mid), Line::from(bot)])
}

/// The stages on one line, for a screen too narrow or too short for the boxes:
/// ` ✔─✔─⠹─○─○─○  Find identical`.
fn stage_chain(app: &App, p: &Pipeline) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for i in 0..p.job.steps().len() {
        if i > 0 {
            let lit = p.status(i - 1) == Status::Done;
            spans.push(Span::styled(
                "─",
                if lit { theme::good() } else { theme::dim() },
            ));
        }
        let (glyph, style) = icon(app, p, i);
        spans.push(Span::styled(glyph, style));
    }
    spans.push(Span::styled(
        format!("  {}", p.step().label()),
        label_style(p.status(p.at)),
    ));
    Line::from(spans)
}

/// How far the step at work has got, when it can tell.
fn step_ratio(app: &App, p: &Pipeline) -> Option<f64> {
    if !p.going(p.step()) {
        return None;
    }
    match p.step() {
        Step::Scan => {
            let s = &app.dashboard.scan;
            if s.running && s.step() == 2 {
                s.ratio()
            } else {
                None
            }
        }
        Step::FindIdentical => app.dupes.ratio(),
        Step::Confirm if app.actions.review.is_none() => {
            match (&app.actions.org.phase, app.plan_progress) {
                (Phase::Planning, Some((done, Some(total)))) if total > 0 => {
                    Some(done as f64 / total as f64)
                }
                _ => None,
            }
        }
        Step::Run => match p.run.total {
            Some(t) if t > 0 => Some(p.run.done as f64 / t as f64),
            _ => None,
        },
        _ => None,
    }
}

/// What the current step is doing or asking, on its own line.
fn current_text(app: &App, p: &Pipeline, step: Step) -> String {
    let a = &app.actions;
    match step {
        Step::Scan => {
            let s = &app.dashboard.scan;
            if !s.running {
                "starting…".into()
            } else if s.cancelling {
                "stopping…".into()
            } else if s.step() == 1 {
                format!("finding files… {} so far", fmt::thousands(s.done))
            } else {
                match s.ratio() {
                    Some(r) => format!("reading details… {:.0}%", r * 100.0),
                    None => "reading details…".into(),
                }
            }
        }
        Step::Summary => "reading the summary…".into(),
        Step::FindIdentical => {
            let d = &app.dupes;
            match d.total {
                Some(t) if t > 0 => format!(
                    "comparing files… {} of {}",
                    fmt::thousands(d.done),
                    fmt::thousands(t)
                ),
                _ => "comparing files…".into(),
            }
        }
        Step::Layout => "how should the files be arranged?".into(),
        Step::Keep => "in each group of identical files, one copy stays".into(),
        Step::Destination => "where should the files go?".into(),
        Step::Confirm if a.review.is_some() => "check what will happen, then confirm".into(),
        Step::Confirm => match (&a.org.phase, app.plan_progress) {
            (Phase::Planning, Some((done, Some(total)))) if total > 0 => format!(
                "comparing files… {} of {}",
                fmt::thousands(done),
                fmt::thousands(total)
            ),
            (Phase::Planning, Some(_)) => "comparing files…".into(),
            _ => app
                .plan_blocker()
                .map_or("working out the plan…".into(), |b| b.to_lowercase()),
        },
        Step::Run => {
            let r = &p.run;
            let verb = match p.job {
                crate::actions::Job::Dupes => "moving to the Trash",
                crate::actions::Job::Organize if a.org.transfer == Transfer::Move => "moving",
                crate::actions::Job::Organize => "copying",
            };
            format!(
                "{verb}… {} of {}",
                fmt::thousands(r.done),
                r.total.map_or_else(|| "?".into(), fmt::thousands)
            )
        }
    }
}

/// The lines under the open step, using at most `room` rows (Confirm's folder list shrinks).
fn body_lines(app: &App, p: &Pipeline, width: usize, room: usize) -> Vec<Line<'static>> {
    let inner = width.saturating_sub(BODY.chars().count());
    let rows: Vec<Line<'static>> = if p.failed.is_some() {
        if p.finished {
            vec![]
        } else {
            vec![Line::styled("Esc goes back to change it.", theme::dim())]
        }
    } else if p.finished {
        // The run's report under the Run line.
        p.report
            .iter()
            .map(|l| {
                Line::styled(
                    l.text.clone(),
                    if l.bad { theme::error() } else { theme::dim() },
                )
            })
            .collect()
    } else {
        match p.step() {
            Step::Scan => scan_body(app),
            Step::FindIdentical => {
                let d = &app.dupes;
                d.ratio()
                    .map(|r| {
                        let mut l = ratio_bar(BAR_W, r, theme::bar());
                        l.spans.push(Span::styled(
                            format!("  {} read", fmt::bytes(d.bytes)),
                            theme::dim(),
                        ));
                        vec![l]
                    })
                    .unwrap_or_default()
            }
            Step::Layout => layout_body(app, inner),
            Step::Keep => keep_body(app),
            Step::Destination => destination_body(app, inner),
            Step::Confirm => confirm_body(app, inner, room),
            Step::Run => run_body(p),
            Step::Summary => vec![],
        }
    };
    rows.into_iter()
        .map(|l| {
            let mut spans = vec![Span::styled(BODY, theme::dim())];
            spans.extend(l.spans.into_iter().map(|s| {
                if s.style == ratatui::style::Style::default() {
                    Span::styled(s.content, l.style)
                } else {
                    s
                }
            }));
            Line::from(spans)
        })
        .collect()
}

fn scan_body(app: &App) -> Vec<Line<'static>> {
    let s = &app.dashboard.scan;
    match s.ratio() {
        Some(r) if s.running && s.step() == 2 => vec![ratio_bar(BAR_W, r, theme::bar())],
        _ => vec![],
    }
}

/// The six layouts with an example path each, then what the highlighted one does.
fn layout_body(app: &App, width: usize) -> Vec<Line<'static>> {
    let a = &app.actions;
    let mut v: Vec<Line> = ACTIONS
        .iter()
        .enumerate()
        .map(|(i, act)| {
            let chosen = i == a.layout;
            let name = format!("{}{:<24}", if chosen { "▶ " } else { "  " }, act.title);
            Line::from(vec![
                Span::styled(
                    name,
                    if chosen {
                        theme::selected()
                    } else {
                        theme::base()
                    },
                ),
                Span::styled(format!(" {}", act.examples[0]), theme::accent()),
            ])
        })
        .collect();
    let act: &Action = &ACTIONS[a.layout.min(ACTIONS.len() - 1)];
    let about = format!(
        "{} {}",
        act.description,
        Action::identical_note(a.org.transfer)
    );
    for l in wrap(&about, width.max(10)) {
        v.push(Line::styled(l, theme::dim()));
    }
    v
}

fn keep_body(app: &App) -> Vec<Line<'static>> {
    let mut v: Vec<Line> = KEEP_CHOICES
        .iter()
        .enumerate()
        .map(|(i, (name, what))| {
            let chosen = i == app.actions.keep;
            Line::from(vec![
                Span::styled(
                    format!("{}{name:<22}", if chosen { "▶ " } else { "  " }),
                    if chosen {
                        theme::selected()
                    } else {
                        theme::base()
                    },
                ),
                Span::styled(format!(" {what}"), theme::dim()),
            ])
        })
        .collect();
    v.push(Line::styled(
        "Only files with the same bytes. A Live Photo pair stays together.",
        theme::dim(),
    ));
    v
}

fn destination_body(app: &App, width: usize) -> Vec<Line<'static>> {
    let a = &app.actions;
    let o = &a.org;
    let label = |row: DestRow, text: &str| {
        let on = a.dest_row == row;
        Span::styled(
            format!("{}{text:<8}", if on { "▶ " } else { "  " }),
            if on { theme::selected() } else { theme::dim() },
        )
    };
    let room = width.saturating_sub(12).min(BOX_MAX_W);
    let mut folder = vec![label(DestRow::Folder, "Folder"), Span::raw(" ")];
    folder.extend(text_box(&o.dest, o.editing == Some(Editing::Dest), room));
    let (word, what) = match o.transfer {
        Transfer::Copy => ("Copy", "the originals stay where they are"),
        Transfer::Move => ("Move", "the originals go to the new folders"),
    };
    let mut v = vec![
        Line::from(folder),
        Line::from(vec![
            label(DestRow::Transfer, "Action"),
            // Lines up with the text inside the folder box.
            Span::raw("  "),
            choice(word),
            Span::styled(format!("  {what}"), theme::dim()),
        ]),
    ];
    if let Some(p) = &o.problem {
        v.push(Line::styled(p.clone(), theme::error()));
    }
    v
}

/// The review: what will happen, the folders, the no-undo note, then the answer it needs.
fn confirm_body(app: &App, width: usize, room: usize) -> Vec<Line<'static>> {
    let Some(r) = &app.actions.review else {
        return match (&app.actions.org.phase, app.plan_progress) {
            (Phase::Planning, Some((done, Some(total)))) if total > 0 => {
                vec![ratio_bar(BAR_W, done as f64 / total as f64, theme::bar())]
            }
            _ => vec![],
        };
    };
    let tail = confirm_tail(app, r);
    // A long destination wraps instead of being cut.
    let mut head: Vec<Line<'static>> = review_head(r)
        .into_iter()
        .flat_map(|l| wrap_line(l, width))
        .collect();
    // The folder list takes what is left. One row short, its heading gives up its row; with no
    // room for it at all, the heading goes too.
    let mut avail = room.saturating_sub(head.len() + tail.len());
    let mut heading = true;
    if avail == 1 {
        head.pop();
        heading = false;
        avail = 2;
    }
    let mut folders = if avail >= 2 {
        review_folders(r, avail, width)
    } else {
        vec![]
    };
    if folders.len() > avail {
        // The "… more lines" note needs a row of its own.
        folders = review_folders(r, avail - 1, width);
    }
    if folders.is_empty() && heading {
        head.pop();
    }
    let first = head.first().cloned();
    let mut v = head;
    v.extend(folders);
    v.extend(tail.iter().cloned());
    if v.len() > room {
        // Very short: what will happen, and the answer it needs.
        v = first.into_iter().chain(tail.iter().cloned()).collect();
        if v.len() > room {
            v = tail.into_iter().rev().take(room).rev().collect();
        }
    }
    v
}

/// The end of Confirm, always on screen: the no-undo note and the answer it needs.
fn confirm_tail(app: &App, r: &Review) -> Vec<Line<'static>> {
    let plan = &r.plan;
    let mut tail = vec![Line::styled(plan.no_undo_note().to_string(), theme::dim())];
    if app.mode != crate::app::Mode::Write {
        tail.push(Line::styled(
            "Write mode is off.  y / Enter  turn it on (asks first)      Esc  back",
            theme::accent(),
        ));
    } else if r.needs_typing() {
        tail.push(Line::raw(format!(
            "Type \"{}\" and press Enter:",
            plan.confirm_phrase()
        )));
        tail.push(Line::styled(format!("> {}▏", r.typed), theme::accent()));
    } else {
        let go = match plan.mode {
            OpMode::Copy => "y / Enter  copy      Esc  back",
            OpMode::Move => "y / Enter  move      Esc  back",
            _ => "y / Enter  move to the Trash      Esc  back",
        };
        tail.push(Line::styled(go, theme::accent()));
    }
    tail
}

fn run_body(p: &Pipeline) -> Vec<Line<'static>> {
    let r = &p.run;
    let ratio = match r.total {
        Some(t) if t > 0 => r.done as f64 / t as f64,
        _ => 0.0,
    };
    let mut bar = ratio_bar(BAR_W, ratio, theme::bar());
    bar.spans.push(Span::styled(
        format!("  {:.0}%", ratio * 100.0),
        theme::dim(),
    ));
    let mut about = Vec::new();
    if let Some(name) = r.current.as_deref().and_then(|c| c.file_name()) {
        about.push(name.to_string_lossy().into_owned());
    }
    about.push(fmt::bytes(r.bytes));
    let elapsed = p.since.elapsed();
    if let Some(t) = r.total
        && r.done > 0
        && r.done < t
        && elapsed.as_secs() >= 2
    {
        let left = elapsed.as_millis() as u64 * (t - r.done) / r.done;
        about.push(format!("{} left", fmt::clock(left)));
    }
    let hint = if r.cancelling {
        "stopping after the current file…"
    } else {
        "Ctrl-C stops after this file"
    };
    about.push(hint.to_string());
    vec![bar, Line::styled(about.join(" · "), theme::dim())]
}

/// Under the steps once the job is over: what now.
fn tail_lines(p: &Pipeline) -> Vec<Line<'static>> {
    if !p.finished {
        return vec![];
    }
    let mut v = vec![Line::raw("")];
    if p.failed.is_none() && p.step() == Step::Run {
        v.push(Line::styled(
            " Done. The folder is being scanned again.",
            theme::good(),
        ));
    }
    v.push(Line::styled(
        " Enter  back to the list      6  History",
        theme::accent(),
    ));
    v
}

/// The footer keys for the Actions tab (not while typing in a box).
pub fn footer(app: &App) -> Vec<(&'static str, &'static str)> {
    let a = &app.actions;
    let Some(p) = &a.pipeline else {
        return vec![
            ("q", "quit"),
            ("?", "help"),
            ("↑↓", "choose"),
            ("Enter", "start"),
            ("< >", "tabs"),
        ];
    };
    if p.finished {
        return vec![
            ("Enter", "back to the list"),
            ("6", "History"),
            ("q", "quit"),
        ];
    }
    if p.failed.is_some() {
        return vec![("Esc", "go back"), ("q", "quit")];
    }
    match p.step() {
        Step::Run => vec![("Ctrl-C", "stop after this file")],
        Step::Scan | Step::Summary | Step::FindIdentical => {
            vec![("Esc", "stop and go back"), ("q", "quit")]
        }
        Step::Layout | Step::Keep => vec![
            ("↑↓", "choose"),
            ("Enter", "next"),
            ("Esc", "back"),
            ("q", "quit"),
        ],
        Step::Destination if a.dest_row == DestRow::Folder => vec![
            ("↑↓", "choose"),
            ("Enter", "type the folder"),
            ("Esc", "back"),
        ],
        Step::Destination => vec![
            ("← →", "copy / move"),
            ("Enter", "next"),
            ("↑", "folder"),
            ("Esc", "back"),
        ],
        Step::Confirm => match &a.review {
            None => vec![("Esc", "back"), ("q", "quit")],
            Some(_) if app.mode != crate::app::Mode::Write => {
                vec![("y", "turn on write mode"), ("Esc", "back")]
            }
            Some(r) if r.needs_typing() => {
                vec![
                    ("Enter", "confirm (once the phrase matches)"),
                    ("Esc", "back"),
                ]
            }
            Some(_) => vec![("y", "go ahead"), ("Esc", "back")],
        },
    }
}

fn spinner(app: &App) -> char {
    SPINNER[app.ticks as usize % SPINNER.len()]
}

/// A one-span line wider than `width`, wrapped into several with the same style.
fn wrap_line(l: Line<'static>, width: usize) -> Vec<Line<'static>> {
    if l.width() <= width || l.spans.len() != 1 {
        return vec![l];
    }
    let style = l.style.patch(l.spans[0].style);
    wrap(&l.spans[0].content, width.max(10))
        .into_iter()
        .map(|t| Line::styled(t, style))
        .collect()
}

/// Word wrap to `width` columns.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::wrap;

    #[test]
    fn wrap_keeps_words_whole() {
        assert_eq!(
            wrap("one two three four", 9),
            vec!["one two", "three", "four"]
        );
        assert!(wrap("", 5).is_empty());
    }
}
