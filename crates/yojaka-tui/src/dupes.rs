//! State of the Duplicates screen: the groups found, what is marked, and where the cursor is.
//! Marking is bookkeeping only; nothing here (or anywhere in this iteration) deletes a file.

use yojaka_core::dupes::{DupGroup, DupReport, DupStats, KeepRule, parse_rules};

/// Keep-rule chains `p` cycles through: (label, `--keep` text).
pub const PRESETS: [(&str, &str); 4] = [
    (
        "oldest, then shortest path",
        "oldest-modified,shortest-path",
    ),
    ("shortest path", "shortest-path,oldest-modified"),
    (
        "newest, then shortest path",
        "newest-modified,shortest-path",
    ),
    (
        "highest resolution, then oldest",
        "highest-resolution,oldest-modified,shortest-path",
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Groups,
    Copies,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Phase {
    /// Nothing looked for yet in this session.
    #[default]
    Idle,
    Searching,
    Done,
    Cancelled,
    Failed(String),
}

/// What happened when a mark was toggled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggled {
    Changed,
    /// Refused: it would have marked the last copy that stays.
    LastCopy,
    Nothing,
}

#[derive(Debug)]
pub struct DupesState {
    pub phase: Phase,
    pub groups: Vec<DupGroup>,
    pub stats: Option<DupStats>,
    pub issues: u64,
    pub preset: usize,
    pub pane: Pane,
    pub group: usize,
    pub copy: usize,
    pub done: u64,
    pub total: Option<u64>,
    pub bytes: u64,
    /// Rows the lists show; set from the layout so PgUp / PgDn know their step.
    pub page_rows: u16,
}

impl Default for DupesState {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            groups: Vec::new(),
            stats: None,
            issues: 0,
            preset: 0,
            pane: Pane::Groups,
            group: 0,
            copy: 0,
            done: 0,
            total: None,
            bytes: 0,
            page_rows: 10,
        }
    }
}

impl DupesState {
    pub fn searching(&self) -> bool {
        self.phase == Phase::Searching
    }

    /// 0..=1 while the search knows its size.
    pub fn ratio(&self) -> Option<f64> {
        let total = self.total?;
        (total > 0).then(|| (self.done as f64 / total as f64).clamp(0.0, 1.0))
    }

    /// Forgets the results (the index changed, so they may be stale).
    pub fn reset(&mut self) {
        *self = Self {
            preset: self.preset,
            page_rows: self.page_rows,
            ..Self::default()
        };
    }

    pub fn begin(&mut self) {
        let (preset, page_rows) = (self.preset, self.page_rows);
        *self = Self {
            phase: Phase::Searching,
            preset,
            page_rows,
            ..Self::default()
        };
    }

    pub fn progress(&mut self, done: u64, total: Option<u64>, bytes: u64) {
        self.done = done;
        self.total = total;
        self.bytes = bytes;
    }

    pub fn finish(&mut self, report: DupReport) {
        self.issues = report.issues.len() as u64;
        self.stats = Some(report.stats);
        self.groups = report.groups;
        self.apply_preset();
        self.group = 0;
        self.copy = 0;
        self.pane = Pane::Groups;
        self.phase = Phase::Done;
    }

    pub fn cancelled(&mut self) {
        self.phase = Phase::Cancelled;
    }

    pub fn failed(&mut self, message: String) {
        self.phase = Phase::Failed(message);
    }

    pub fn rules(&self) -> Vec<KeepRule> {
        parse_rules(&[PRESETS[self.preset].1]).unwrap_or_default()
    }

    pub fn preset_label(&self) -> &'static str {
        PRESETS[self.preset].0
    }

    /// Applies the current rule chain to every group (manual marks are replaced).
    pub fn apply_preset(&mut self) {
        let rules = self.rules();
        for g in &mut self.groups {
            g.apply_rules(&rules);
        }
    }

    pub fn next_preset(&mut self) {
        self.preset = (self.preset + 1) % PRESETS.len();
        self.apply_preset();
    }

    pub fn current(&self) -> Option<&DupGroup> {
        self.groups.get(self.group)
    }

    /// Bytes freed if every mark were carried out.
    pub fn total_reclaim(&self) -> u64 {
        self.groups.iter().map(DupGroup::reclaim).sum()
    }

    /// Files (not items) that are marked.
    pub fn marked_files(&self) -> usize {
        self.groups
            .iter()
            .flat_map(|g| g.members.iter().zip(&g.delete))
            .filter(|(_, d)| **d)
            .map(|(m, _)| m.files.len())
            .sum()
    }

    pub fn move_by(&mut self, delta: i64) {
        match self.pane {
            Pane::Groups => {
                let last = self.groups.len().saturating_sub(1);
                let g = self.group.saturating_add_signed(delta as isize).min(last);
                if g != self.group {
                    self.group = g;
                    self.copy = 0;
                }
            }
            Pane::Copies => {
                let last = self
                    .current()
                    .map_or(0, |g| g.members.len().saturating_sub(1));
                self.copy = self.copy.saturating_add_signed(delta as isize).min(last);
            }
        }
    }

    pub fn page(&mut self, forward: bool) {
        let step = i64::from(self.page_rows.max(1));
        self.move_by(if forward { step } else { -step });
    }

    pub fn jump(&mut self, bottom: bool) {
        self.move_by(if bottom { i64::MAX / 2 } else { i64::MIN / 2 });
    }

    pub fn switch_pane(&mut self) {
        self.pane = match self.pane {
            Pane::Groups => Pane::Copies,
            Pane::Copies => Pane::Groups,
        };
    }

    /// Flips the mark on the selected copy. The last unmarked copy of a group cannot be marked.
    pub fn toggle(&mut self) -> Toggled {
        let Some(g) = self.groups.get_mut(self.group) else {
            return Toggled::Nothing;
        };
        if g.toggle(self.copy) {
            Toggled::Changed
        } else {
            Toggled::LastCopy
        }
    }

    /// Keeps the selected copy and marks the others.
    pub fn keep_only(&mut self) {
        if let Some(g) = self.groups.get_mut(self.group)
            && self.copy < g.members.len()
        {
            g.set_keeper(self.copy);
        }
    }

    /// Clears every mark in the selected group.
    pub fn keep_all(&mut self) {
        if let Some(g) = self.groups.get_mut(self.group) {
            g.keep_all();
        }
    }
}
