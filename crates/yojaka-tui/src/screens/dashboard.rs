//! Dashboard: live scan gauge, totals, and the format / year / camera charts.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use yojaka_core::TaskKind;
use yojaka_core::stats::Stats;

use crate::app::{App, ScanView};
use crate::{fmt, theme};

const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

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
        Some(TaskKind::Metadata) => "Reading metadata",
        Some(TaskKind::Geocode) => "Resolving places",
        _ if s.cancelling => "Stopping",
        _ => "Scanning",
    }
}

fn render_scan(app: &App, f: &mut Frame, area: Rect) {
    let d = &app.dashboard;
    let s = &d.scan;
    let block = Block::default().borders(Borders::ALL).title("Scan");
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
        let mut t = format!(
            "{} {} · {} files",
            SPINNER[(app.ticks as usize) % SPINNER.len()],
            phase_name(s),
            fmt::thousands(s.done)
        );
        if let Some(total) = s.total {
            t = format!(
                "{} {} · {} / {}",
                SPINNER[(app.ticks as usize) % SPINNER.len()],
                phase_name(s),
                fmt::thousands(s.done),
                fmt::thousands(total)
            );
        }
        if let Some(r) = s.rate {
            t.push_str(&format!(" · {}/s", fmt::thousands(r as u64)));
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
        "No index yet".to_string()
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
        match &s.current {
            Some(p) => Line::styled(
                fmt::truncate_start(&p.display().to_string(), w),
                theme::dim(),
            ),
            None => Line::styled("starting…", theme::dim()),
        }
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

/// A progress bar `width` cells wide, filled to `ratio`.
fn ratio_bar(width: u16, ratio: f64, fill: Style) -> Line<'static> {
    let width = width as usize;
    let done = fmt::bar((ratio.clamp(0.0, 1.0) * 1000.0) as u64, 1000, width);
    let pad = width.saturating_sub(done.chars().count());
    Line::from(vec![
        Span::styled(done, fill),
        Span::styled("░".repeat(pad), theme::dim()),
    ])
}

/// A bright block sliding along a dim track, for work whose size is not known yet.
fn indeterminate(width: u16, tick: u64) -> Line<'static> {
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
    let block = Block::default().borders(Borders::ALL).title("Library");
    let st = app.dashboard.stats.as_deref();
    let lines = match st {
        None => vec![
            Line::styled("No numbers yet.", theme::dim()),
            Line::styled(
                "They appear as soon as the first files are indexed.",
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

    let cams: Vec<Line> = st
        .map(|s| {
            s.cameras
                .iter()
                .take(rows)
                .map(|cam| {
                    let n = fmt::thousands(cam.count);
                    let w = (c.width.saturating_sub(2) as usize).saturating_sub(n.len() + 1);
                    Line::from(vec![
                        Span::raw(format!("{:<w$} ", fmt::truncate(&cam.camera, w))),
                        Span::styled(n, theme::accent()),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();
    panel(f, c, "Top cameras", st.is_some(), cams);
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
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title.to_string());
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
