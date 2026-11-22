//! Duplicates: groups (left), the copies of the selected group (right), and a summary on top.
//! Each group carries its tier: identical files, or a guess (likely, similar, same name) that
//! shows a comparison of its files and is only ever marked by hand. Marks are a proposal only.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use yojaka_core::dupes::{DupFile, DupGroup, Tier};

use crate::app::App;
use crate::dupes::{DupesState, Pane, Phase, tier_name};
use crate::screens::dashboard::{SPINNER, indeterminate, ratio_bar};
use crate::view::dupes_layout;
use crate::{fmt, theme};

/// Width of the "match" column in the group list (`same name`).
const TIER_W: usize = 9;

pub fn render(app: &App, f: &mut Frame, area: Rect) {
    let l = dupes_layout(area);
    render_summary(app, f, l.summary);
    render_groups(app, f, l.groups);
    render_copies(app, f, l.copies);
}

/// Copies marked for the Trash and the bytes they free, over the groups that match `which`.
fn marked_in(d: &DupesState, which: impl Fn(&DupGroup) -> bool) -> (usize, u64) {
    d.groups
        .iter()
        .filter(|g| which(g))
        .fold((0, 0), |(n, b), g| {
            let files = g
                .members
                .iter()
                .zip(&g.delete)
                .filter(|(_, d)| **d)
                .map(|(m, _)| m.files.len())
                .sum::<usize>();
            (n + files, b + g.reclaim())
        })
}

/// `Identical   68 groups · …`: a yellow label, then the facts.
fn summary_line(label: &str, groups: usize, rest: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<10}"), theme::title()),
        Span::raw(format!(
            "{:>5} group{} · {rest}",
            fmt::thousands(groups as u64),
            if groups == 1 { "" } else { "s" }
        )),
    ])
}

fn render_summary(app: &App, f: &mut Frame, area: Rect) {
    let d = &app.dupes;
    let mut title = String::from("Duplicates");
    for (tier, n) in d.tier_counts() {
        title.push_str(&format!(
            " · {} {}",
            fmt::thousands(n as u64),
            tier_name(tier)
        ));
    }
    if d.filter.is_some() {
        title.push_str(&format!(" · showing {}", d.filter_label()));
    }
    let block = theme::panel(title, false);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let w = inner.width as usize;
    let lines: Vec<Line> = match &d.phase {
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
            vec![Line::from(spans)]
        }
        Phase::Done if d.groups.is_empty() => {
            let n = d.stats.as_ref().map_or(0, |s| s.candidate_files);
            vec![Line::styled(
                format!(
                    "No duplicates: {} files share a size, none are identical or likely copies.",
                    fmt::thousands(n)
                ),
                theme::good(),
            )]
        }
        Phase::Done => done_lines(d),
        Phase::Cancelled => vec![Line::styled(
            "Search stopped. Press f to start again.",
            theme::dim(),
        )],
        Phase::Failed(e) => vec![Line::styled(fmt::truncate(e, w), theme::error())],
        Phase::Idle if app.dashboard.no_index => vec![Line::styled(
            "This folder has not been scanned yet.",
            theme::dim(),
        )],
        Phase::Idle => vec![
            Line::from(vec![
                Span::styled("f", theme::key_hint()),
                Span::styled(" finds identical files and likely copies. ", theme::dim()),
                Span::styled("F", theme::key_hint()),
                Span::styled(" also compares how pictures look.", theme::dim()),
            ]),
            Line::styled(
                "Nothing goes to the Trash until you review it.",
                theme::dim(),
            ),
        ],
    };
    f.render_widget(Paragraph::new(lines), inner);
}

/// The two summary lines once the search is done: identical groups (marked by the keep rule),
/// then the groups a person decides one by one.
fn done_lines(d: &DupesState) -> Vec<Line<'static>> {
    let exact = d.groups.iter().filter(|g| g.tier.is_exact()).count();
    let others = d.groups.len() - exact;
    let mut lines = vec![];
    if exact > 0 {
        let (n, bytes) = marked_in(d, |g| g.tier.is_exact());
        lines.push(summary_line(
            "Identical",
            exact,
            format!(
                "{} to the Trash · frees {} · keep rule: {}",
                fmt::files(n as u64),
                fmt::bytes(bytes),
                d.preset_label()
            ),
        ));
    }
    if others > 0 {
        let undecided = d.undecided();
        let (n, bytes) = marked_in(d, |g| !g.tier.is_exact());
        let mut rest = if undecided > 0 {
            format!(
                "{} left to decide, one by one",
                fmt::thousands(undecided as u64)
            )
        } else {
            "all decided".to_string()
        };
        if n > 0 {
            rest.push_str(&format!(
                " · {} to the Trash · frees {}",
                fmt::files(n as u64),
                fmt::bytes(bytes)
            ));
        }
        lines.push(summary_line("To decide", others, rest));
    }
    if d.issues > 0
        && let Some(last) = lines.last_mut()
    {
        last.spans.push(Span::styled(
            format!(" · {} skipped", d.issues),
            theme::error(),
        ));
    }
    lines
}

/// First row to show so that `selected` stays visible, roughly centered.
fn window(selected: usize, len: usize, rows: usize) -> usize {
    if len <= rows {
        0
    } else {
        selected.saturating_sub(rows / 2).min(len - rows)
    }
}

/// The tier of a group as shown in the list: identical in the plain colour, guesses in the
/// accent colour, so the groups to decide by hand stand out.
fn badge(tier: Tier) -> (String, Style) {
    let style = if tier.is_exact() {
        Style::new()
    } else {
        theme::accent()
    };
    (format!("{:<TIER_W$}", tier_name(tier)), style)
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
    let width = inner.width as usize;
    let header = format!(
        " {:>4} {:<TIER_W$}{:>6} {:>8}",
        "#", "match", "copies", "frees"
    );
    let mut lines = vec![Line::styled(fmt::truncate(&header, width), theme::dim())];
    let rows = (inner.height as usize).saturating_sub(1);
    let top = window(at, visible.len(), rows);
    lines.extend(visible.iter().skip(top).take(rows).map(|&i| {
        let g = &d.groups[i];
        let (tier, tier_style) = badge(g.tier);
        let chosen = i == d.group;
        let head = format!("{}{:>4} ", if chosen { "▶" } else { " " }, g.id);
        let tail = format!(
            "{:>6} {:>8}{}",
            format!("{}×", g.members.len()),
            fmt::bytes(g.reclaim()),
            if g.members.iter().any(|m| m.is_pair()) {
                " ◉"
            } else {
                ""
            }
        );
        if chosen {
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
    }));
    f.render_widget(Paragraph::new(lines), inner);
}

/// The folder part all `paths` share (`a/b/` for `a/b/x.jpg` and `a/b/c/y.jpg`), ending in `/`;
/// empty when they share none.
fn shared_prefix(paths: &[String]) -> String {
    let Some(first) = paths.first() else {
        return String::new();
    };
    let mut parts: Vec<&str> = first.split('/').collect();
    parts.pop(); // the file name
    for p in &paths[1..] {
        let other: Vec<&str> = p.split('/').collect();
        let same = parts
            .iter()
            .zip(&other[..other.len().saturating_sub(1)])
            .take_while(|(a, b)| a == b)
            .count();
        parts.truncate(same);
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("{}/", parts.join("/"))
    }
}

/// `▶ ✕ trash  2023/Goa/IMG_0001 (1).jpg      4.2 MB`: the cursor, what happens to the copy,
/// its path inside the opened folder (the part every copy shares is dim), and its size.
fn copy_line(
    g: &DupGroup,
    i: usize,
    chosen: bool,
    width: usize,
    paths: &[String],
) -> Line<'static> {
    let m = &g.members[i];
    let (tag, style) = if g.delete[i] {
        ("✕ trash  ", theme::error())
    } else if !g.tier.is_exact() && g.delete_count() == 0 {
        // A guess that nobody has decided yet: nothing is kept "instead of" anything.
        ("? decide ", theme::accent())
    } else {
        ("✓ keep   ", theme::good())
    };
    let size = format!(" {:>9}", fmt::bytes(m.total_size()));
    let suffix = if m.is_pair() { " +video" } else { "" };
    let mark = if chosen { "▶ " } else { "  " };
    let path_w = width.saturating_sub(
        mark.chars().count() + tag.chars().count() + size.chars().count() + suffix.len(),
    );
    let shared = shared_prefix(paths);
    let full = &paths[i];
    let shown = fmt::truncate_start(full, path_w);
    let pad = " ".repeat(path_w.saturating_sub(shown.chars().count()));
    let mut spans = vec![Span::styled(mark, theme::title()), Span::styled(tag, style)];
    match (shown == *full, full.strip_prefix(&shared)) {
        (true, Some(rest)) if !shared.is_empty() => {
            spans.push(Span::styled(shared.clone(), theme::dim()));
            spans.push(Span::raw(rest.to_string()));
        }
        _ => spans.push(Span::raw(shown)),
    }
    spans.push(Span::raw(pad));
    spans.push(Span::styled(suffix, theme::dim()));
    spans.push(Span::raw(size));
    Line::from(spans)
}

/// The facts that tell the files of a group apart, one column each: (label, width). The last
/// column takes what is left. Identical copies differ only in where they are and when they were
/// last modified, which is what the keep rules look at.
fn compare_columns(g: &DupGroup) -> Vec<(&'static str, usize)> {
    if g.tier.is_exact() {
        return vec![
            ("modified", 18),
            ("taken", 21),
            ("pixels", 11),
            ("camera", 0),
        ];
    }
    let mut cols = vec![("size", 10), ("pixels", 11), ("taken", 20)];
    // Photos have no length: the column is only there when a file has one.
    if g.members.iter().any(|m| m.primary().duration_ms.is_some()) {
        cols.push(("length", 8));
    }
    cols.push(("camera", 0));
    cols
}

fn compare_cell(f: &DupFile, column: &str, tz: Option<time::UtcOffset>) -> String {
    let dash = || "—".to_string();
    match column {
        "size" => fmt::bytes(f.size),
        "pixels" => f.pixels_text().unwrap_or_else(dash),
        "taken" => f.taken_text().unwrap_or_else(dash),
        "modified" => {
            yojaka_core::meta::format_run_time_at(f.mtime_ns.div_euclid(1_000_000_000), tz)
        }
        "length" => f
            .duration_ms
            .map_or_else(dash, |ms| fmt::clock(ms.max(0) as u64)),
        _ => f.camera.clone().unwrap_or_else(dash),
    }
}

/// One row of the comparison: the facts of member `i`, with those that differ from the
/// selected copy standing out.
fn compare_line(
    g: &DupGroup,
    i: usize,
    selected: usize,
    width: usize,
    tz: Option<time::UtcOffset>,
) -> Line<'static> {
    let (this, base) = (
        g.members[i].primary(),
        g.members[selected.min(g.members.len() - 1)].primary(),
    );
    let mut spans = vec![Span::styled(
        if i == selected { "▶ " } else { "  " },
        theme::title(),
    )];
    let mut used = 2usize;
    for (label, w) in compare_columns(g) {
        let w = if w == 0 {
            width.saturating_sub(used)
        } else {
            w.min(width.saturating_sub(used))
        };
        let cell = compare_cell(this, label, tz);
        let text = fmt::truncate(&cell, w.saturating_sub(1));
        let style = if cell != compare_cell(base, label, tz) {
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

fn render_copies(app: &App, f: &mut Frame, area: Rect) {
    let d = &app.dupes;
    let focused = d.pane == Pane::Copies;
    let Some(g) = d.current() else {
        let block = theme::panel("Copies", focused);
        f.render_widget(block, area);
        return;
    };
    let title = if g.tier.is_exact() {
        format!(
            "Group {} · {} identical copies{}",
            g.id,
            g.members.len(),
            if g.members.iter().any(|m| m.is_pair()) {
                " · Live Photos (photo and video move together)"
            } else {
                ""
            }
        )
    } else {
        format!(
            "Group {} · {} · {}",
            g.id,
            tier_name(g.tier),
            g.tier.reason()
        )
    };
    let block = theme::panel(title, focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let width = inner.width as usize;
    let height = inner.height as usize;
    let paths: Vec<String> = g
        .members
        .iter()
        .map(|m| fmt::rel(&m.primary().path, &app.root))
        .collect();
    // The files are shown twice: the list, then their facts side by side. The last two lines
    // are a gap and the whole path of the selected copy, which the list may have cut.
    let compare = height >= 7;
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
            let line = copy_line(g, i, i == d.copy, width, &paths);
            if i == d.copy {
                let style = if focused {
                    theme::selected()
                } else {
                    theme::selected_dim()
                };
                Line::from(
                    line.spans
                        .into_iter()
                        .map(|s| Span::styled(s.content, style))
                        .collect::<Vec<_>>(),
                )
            } else {
                line
            }
        })
        .collect();
    if compare {
        lines.push(Line::raw(""));
        lines.push(compare_header(g, width));
        lines.extend(shown.map(|i| compare_line(g, i, d.copy, width, app.tz)));
    }
    while lines.len() < height.saturating_sub(2) {
        lines.push(Line::raw(""));
    }
    if height >= 2
        && let Some(m) = g.members.get(d.copy)
    {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            fmt::truncate_start(&fmt::home(&m.primary().path), width),
            theme::dim(),
        ));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use super::shared_prefix;

    #[test]
    fn the_shared_folder_ends_at_a_folder() {
        let p = |v: &[&str]| shared_prefix(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(p(&["a/b/x.jpg", "a/b/c/y.jpg"]), "a/b/");
        assert_eq!(p(&["a/bc/x.jpg", "a/bd/x.jpg"]), "a/");
        assert_eq!(p(&["x.jpg", "y.jpg"]), "");
        assert_eq!(p(&["a/x.jpg"]), "a/");
        assert_eq!(p(&[]), "");
    }
}
