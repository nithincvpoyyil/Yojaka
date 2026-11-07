//! State of the Browser screen: folder tree (left), virtualized file table (right), detail pane.
//!
//! The table never holds more than [`FETCH`] rows. It keeps one window of the result in memory,
//! shows placeholders for rows outside it, and asks the worker for a new window before the user
//! reaches the edge.

use std::collections::HashSet;

use yojaka_core::browse::{BrowseQuery, FileDetail, FileRow, FolderTree, KindFilter, SortKey};

use crate::app::Cmd;
use crate::worker::Request;

/// Rows fetched per request.
pub const FETCH: u64 = 256;
/// Fetch the next window when the view comes this close to the edge of the current one.
const MARGIN: u64 = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Tree,
    Table,
}

#[derive(Debug, Clone)]
pub struct Window {
    pub offset: u64,
    pub rows: Vec<FileRow>,
}

impl Window {
    fn end(&self) -> u64 {
        self.offset + self.rows.len() as u64
    }
}

#[derive(Debug)]
pub struct BrowserState {
    pub focus: Focus,
    pub tree: Option<FolderTree>,
    pub expanded: HashSet<usize>,
    /// Selected node of `tree`.
    pub selected: usize,
    pub tree_top: usize,
    pub query: BrowseQuery,
    /// Bumped on every query change; answers for older queries are ignored.
    pub generation: u64,
    /// Matching rows; `None` until the count arrives.
    pub total: Option<u64>,
    pub cursor: u64,
    pub top: u64,
    pub window: Option<Window>,
    inflight: bool,
    pub detail: Option<FileDetail>,
    detail_for: Option<i64>,
    pub editing_filter: bool,
    /// Visible table rows / tree rows, from the last layout.
    pub table_rows: u16,
    pub tree_rows: u16,
}

impl Default for BrowserState {
    fn default() -> Self {
        Self {
            focus: Focus::Table,
            tree: None,
            expanded: HashSet::from([0]),
            selected: 0,
            tree_top: 0,
            query: BrowseQuery::default(),
            generation: 0,
            total: None,
            cursor: 0,
            top: 0,
            window: None,
            inflight: false,
            detail: None,
            detail_for: None,
            editing_filter: false,
            table_rows: 10,
            tree_rows: 10,
        }
    }
}

impl BrowserState {
    /// Forgets everything read from the index (before it is flushed). Keeps the query.
    pub fn reset(&mut self) {
        self.generation += 1;
        self.tree = None;
        self.expanded = HashSet::from([0]);
        self.selected = 0;
        self.tree_top = 0;
        self.total = Some(0);
        self.cursor = 0;
        self.top = 0;
        self.window = None;
        self.inflight = false;
        self.detail = None;
        self.detail_for = None;
    }

    /// Nodes shown in the tree pane, in order, with their depth.
    pub fn visible_nodes(&self) -> Vec<(usize, u16)> {
        let mut out = vec![];
        let Some(tree) = &self.tree else { return out };
        if tree.is_empty() {
            return out;
        }
        let mut stack = vec![(0usize, 0u16)];
        while let Some((n, depth)) = stack.pop() {
            out.push((n, depth));
            if self.expanded.contains(&n) {
                for &c in tree.nodes[n].children.iter().rev() {
                    stack.push((c, depth + 1));
                }
            }
        }
        out
    }

    pub fn current_row(&self) -> Option<&FileRow> {
        let w = self.window.as_ref()?;
        let i = self.cursor.checked_sub(w.offset)?;
        w.rows.get(usize::try_from(i).ok()?)
    }

    /// The row at absolute position `pos`, if it is inside the cached window.
    pub fn row_at(&self, pos: u64) -> Option<&FileRow> {
        let w = self.window.as_ref()?;
        w.rows
            .get(usize::try_from(pos.checked_sub(w.offset)?).ok()?)
    }

    /// The detail for the row under the cursor, once it has arrived.
    pub fn current_detail(&self) -> Option<&FileDetail> {
        let id = self.current_row()?.id;
        self.detail.as_ref().filter(|d| d.row.id == id)
    }

    // ---- queries ------------------------------------------------------------------------

    /// The query changed (or the index did): start over from the first row.
    pub fn refresh(&mut self) -> Vec<Cmd> {
        self.generation += 1;
        self.total = None;
        self.cursor = 0;
        self.top = 0;
        self.window = None;
        self.detail = None;
        self.detail_for = None;
        self.inflight = true;
        vec![
            Cmd::Worker(Request::Count {
                generation: self.generation,
                query: self.query.clone(),
            }),
            Cmd::Worker(Request::Page {
                generation: self.generation,
                query: self.query.clone(),
                offset: 0,
                limit: FETCH,
            }),
        ]
    }

    pub fn on_count(&mut self, generation: u64, total: u64) -> Vec<Cmd> {
        if generation != self.generation {
            return vec![];
        }
        self.total = Some(total);
        self.cursor = self.cursor.min(total.saturating_sub(1));
        self.after_move()
    }

    pub fn on_page(
        &mut self,
        generation: u64,
        offset: u64,
        limit: u64,
        rows: Vec<FileRow>,
    ) -> Vec<Cmd> {
        if generation != self.generation {
            return vec![];
        }
        self.inflight = false;
        // A short page that is not the last one means the index changed under us: trust it.
        if (rows.len() as u64) < limit {
            self.total = Some(offset + rows.len() as u64);
            self.cursor = self.cursor.min(self.total.unwrap_or(0).saturating_sub(1));
        }
        self.window = Some(Window { offset, rows });
        self.after_move()
    }

    pub fn on_detail(&mut self, id: i64, detail: Option<FileDetail>) {
        if self.detail_for == Some(id) {
            self.detail = detail;
        }
    }

    pub fn on_folders(&mut self, tree: FolderTree) {
        // Keep the selection when the folder still exists after a rescan.
        let old = self
            .tree
            .as_ref()
            .and_then(|t| t.nodes.get(self.selected))
            .map(|n| n.path.clone());
        let keep = old.and_then(|p| tree.nodes.iter().position(|n| n.path == p));
        let was_expanded: HashSet<_> = self
            .tree
            .as_ref()
            .map(|t| {
                self.expanded
                    .iter()
                    .filter_map(|&i| t.nodes.get(i).map(|n| n.path.clone()))
                    .collect()
            })
            .unwrap_or_default();
        self.expanded = tree
            .nodes
            .iter()
            .enumerate()
            .filter(|(i, n)| *i == 0 || was_expanded.contains(&n.path))
            .map(|(i, _)| i)
            .collect();
        self.selected = keep.unwrap_or(0);
        self.tree = Some(tree);
        self.scroll_tree();
    }

    /// Asks for the next window and the cursor row's details when they are missing.
    fn after_move(&mut self) -> Vec<Cmd> {
        let mut cmds = self.ensure_window();
        if let Some(id) = self.current_row().map(|r| r.id)
            && self.detail_for != Some(id)
        {
            self.detail_for = Some(id);
            cmds.push(Cmd::Worker(Request::Detail { id }));
        }
        cmds
    }

    fn ensure_window(&mut self) -> Vec<Cmd> {
        let Some(total) = self.total else {
            return vec![];
        };
        if total == 0 || self.inflight {
            return vec![];
        }
        let lo = self.top;
        let hi = (self.top + u64::from(self.table_rows)).min(total);
        let need = match &self.window {
            None => true,
            Some(w) => {
                let left = if w.offset == 0 { 0 } else { MARGIN };
                let right = if w.end() >= total { 0 } else { MARGIN };
                lo < w.offset + left || hi + right > w.end()
            }
        };
        if !need {
            return vec![];
        }
        let offset = lo.saturating_sub(FETCH / 3).min(total - 1);
        self.inflight = true;
        vec![Cmd::Worker(Request::Page {
            generation: self.generation,
            query: self.query.clone(),
            offset,
            limit: FETCH,
        })]
    }

    // ---- table movement --------------------------------------------------------------------

    fn scroll_table(&mut self) {
        let rows = u64::from(self.table_rows.max(1));
        if self.cursor < self.top {
            self.top = self.cursor;
        } else if self.cursor >= self.top + rows {
            self.top = self.cursor + 1 - rows;
        }
    }

    pub fn move_cursor(&mut self, delta: i64) -> Vec<Cmd> {
        let Some(total) = self.total else {
            return vec![];
        };
        if total == 0 {
            return vec![];
        }
        let target = self.cursor.saturating_add_signed(delta).min(total - 1);
        self.cursor = target;
        self.scroll_table();
        self.after_move()
    }

    pub fn page(&mut self, down: bool) -> Vec<Cmd> {
        let step = i64::from(self.table_rows.max(2) - 1);
        self.move_cursor(if down { step } else { -step })
    }

    pub fn jump(&mut self, bottom: bool) -> Vec<Cmd> {
        let Some(total) = self.total else {
            return vec![];
        };
        self.cursor = if bottom { total.saturating_sub(1) } else { 0 };
        self.scroll_table();
        self.after_move()
    }

    // ---- tree movement ---------------------------------------------------------------------

    fn scroll_tree(&mut self) {
        let vis = self.visible_nodes();
        let Some(pos) = vis.iter().position(|(n, _)| *n == self.selected) else {
            return;
        };
        let rows = usize::from(self.tree_rows.max(1));
        if pos < self.tree_top {
            self.tree_top = pos;
        } else if pos >= self.tree_top + rows {
            self.tree_top = pos + 1 - rows;
        }
    }

    /// Makes the selected folder the table's filter.
    fn apply_folder(&mut self) -> Vec<Cmd> {
        let folder = match (&self.tree, self.selected) {
            (Some(t), n) if n > 0 => t.nodes.get(n).map(|x| x.path.clone()),
            _ => None,
        };
        if folder == self.query.folder {
            return vec![];
        }
        self.query.folder = folder;
        self.refresh()
    }

    pub fn tree_move(&mut self, delta: i64) -> Vec<Cmd> {
        let vis = self.visible_nodes();
        let Some(pos) = vis.iter().position(|(n, _)| *n == self.selected) else {
            return vec![];
        };
        let to = pos.saturating_add_signed(delta as isize).min(vis.len() - 1);
        self.selected = vis[to].0;
        self.scroll_tree();
        self.apply_folder()
    }

    pub fn tree_collapse_or_parent(&mut self) -> Vec<Cmd> {
        let Some(tree) = &self.tree else {
            return vec![];
        };
        if self.expanded.contains(&self.selected) && !tree.nodes[self.selected].children.is_empty()
        {
            self.expanded.remove(&self.selected);
            // The root stays open: it is the only way to see everything.
            if self.selected == 0 {
                self.expanded.insert(0);
            }
            self.scroll_tree();
            return vec![];
        }
        if let Some(p) = tree.nodes[self.selected].parent {
            self.selected = p;
            self.scroll_tree();
            return self.apply_folder();
        }
        vec![]
    }

    pub fn tree_expand_or_child(&mut self) -> Vec<Cmd> {
        let Some(tree) = &self.tree else {
            return vec![];
        };
        let node = &tree.nodes[self.selected];
        if node.children.is_empty() {
            return vec![];
        }
        if !self.expanded.contains(&self.selected) {
            self.expanded.insert(self.selected);
            self.scroll_tree();
            return vec![];
        }
        self.selected = node.children[0];
        self.scroll_tree();
        self.apply_folder()
    }

    // ---- query controls --------------------------------------------------------------------

    pub fn cycle_kind(&mut self) -> Vec<Cmd> {
        self.query.kind = self.query.kind.next();
        self.refresh()
    }

    pub fn cycle_sort(&mut self) -> Vec<Cmd> {
        self.query.sort = self.query.sort.next();
        self.refresh()
    }

    pub fn reverse_sort(&mut self) -> Vec<Cmd> {
        self.query.descending = !self.query.descending;
        self.refresh()
    }

    pub fn set_text(&mut self, text: String) -> Vec<Cmd> {
        if text == self.query.text {
            return vec![];
        }
        self.query.text = text;
        self.refresh()
    }

    pub fn is_default_query(&self) -> bool {
        self.query.kind == KindFilter::All
            && self.query.text.is_empty()
            && self.query.sort == SortKey::Name
            && !self.query.descending
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use yojaka_core::model::MediaKind;

    use super::*;

    pub fn row(id: i64) -> FileRow {
        FileRow {
            id,
            path: PathBuf::from(format!("/r/f{id}.jpg")),
            name: format!("f{id}.jpg"),
            kind: MediaKind::Image,
            format: Some("jpeg".into()),
            width: None,
            height: None,
            duration_ms: None,
            size_bytes: 1,
            taken_at: None,
            camera: None,
            place: None,
            place_inferred: false,
            live: false,
            cloud_only: false,
        }
    }

    fn rows(from: u64, n: u64) -> Vec<FileRow> {
        (from..from + n).map(|i| row(i as i64)).collect()
    }

    fn loaded(total: u64) -> BrowserState {
        let mut b = BrowserState {
            table_rows: 10,
            ..Default::default()
        };
        b.refresh();
        b.on_count(b.generation, total);
        b.on_page(b.generation, 0, FETCH, rows(0, FETCH.min(total)));
        b
    }

    fn pages(cmds: &[Cmd]) -> Vec<u64> {
        cmds.iter()
            .filter_map(|c| match c {
                Cmd::Worker(Request::Page { offset, .. }) => Some(*offset),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn refresh_asks_for_a_count_and_the_first_page() {
        let mut b = BrowserState::default();
        let cmds = b.refresh();
        assert_eq!(cmds.len(), 2);
        assert_eq!(pages(&cmds), [0]);
    }

    #[test]
    fn stale_answers_are_ignored() {
        let mut b = BrowserState::default();
        b.refresh();
        let old = b.generation;
        b.refresh();
        assert!(b.on_count(old, 5).is_empty());
        assert!(b.total.is_none());
        assert!(b.on_page(old, 0, FETCH, rows(0, 5)).is_empty());
        assert!(b.window.is_none());
    }

    #[test]
    fn the_cursor_stays_inside_the_result() {
        let mut b = loaded(5);
        b.move_cursor(100);
        assert_eq!(b.cursor, 4);
        b.move_cursor(-100);
        assert_eq!(b.cursor, 0);
        b.jump(true);
        assert_eq!(b.cursor, 4);
    }

    #[test]
    fn scrolling_keeps_the_cursor_on_screen() {
        let mut b = loaded(100);
        b.move_cursor(25);
        assert_eq!(b.cursor, 25);
        assert_eq!(b.top, 16); // 25 + 1 - 10 rows
        b.move_cursor(-20);
        assert_eq!(b.top, 5);
    }

    #[test]
    fn a_new_window_is_requested_before_the_edge_is_reached() {
        let mut b = loaded(10_000);
        // far from the edge: nothing to fetch
        assert!(pages(&b.move_cursor(50)).is_empty());
        // within MARGIN of the end of the 256-row window
        let cmds = b.move_cursor(170);
        assert_eq!(pages(&cmds).len(), 1, "{cmds:?}");
        // only one request in flight at a time
        assert!(pages(&b.move_cursor(1)).is_empty());
    }

    #[test]
    fn jumping_to_the_end_fetches_the_last_rows() {
        let mut b = loaded(10_000);
        let cmds = b.jump(true);
        let off = pages(&cmds)[0];
        assert!(off > 9_000 && off < 10_000);
        b.on_page(b.generation, off, FETCH, rows(off, 10_000 - off));
        assert!(b.current_row().is_some());
        assert_eq!(b.current_row().map(|r| r.id), Some(9_999));
    }

    #[test]
    fn a_short_page_corrects_the_total() {
        let mut b = loaded(1_000);
        b.on_page(b.generation, 0, FETCH, rows(0, 10));
        assert_eq!(b.total, Some(10));
    }

    #[test]
    fn detail_is_requested_once_per_row_and_stale_ones_are_dropped() {
        let mut b = loaded(20);
        // loading the first page already asked for row 0's details
        assert_eq!(b.detail_for, Some(0));
        let cmds = b.move_cursor(1);
        assert!(
            cmds.iter()
                .any(|c| matches!(c, Cmd::Worker(Request::Detail { id: 1 })))
        );
        b.on_detail(0, None); // stale
        assert!(b.detail.is_none());
    }

    fn tree() -> FolderTree {
        FolderTree::build(
            std::path::Path::new("/r"),
            [
                (PathBuf::from("/r"), 1),
                (PathBuf::from("/r/a"), 2),
                (PathBuf::from("/r/a/x"), 3),
                (PathBuf::from("/r/b"), 4),
            ],
        )
    }

    #[test]
    fn tree_navigation_filters_the_table() {
        let mut b = BrowserState::default();
        b.on_folders(tree());
        let names = |b: &BrowserState| -> Vec<String> {
            let t = b.tree.as_ref().unwrap();
            b.visible_nodes()
                .iter()
                .map(|(n, d)| format!("{}{}", " ".repeat(*d as usize), t.nodes[*n].name))
                .collect()
        };
        assert_eq!(names(&b), ["r", " a", " b"]);
        // move onto "a": the table is now filtered by it
        let cmds = b.tree_move(1);
        assert_eq!(cmds.len(), 2);
        assert_eq!(b.query.folder, Some(PathBuf::from("/r/a")));
        // expand it
        b.tree_expand_or_child();
        assert_eq!(names(&b), ["r", " a", "  x", " b"]);
        // right again enters the child
        b.tree_expand_or_child();
        assert_eq!(b.query.folder, Some(PathBuf::from("/r/a/x")));
        // left goes to the parent, left again collapses it
        b.tree_collapse_or_parent();
        assert_eq!(b.query.folder, Some(PathBuf::from("/r/a")));
        b.tree_collapse_or_parent();
        assert_eq!(names(&b), ["r", " a", " b"]);
        // back on the root, the folder filter goes away
        b.tree_collapse_or_parent();
        assert_eq!(b.query.folder, None);
    }

    #[test]
    fn the_root_cannot_be_collapsed() {
        let mut b = BrowserState::default();
        b.on_folders(tree());
        b.tree_collapse_or_parent();
        assert!(b.expanded.contains(&0));
        assert_eq!(b.visible_nodes().len(), 3);
    }

    #[test]
    fn rescan_keeps_the_selected_folder() {
        let mut b = BrowserState::default();
        b.on_folders(tree());
        b.tree_move(1);
        b.tree_expand_or_child();
        b.on_folders(tree());
        let t = b.tree.as_ref().unwrap();
        assert_eq!(t.nodes[b.selected].path, PathBuf::from("/r/a"));
        assert!(b.expanded.contains(&b.selected));
    }

    #[test]
    fn changing_filters_restarts_the_table() {
        let mut b = loaded(100);
        b.move_cursor(30);
        let g = b.generation;
        b.cycle_kind();
        assert_eq!(b.query.kind, KindFilter::Photos);
        assert_eq!((b.cursor, b.top, b.generation), (0, 0, g + 1));
        b.cycle_sort();
        b.reverse_sort();
        assert!(b.query.descending);
        assert!(b.set_text("abc".into()).len() == 2);
        assert!(b.set_text("abc".into()).is_empty());
        assert!(!b.is_default_query());
    }
}
