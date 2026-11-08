//! Dashboard: live scan gauge, totals, and the format / year / camera charts.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use yojaka_core::TaskKind;
use yojaka_core::stats::Stats;

use crate::app::{App, ScanView};
use crate::{fmt, theme};

pub(crate) const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub fn render(app: &App, f: &mut Frame, area: Rect) {
    let [scan, totals, panels] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Length(4),
        Constraint::Min(4),
    ])
    .areas(area);
    render_scan(app, f, scan);
    render_totals(app, f, totals);
    render_panels(app.dashboard.stats.as_deref(), f, panels);
}

fn phase_name(s: &ScanView) -> &'static str {
    match s.phase {
        _ if s.cancelling => "Stopping",
        Some(TaskKind::Metadata) => "Reading details",
        Some(TaskKind::Geocode) => "Resolving places",
        _ => "Finding files",
    }
}

fn render_scan(app: &App, f: &mut Frame, area: Rect) {
    let d = &app.dashboard;
    let s = &d.scan;
    let block = theme::panel("Scan", false);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height < 2 {
        return;
    }
    let [line1, line2] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(inner);

    if let Some(e) = &d.error {
        f.render_widget(
            Paragraph::new(fmt::truncate(e, inner.width as usize)).style(theme::error()),
            line1,
        );
        f.render_widget(
            Paragraph::new("Open a folder with your photos, such as ~/Pictures.")
                .style(theme::dim()),
            line2,
        );
        return;
    }

    // line 1: bar + numbers
    let text = if s.running {
        let spin = SPINNER[(app.ticks as usize) % SPINNER.len()];
        let head = format!("{spin} Step {}/2 · {}", s.step(), phase_name(s));
        let mut t = match s.ratio() {
            Some(r) => format!("{head} {:.0}%", r * 100.0),
            None => format!("{head} · {} files", fmt::thousands(s.done)),
        };
        if let Some(eta) = s.eta() {
            t.push_str(&format!(" · ~{} left", fmt::span(eta.as_millis() as u64)));
        }
        t
    } else if d.stats.is_some() {
        let partial = s.partial
            || d.stats
                .as_ref()
                .and_then(|st| st.last_scan.as_ref())
                .is_some_and(|l| l.status == "partial");
        if partial {
            "Partial, r to finish".to_string()
        } else {
            "Up to date".to_string()
        }
    } else if d.no_index {
        "Not scanned yet".to_string()
    } else {
        "Loading…".to_string()
    };
    let text_w = (text.chars().count() as u16 + 1).min(inner.width.saturating_sub(10));
    let [bar, label] =
        Layout::horizontal([Constraint::Min(8), Constraint::Length(text_w)]).areas(line1);
    if s.running {
        match s.ratio() {
            Some(r) => f.render_widget(Paragraph::new(ratio_bar(bar.width, r, theme::bar())), bar),
            None => f.render_widget(Paragraph::new(indeterminate(bar.width, app.ticks)), bar),
        }
    } else {
        let full = if d.stats.is_some() { 1.0 } else { 0.0 };
        f.render_widget(
            Paragraph::new(ratio_bar(bar.width, full, theme::good())),
            bar,
        );
    }
    f.render_widget(
        Paragraph::new(format!(" {}", fmt::truncate(&text, text_w as usize))),
        label,
    );

    // line 2: what is being looked at, or what happened last
    let w = inner.width as usize;
    let line = if s.running {
        running_line(s, w)
    } else if let Some(o) = &s.outcome {
        Line::styled(fmt::truncate(o, w), theme::dim())
    } else if let Some(last) = d.stats.as_ref().and_then(|st| st.last_scan.as_ref()) {
        Line::styled(
            fmt::truncate(
                &format!(
                    "Last scan {}: {} files seen",
                    last.status,
                    fmt::thousands(last.files_seen)
                ),
                w,
            ),
            theme::dim(),
        )
    } else {
        Line::styled("Press r to scan this folder.", theme::dim())
    };
    f.render_widget(Paragraph::new(line), line2);
}

/// Second line while working: counts, speed, time spent and the folder; or a plain warning
/// when nothing has moved for a while.
fn running_line(s: &ScanView, w: usize) -> Line<'static> {
    let dir = s
        .current
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    if let Some(stuck) = s.stalled_for() {
        let msg = format!(
            "No progress for {}s. Still working in {} · Esc stops and keeps what is done",
            stuck.as_secs(),
            if dir.is_empty() { "…" } else { &dir }
        );
        return Line::styled(fmt::truncate(&msg, w), theme::error());
    }
    if s.finishing() {
        let mut t = "Finishing up".to_string();
        if let Some(e) = s.elapsed() {
            t.push_str(&format!(" · {} elapsed", fmt::clock(e.as_millis() as u64)));
        }
        return Line::styled(fmt::truncate(&t, w), theme::dim());
    }
    let tilde = if s.total_is_estimate() { "~" } else { "" };
    let mut parts = vec![match s.total {
        Some(t) => format!("{} of {tilde}{}", fmt::thousands(s.done), fmt::thousands(t)),
        None => format!("{} found so far", fmt::thousands(s.done)),
    }];
    if let Some(r) = s.rate.filter(|r| *r >= 1.0) {
        parts.push(format!("{}/s", fmt::thousands(r as u64)));
    }
    if let Some(e) = s.elapsed() {
        parts.push(format!("{} elapsed", fmt::clock(e.as_millis() as u64)));
    }
    let head = parts.join(" · ");
    let room = w.saturating_sub(head.chars().count() + 3);
    if dir.is_empty() || room < 12 {
        return Line::styled(fmt::truncate(&head, w), theme::dim());
    }
    Line::styled(
        format!("{head} · {}", fmt::truncate_start(&dir, room)),
        theme::dim(),
    )
}

/// A progress bar `width` cells wide, filled to `ratio`.
pub(crate) fn ratio_bar(width: u16, ratio: f64, fill: Style) -> Line<'static> {
    let width = width as usize;
    let done = fmt::bar((ratio.clamp(0.0, 1.0) * 1000.0) as u64, 1000, width);
    let pad = width.saturating_sub(done.chars().count());
    Line::from(vec![
        Span::styled(done, fill),
        Span::styled("░".repeat(pad), theme::dim()),
    ])
}

/// A bright block sliding along a dim track, for work whose size is not known yet.
pub(crate) fn indeterminate(width: u16, tick: u64) -> Line<'static> {
    let width = width as usize;
    if width < 6 {
        return Line::default();
    }
    let seg = (width / 5).max(3);
    let span = width - seg;
    // bounce: 0..span..0
    let t = (tick as usize * 2) % (span * 2).max(1);
    let pos = if t <= span { t } else { span * 2 - t };
    Line::from(vec![
        Span::styled("░".repeat(pos), theme::dim()),
        Span::styled("█".repeat(seg), theme::bar()),
        Span::styled("░".repeat(width - pos - seg), theme::dim()),
    ])
}

fn render_totals(app: &App, f: &mut Frame, area: Rect) {
    let block = theme::panel("Library", false);
    let st = app.dashboard.stats.as_deref();
    let lines = match st {
        None => vec![
            Line::styled("No numbers yet.", theme::dim()),
            Line::styled(
                "They appear as soon as the first files are scanned.",
                theme::dim(),
            ),
        ],
        Some(st) => {
            let mut l1 = vec![
                Span::styled("Photos ", theme::title()),
                Span::raw(fmt::thousands(st.photos.count)),
                Span::styled(format!(" ({})", fmt::bytes(st.photos.bytes)), theme::dim()),
                Span::styled("   Videos ", theme::title()),
                Span::raw(fmt::thousands(st.videos.count)),
                Span::styled(format!(" ({})", fmt::bytes(st.videos.bytes)), theme::dim()),
            ];
            if st.video.total_duration_ms > 0 {
                l1.push(Span::styled(
                    format!(" {} total", fmt::span(st.video.total_duration_ms)),
                    theme::dim(),
                ));
            }
            if st.live_photo_pairs > 0 {
                l1.push(Span::styled("   Live Photos ", theme::title()));
                l1.push(Span::raw(fmt::thousands(st.live_photo_pairs)));
            }

            let errors = st.errors.max(app.dashboard.scan.issues);
            let all = (st.photos.count + st.videos.count).max(1);
            let mut l2 = vec![
                Span::styled("Errors ", theme::title()),
                Span::styled(
                    fmt::thousands(errors),
                    if errors > 0 {
                        theme::error()
                    } else {
                        theme::good()
                    },
                ),
                Span::styled(" · iCloud-only ", theme::dim()),
                Span::raw(fmt::thousands(st.cloud_only)),
                Span::styled(" · located ", theme::dim()),
                Span::raw(format!("{}%", st.location.with_place * 100 / all)),
            ];
            if st.metadata_pending > 0 {
                l2.push(Span::styled(" · metadata pending ", theme::dim()));
                l2.push(Span::raw(fmt::thousands(st.metadata_pending)));
            }
            vec![Line::from(l1), Line::from(l2)]
        }
    };
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_panels(st: Option<&Stats>, f: &mut Frame, area: Rect) {
    let [a, b, c] = Layout::horizontal([
        Constraint::Percentage(34),
        Constraint::Percentage(33),
        Constraint::Percentage(33),
    ])
    .areas(area);
    let rows = area.height.saturating_sub(2) as usize;

    let formats: Vec<(String, u64)> = st
        .map(|s| {
            s.formats
                .iter()
                .map(|x| (x.format.clone(), x.count))
                .collect()
        })
        .unwrap_or_default();
    panel(
        f,
        a,
        "Formats",
        st.is_some(),
        bars(&formats, a.width.saturating_sub(2) as usize, rows),
    );

    let (years, title, undated) = match st {
        Some(s) => {
            let mut v: Vec<(String, u64)> = s
                .by_year
                .iter()
                .map(|y| (y.year.to_string(), y.count))
                .collect();
            let room = rows.saturating_sub(usize::from(s.undated > 0));
            let cut = v.len() > room;
            if cut {
                v = v.split_off(v.len() - room);
            }
            (
                v,
                if cut { "By year (latest)" } else { "By year" },
                s.undated,
            )
        }
        None => (vec![], "By year", 0),
    };
    let mut year_lines = bars(&years, b.width.saturating_sub(2) as usize, rows);
    if undated > 0 && year_lines.len() < rows {
        year_lines.push(Line::styled(
            format!("no date  {}", fmt::thousands(undated)),
            theme::dim(),
        ));
    }
    panel(f, b, title, st.is_some(), year_lines);

    // Right column: cameras above, places below.
    let [cam_area, place_area] =
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(c);
    let cam_rows = cam_area.height.saturating_sub(2) as usize;
    let cams: Vec<Line> = st
        .map(|s| {
            s.cameras
                .iter()
                .take(cam_rows)
                .map(|cam| count_line(&cam.camera, cam.count, cam_area.width))
                .collect()
        })
        .unwrap_or_default();
    panel(f, cam_area, "Top cameras", st.is_some(), cams);

    let place_rows = place_area.height.saturating_sub(2) as usize;
    let places = st.map(|s| place_lines(&s.location, place_area.width, place_rows));
    panel(
        f,
        place_area,
        "Top places",
        st.is_some(),
        places.unwrap_or_default(),
    );
}

/// `name        1,234`: the name is cut to fit, the count is right-aligned in the accent color.
fn count_line(name: &str, count: u64, width: u16) -> Line<'static> {
    let n = fmt::thousands(count);
    let w = usize::from(width.saturating_sub(2)).saturating_sub(n.len() + 1);
    Line::from(vec![
        Span::raw(format!("{:<w$} ", fmt::truncate(name, w))),
        Span::styled(n, theme::accent()),
    ])
}

/// The most photographed cities as `City, Country  count`. When there is room, a last line
/// sums up the countries and the files without a place.
fn place_lines(
    loc: &yojaka_core::stats::LocationStats,
    width: u16,
    rows: usize,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = loc
        .cities
        .iter()
        .map(|c| count_line(&format!("{}, {}", c.city, c.country), c.count, width))
        .collect();
    let summary = {
        let mut parts = vec![];
        if !loc.countries.is_empty() {
            let n = loc.countries.len();
            parts.push(format!("{n} countr{}", if n == 1 { "y" } else { "ies" }));
        }
        if loc.without_place > 0 {
            parts.push(format!(
                "{} without place",
                fmt::thousands(loc.without_place)
            ));
        }
        parts.join(" · ")
    };
    let keep = if summary.is_empty() {
        rows
    } else {
        rows.saturating_sub(1)
    };
    lines.truncate(keep);
    if !summary.is_empty() && rows > 0 {
        lines.push(Line::styled(
            fmt::truncate(&summary, usize::from(width.saturating_sub(2))),
            theme::dim(),
        ));
    }
    lines
}

fn panel(f: &mut Frame, area: Rect, title: &str, have_stats: bool, mut lines: Vec<Line<'static>>) {
    if lines.is_empty() {
        lines.push(Line::styled(
            if have_stats {
                "none found"
            } else {
                "waiting for data…"
            },
            theme::dim(),
        ));
    }
    let block = theme::panel(title.to_string(), false);
    f.render_widget(Paragraph::new(lines).block(block), area);
}

/// `label ████▌ 1,234` lines, scaled to the largest value.
fn bars(items: &[(String, u64)], width: usize, max_rows: usize) -> Vec<Line<'static>> {
    let max = items.iter().map(|(_, n)| *n).max().unwrap_or(0);
    let label_w = items
        .iter()
        .map(|(l, _)| l.chars().count())
        .max()
        .unwrap_or(0)
        .min(10);
    let count_w = fmt::thousands(max).len();
    let bar_w = width.saturating_sub(label_w + count_w + 2);
    items
        .iter()
        .take(max_rows)
        .map(|(label, n)| {
            let bar = fmt::bar(*n, max, bar_w);
            let pad = bar_w.saturating_sub(bar.chars().count());
            Line::from(vec![
                Span::raw(format!("{:<label_w$} ", fmt::truncate(label, label_w))),
                Span::styled(bar, theme::bar()),
                Span::raw(" ".repeat(pad)),
                Span::raw(format!(" {:>count_w$}", fmt::thousands(*n))),
            ])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_lines_have_equal_width() {
        let items = vec![("jpeg".to_string(), 1200), ("mov".to_string(), 3)];
        let lines = bars(&items, 30, 10);
        let w: Vec<usize> = lines.iter().map(Line::width).collect();
        assert_eq!(w[0], w[1]);
        assert!(w[0] <= 30);
    }

    #[test]
    fn bars_respect_the_row_limit() {
        let items: Vec<_> = (0..20).map(|i| (format!("f{i}"), i + 1)).collect();
        assert_eq!(bars(&items, 30, 5).len(), 5);
    }

    #[test]
    fn progress_bar_fills_its_width() {
        for r in [0.0, 0.01, 0.5, 0.999, 1.0, 2.0] {
            assert_eq!(ratio_bar(30, r, Style::new()).width(), 30, "{r}");
        }
        assert!(
            ratio_bar(10, 0.5, Style::new())
                .to_string()
                .starts_with("█████░")
        );
    }

    #[test]
    fn indeterminate_bar_fills_its_width_at_every_tick() {
        for tick in 0..100 {
            assert_eq!(indeterminate(40, tick).width(), 40, "tick {tick}");
        }
        assert_eq!(indeterminate(3, 0).width(), 0);
    }
}
