//! Duplicates: groups (left), the copies of the selected group (right), and a summary on top.
//! Each group carries its tier: identical files, or a guess (likely, similar, name) that shows a
//! comparison of its files and is only ever marked by hand. Marks are a proposal only.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use yojaka_core::dupes::{DupFile, DupGroup, Tier};

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
    let mut title = String::from("Duplicates");
    for (tier, n) in d.tier_counts() {
        title.push_str(&format!(" · {} {}", fmt::thousands(n as u64), tier.label()));
    }
    if d.filter.is_some() {
        title.push_str(&format!(" · showing {}", d.filter_label()));
    }
    let block = theme::panel(title, false);
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
                    "No duplicates: {} files share a size, none are identical or likely copies.",
                    fmt::thousands(n)
                ),
                theme::good(),
            )
        }
        Phase::Done => {
            let mut spans = vec![Span::styled(
                format!("{} groups", fmt::thousands(d.groups.len() as u64)),
                theme::title(),
            )];
            let undecided = d.undecided();
            if undecided > 0 {
                spans.push(Span::styled(
                    format!(" · {} to review", fmt::thousands(undecided as u64)),
                    theme::accent(),
                ));
            }
            spans.push(Span::raw(format!(
                " · {} files marked · frees {} · rule: {}",
                fmt::thousands(d.marked_files() as u64),
                fmt::bytes(d.total_reclaim()),
                d.preset_label()
            )));
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
            Span::styled(" finds duplicates. ", theme::dim()),
            Span::styled("F", theme::key_hint()),
            Span::styled(
                " also compares how pictures look. Nothing is deleted.",
                theme::dim(),
            ),
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

/// Seven columns: the tier of a group, as shown in the list.
fn badge(tier: Tier) -> (&'static str, Style) {
    match tier {
        Tier::Exact => ("exact  ", theme::good()),
        Tier::Likely => ("likely ", theme::accent()),
        Tier::Similar => ("similar", theme::accent()),
        Tier::Name => ("name   ", theme::accent()),
    }
}

fn render_groups(app: &App, f: &mut Frame, area: Rect) {
    let d = &app.dupes;
    let focused = d.pane == Pane::Groups;
    let visible = d.visible();
    let at = visible.iter().position(|&i| i == d.group).unwrap_or(0);
    let title = if visible.is_empty() {
        "Groups".to_string()
    } else {
        format!("Groups {}/{}", at + 1, visible.len())
    };
    let block = theme::panel(title, focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if visible.is_empty() {
        let msg = match d.phase {
            Phase::Searching => "searching…",
            Phase::Done => "nothing to review",
            _ => "no results yet",
        };
        f.render_widget(Paragraph::new(msg).style(theme::dim()), inner);
        return;
    }
    let rows = inner.height as usize;
    let width = inner.width as usize;
    let top = window(at, visible.len(), rows);
    let lines: Vec<Line> = visible
        .iter()
        .skip(top)
        .take(rows)
        .map(|&i| {
            let g = &d.groups[i];
            let (tier, tier_style) = badge(g.tier);
            let head = format!("{:>4} ", g.id);
            let tail = format!(
                " {:>2}× {:>9}{}",
                g.members.len(),
                fmt::bytes(g.reclaim()),
                if g.members.iter().any(|m| m.is_pair()) {
                    " ◉"
                } else {
                    ""
                }
            );
            if i == d.group {
                let style = if focused {
                    theme::selected()
                } else {
                    theme::selected_dim()
                };
                let text = fmt::truncate(&format!("{head}{tier}{tail}"), width);
                Line::styled(format!("{text:<width$}"), style)
            } else {
                let room = width.saturating_sub(head.chars().count() + tier.chars().count());
                Line::from(vec![
                    Span::raw(head),
                    Span::styled(tier, tier_style),
                    Span::raw(fmt::truncate(&tail, room)),
                ])
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
    } else if !g.tier.is_exact() && g.delete_count() == 0 {
        // A guess that nobody has decided yet: nothing is kept "instead of" anything.
        ("? review ", theme::accent())
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

/// The facts that tell look-alike files apart, one column each: (label, width). The last
/// column takes what is left.
fn compare_columns(g: &DupGroup) -> Vec<(&'static str, usize)> {
    let mut cols = vec![("size", 10), ("pixels", 11), ("taken", 20)];
    // Photos have no length: the column is only there when a file has one.
    if g.members.iter().any(|m| m.primary().duration_ms.is_some()) {
        cols.push(("length", 8));
    }
    cols.push(("camera", 0));
    cols
}

fn compare_cell(f: &DupFile, column: &str) -> String {
    let dash = || "—".to_string();
    match column {
        "size" => fmt::bytes(f.size),
        "pixels" => f.pixels_text().unwrap_or_else(dash),
        "taken" => f.taken_text().unwrap_or_else(dash),
        "length" => f
            .duration_ms
            .map_or_else(dash, |ms| fmt::clock(ms.max(0) as u64)),
        _ => f.camera.clone().unwrap_or_else(dash),
    }
}

/// One row of the comparison: the facts of member `i`, with those that differ from the
/// selected copy standing out.
fn compare_line(g: &DupGroup, i: usize, selected: usize, width: usize) -> Line<'static> {
    let (this, base) = (
        g.members[i].primary(),
        g.members[selected.min(g.members.len() - 1)].primary(),
    );
    let mut spans = vec![Span::styled(
        if i == selected { "▸ " } else { "  " },
        theme::title(),
    )];
    let mut used = 2usize;
    for (label, w) in compare_columns(g) {
        let w = if w == 0 {
            width.saturating_sub(used)
        } else {
            w.min(width.saturating_sub(used))
        };
        let cell = compare_cell(this, label);
        let text = fmt::truncate(&cell, w.saturating_sub(1));
        let style = if cell != compare_cell(base, label) {
            theme::accent()
        } else if i == selected {
            Style::new()
        } else {
            theme::dim()
        };
        spans.push(Span::styled(format!("{text:<w$}"), style));
        used += w;
    }
    Line::from(spans)
}

fn compare_header(g: &DupGroup, width: usize) -> Line<'static> {
    let mut text = String::from("  ");
    for (label, w) in compare_columns(g) {
        text.push_str(&format!("{label:<w$}"));
    }
    Line::styled(fmt::truncate(&text, width), theme::dim())
}

fn render_copies(d: &DupesState, f: &mut Frame, area: Rect) {
    let focused = d.pane == Pane::Copies;
    let Some(g) = d.current() else {
        let block = theme::panel("Copies", focused);
        f.render_widget(block, area);
        return;
    };
    let title = if g.tier.is_exact() {
        format!(
            "Group {} · {} copies{}",
            g.id,
            g.members.len(),
            if g.members.iter().any(|m| m.is_pair()) {
                " · Live Photos (photo and video move together)"
            } else {
                ""
            }
        )
    } else {
        format!("Group {} · {} · {}", g.id, g.tier.label(), g.tier.reason())
    };
    let block = theme::panel(title, focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let width = inner.width as usize;
    let height = inner.height as usize;
    // A guess shows its files twice: the list, then their facts side by side. The last two
    // lines are a gap and the whole path of the selected copy, which the list may have cut.
    let compare = !g.tier.is_exact() && height >= 7;
    let list_rows = if compare {
        (height.saturating_sub(4) / 2).clamp(1, g.members.len())
    } else {
        height.saturating_sub(2)
    };
    let top = window(d.copy, g.members.len(), list_rows.max(1));
    let shown = top..g.members.len().min(top + list_rows);
    let mut lines: Vec<Line> = shown
        .clone()
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
    if compare {
        lines.push(Line::raw(""));
        lines.push(compare_header(g, width));
        lines.extend(shown.map(|i| compare_line(g, i, d.copy, width)));
    }
    while lines.len() < height.saturating_sub(2) {
        lines.push(Line::raw(""));
    }
    if height >= 2
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
