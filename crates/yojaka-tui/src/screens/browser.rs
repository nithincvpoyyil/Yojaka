//! Browser: folder tree (left), virtualized file table (right), detail pane (below the table).

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table};
use yojaka_core::browse::{FileDetail, FileRow, SortKey};
use yojaka_core::meta::format_local;
use yojaka_core::model::MediaKind;

use crate::app::App;
use crate::browser::{BrowserState, Focus};
use crate::view::browser_layout;
use crate::{fmt, theme};

pub fn render(app: &App, f: &mut Frame, area: Rect) {
    let l = browser_layout(area);
    render_tree(&app.browser, f, l.tree);
    render_table(app, f, l.table);
    render_detail(&app.browser, f, l.detail);
}

// ---- tree ----------------------------------------------------------------------------------

fn render_tree(b: &BrowserState, f: &mut Frame, area: Rect) {
    let focused = b.focus == Focus::Tree && !b.editing_filter;
    let block = theme::panel("Folders", focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let (Some(tree), w) = (&b.tree, inner.width as usize) else {
        let msg = if b.total == Some(0) {
            "no folders yet"
        } else {
            "loading…"
        };
        f.render_widget(Paragraph::new(msg).style(theme::dim()), inner);
        return;
    };
    let lines: Vec<Line> = b
        .visible_nodes()
        .into_iter()
        .skip(b.tree_top)
        .take(inner.height as usize)
        .map(|(n, depth)| {
            let node = &tree.nodes[n];
            let marker = if node.children.is_empty() {
                ' '
            } else if b.expanded.contains(&n) {
                '▾'
            } else {
                '▸'
            };
            let count = fmt::thousands(node.files_total);
            let name = node.name.clone();
            let label_w = w.saturating_sub(count.chars().count() + 1);
            let label = fmt::truncate(
                &format!("{}{marker} {name}", " ".repeat(depth as usize)),
                label_w,
            );
            let style = if n == b.selected {
                if focused {
                    theme::selected()
                } else {
                    theme::selected_dim()
                }
            } else {
                Style::new()
            };
            Line::from(vec![
                Span::styled(format!("{label:<label_w$} "), style),
                Span::styled(count, if n == b.selected { style } else { theme::dim() }),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

// ---- table ---------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Col {
    Size,
    Taken,
    Duration,
    Format,
    Dimensions,
    Kind,
    Camera,
    Place,
}

impl Col {
    /// Most useful first: columns are dropped from the end when the table is narrow.
    const PRIORITY: [Col; 8] = [
        Col::Size,
        Col::Taken,
        Col::Duration,
        Col::Format,
        Col::Dimensions,
        Col::Kind,
        Col::Camera,
        Col::Place,
    ];

    fn width(self, narrow: bool) -> u16 {
        match self {
            Col::Size => 9,
            Col::Taken if narrow => 10,
            Col::Taken => 16,
            Col::Duration => 7,
            Col::Format => 7,
            Col::Dimensions => 9,
            Col::Kind => 5,
            Col::Camera | Col::Place => 18,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Col::Size => "Size",
            Col::Taken => "Taken",
            Col::Duration => "Length",
            Col::Format => "Format",
            Col::Dimensions => "Pixels",
            Col::Kind => "Kind",
            Col::Camera => "Camera",
            Col::Place => "Place",
        }
    }

    fn sort_key(self) -> Option<SortKey> {
        Some(match self {
            Col::Size => SortKey::Size,
            Col::Taken => SortKey::Taken,
            Col::Duration => SortKey::Duration,
            Col::Format => SortKey::Format,
            Col::Kind => SortKey::Kind,
            Col::Camera => SortKey::Camera,
            Col::Place => SortKey::Place,
            Col::Dimensions => return None,
        })
    }
}

const MIN_NAME: u16 = 16;

/// The columns that fit `inner_w` cells next to a name column of at least [`MIN_NAME`], and the
/// width that is left for the name.
fn pick_columns(inner_w: u16) -> (Vec<Col>, u16) {
    let narrow = inner_w < 100;
    let mut used = MIN_NAME;
    let mut cols = vec![];
    for c in Col::PRIORITY {
        let need = c.width(narrow) + 1;
        if used + need <= inner_w {
            used += need;
            cols.push(c);
        }
    }
    (cols, inner_w.saturating_sub(used - MIN_NAME).max(MIN_NAME))
}

fn cell_text(c: Col, r: &FileRow, narrow: bool) -> String {
    match c {
        Col::Size => fmt::bytes(r.size_bytes),
        Col::Taken => r.taken_at.map_or(String::new(), |t| {
            let s = format_local(t);
            if narrow {
                s[..10].to_string()
            } else {
                s[..16].to_string()
            }
        }),
        Col::Duration => r.duration_ms.map_or(String::new(), fmt::clock),
        Col::Format => r.format.clone().unwrap_or_default(),
        Col::Dimensions => match (r.width, r.height) {
            (Some(w), Some(h)) => format!("{w}×{h}"),
            _ => String::new(),
        },
        Col::Kind => match r.kind {
            MediaKind::Image => "photo".into(),
            MediaKind::Video => "video".into(),
        },
        Col::Camera => r.camera.clone().unwrap_or_default(),
        Col::Place => r.place.as_ref().map_or(String::new(), |p| {
            if r.place_inferred {
                format!("≈ {p}")
            } else {
                p.clone()
            }
        }),
    }
}

fn row_cells<'a>(r: &FileRow, cols: &[Col], name_w: u16, narrow: bool) -> Vec<Cell<'a>> {
    let badge = if r.live { " LIVE" } else { "" };
    let name = fmt::truncate(&r.name, (name_w as usize).saturating_sub(badge.len()));
    let name_style = if r.cloud_only {
        theme::dim()
    } else {
        Style::new()
    };
    let mut name_line = vec![Span::styled(name, name_style)];
    if r.live {
        name_line.push(Span::styled(badge, theme::live_badge()));
    }
    let mut cells = vec![Cell::from(Line::from(name_line))];
    for &c in cols {
        let text = fmt::truncate(&cell_text(c, r, narrow), c.width(narrow) as usize);
        let line = if c == Col::Size {
            Line::from(text).alignment(Alignment::Right)
        } else {
            Line::from(text)
        };
        cells.push(Cell::from(line));
    }
    cells
}

fn render_table(app: &App, f: &mut Frame, area: Rect) {
    let b = &app.browser;
    let focused = b.focus == Focus::Table;
    let arrow = if b.query.descending { '▼' } else { '▲' };
    let total = b.total.map_or("…".to_string(), fmt::thousands);
    let mut title = format!(
        "Files · {} · {total} · by {} {arrow}",
        b.query.kind.label(),
        b.query.sort.label()
    );
    if b.editing_filter || !b.query.text.is_empty() {
        title.push_str(&format!(" · /{}", b.query.text));
    }
    let block = theme::panel(title, focused || b.editing_filter);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if b.total == Some(0) {
        let msg = if b.is_default_query() && b.query.folder.is_none() {
            if app.dashboard.scan.running {
                "Scanning… files appear here when the scan finishes."
            } else {
                "Nothing scanned yet. Press r to scan this folder."
            }
        } else {
            "No files match the current folder and filters."
        };
        f.render_widget(Paragraph::new(msg).style(theme::dim()), inner);
        return;
    }

    let narrow = inner.width < 100;
    let (cols, name_w) = pick_columns(inner.width);

    let mut head = vec![Cell::from(if b.query.sort == SortKey::Name {
        format!("Name {arrow}")
    } else {
        "Name".to_string()
    })];
    for &c in &cols {
        let mark = if c.sort_key() == Some(b.query.sort) {
            format!("{} {arrow}", c.title())
        } else {
            c.title().to_string()
        };
        head.push(Cell::from(mark));
    }
    let header = Row::new(head).style(theme::title());

    let total_rows = b.total.unwrap_or(0);
    let shown = u64::from(inner.height.saturating_sub(1));
    let rows: Vec<Row> = (b.top..(b.top + shown).min(total_rows.max(b.top + shown)))
        .take_while(|&p| b.total.is_none_or(|t| p < t))
        .map(|pos| {
            let row = match b.row_at(pos) {
                Some(r) => Row::new(row_cells(r, &cols, name_w, narrow)),
                None => Row::new(vec![Cell::from("…")]).style(theme::dim()),
            };
            if pos == b.cursor {
                row.style(if focused {
                    theme::selected()
                } else {
                    theme::selected_dim()
                })
            } else {
                row
            }
        })
        .collect();

    let mut widths = vec![Constraint::Length(name_w)];
    widths.extend(cols.iter().map(|c| Constraint::Length(c.width(narrow))));
    f.render_widget(
        Table::new(rows, widths).header(header).column_spacing(1),
        inner,
    );
}

// ---- detail --------------------------------------------------------------------------------

fn join(parts: Vec<String>) -> String {
    parts
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Most important first, so a short pane cuts the least useful lines.
fn detail_lines(row: &FileRow, d: Option<&FileDetail>, width: usize) -> Vec<Line<'static>> {
    let mut name = vec![Span::styled(row.name.clone(), theme::title())];
    if row.live {
        name.push(Span::raw(" "));
        name.push(Span::styled(" LIVE ", theme::live_badge()));
        if let Some(p) = d.and_then(|d| d.live_partner.as_ref()) {
            let partner = p
                .file_name()
                .map_or(String::new(), |n| n.to_string_lossy().into_owned());
            name.push(Span::styled(format!(" pair: {partner}"), theme::dim()));
        }
    }
    if row.cloud_only {
        name.push(Span::styled("  iCloud only, not downloaded", theme::dim()));
    }

    let mut facts = vec![
        match row.kind {
            MediaKind::Image => "photo".to_string(),
            MediaKind::Video => "video".to_string(),
        },
        row.format.clone().unwrap_or_default(),
        match (row.width, row.height) {
            (Some(w), Some(h)) => format!("{w}×{h}"),
            _ => String::new(),
        },
        fmt::bytes(row.size_bytes),
        row.duration_ms.map_or(String::new(), fmt::clock),
    ];
    if let Some(fr) = d.and_then(|d| d.frames).filter(|n| *n > 1) {
        facts.push(format!("{fr} frames"));
    }

    let taken = row.taken_at.map_or("no date".to_string(), |t| {
        let src = d.and_then(|d| d.taken_at_src.clone()).unwrap_or_default();
        if src.is_empty() {
            format_local(t)
        } else {
            format!("{} ({src})", format_local(t))
        }
    });
    let mut cam = vec![taken, row.camera.clone().unwrap_or_default()];
    if let Some(lens) = d.and_then(|d| d.lens.clone()) {
        cam.push(lens);
    }

    let mut place = vec![match &row.place {
        Some(p) if row.place_inferred => format!("≈ {p} (inferred)"),
        Some(p) => p.clone(),
        None => "no location".to_string(),
    }];
    if d.is_some_and(|d| d.has_gps) {
        place.push("has GPS".into());
    }

    let mut out = vec![
        Line::from(name),
        Line::from(join(facts)),
        Line::from(join(cam)),
        Line::from(join(place)),
    ];
    if let Some(d) = d
        && row.kind == MediaKind::Video
    {
        let codec = match (&d.video_codec, &d.audio_codec) {
            (Some(v), Some(a)) => format!("{v}/{a}"),
            (Some(v), None) => v.clone(),
            _ => String::new(),
        };
        let tech = join(vec![
            codec,
            d.fps.map_or(String::new(), |v| format!("{v:.0} fps")),
            d.bitrate
                .map_or(String::new(), |b| format!("{:.1} Mbps", b as f64 / 1e6)),
            d.container.clone().unwrap_or_default(),
        ]);
        if !tech.is_empty() {
            out.push(Line::from(tech));
        }
    }
    let folder = row
        .path
        .parent()
        .map_or(String::new(), |p| p.display().to_string());
    out.push(Line::styled(
        fmt::truncate_start(&folder, width),
        theme::dim(),
    ));
    match d {
        None => out.push(Line::styled("loading details…", theme::dim())),
        Some(d) if matches!(d.metadata_status.as_str(), "error" | "pending") => out.push(
            Line::styled(format!("metadata: {}", d.metadata_status), theme::dim()),
        ),
        Some(_) => {}
    }
    out
}

fn render_detail(b: &BrowserState, f: &mut Frame, area: Rect) {
    let block = theme::panel("Details", false);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let Some(row) = b.current_row() else {
        f.render_widget(
            Paragraph::new("No file selected.").style(theme::dim()),
            inner,
        );
        return;
    };
    let lines = detail_lines(row, b.current_detail(), inner.width as usize);
    f.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_tables_drop_the_least_useful_columns_first() {
        let (cols, name) = pick_columns(54);
        assert_eq!(cols[0], Col::Size);
        assert!(cols.contains(&Col::Taken));
        assert!(!cols.contains(&Col::Place));
        assert!(name >= MIN_NAME);
        let used: u16 = name + cols.iter().map(|c| c.width(true) + 1).sum::<u16>();
        assert!(used <= 54, "{used}");
    }

    #[test]
    fn wide_tables_show_everything() {
        let (cols, _) = pick_columns(126);
        assert_eq!(cols.len(), Col::PRIORITY.len());
    }

    #[test]
    fn the_name_column_takes_what_is_left() {
        for w in [40u16, 54, 80, 126, 200] {
            let narrow = w < 100;
            let (cols, name) = pick_columns(w);
            let used: u16 = name + cols.iter().map(|c| c.width(narrow) + 1).sum::<u16>();
            assert!(used <= w.max(MIN_NAME), "w={w} used={used}");
        }
    }
}
