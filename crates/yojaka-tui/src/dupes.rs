//! State of the Duplicates screen: the groups found, what is marked, and where the cursor is.
//! Marking is bookkeeping only; nothing here deletes a file.
//!
//! Keep rules mark identical copies only. Groups of the other tiers (likely, similar, name) are
//! different files that may be the same picture, so each one is marked by hand.

use yojaka_core::dupes::{DupGroup, DupReport, DupStats, KeepRule, Tier, parse_rules};

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
    /// A keep chain from the config file that is none of the `PRESETS`: it is one more step in
    /// the `p` cycle, after the last preset.
    pub saved: Option<String>,
    pub pane: Pane,
    /// Show only groups of this tier (`None`: all of them).
    pub filter: Option<Tier>,
    /// Index into `groups` of the selected group (always one the filter shows, if any is).
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
            saved: None,
            pane: Pane::Groups,
            filter: None,
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
            saved: self.saved.take(),
            page_rows: self.page_rows,
            ..Self::default()
        };
    }

    pub fn begin(&mut self) {
        let (preset, page_rows, saved) = (self.preset, self.page_rows, self.saved.take());
        *self = Self {
            phase: Phase::Searching,
            preset,
            saved,
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
        self.filter = None;
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

    /// The `--keep` text of the current chain.
    pub fn chain_text(&self) -> &str {
        match (&self.saved, self.preset) {
            (Some(text), i) if i >= PRESETS.len() => text,
            (_, i) => PRESETS[i.min(PRESETS.len() - 1)].1,
        }
    }

    pub fn rules(&self) -> Vec<KeepRule> {
        parse_rules(&[self.chain_text()]).unwrap_or_default()
    }

    pub fn preset_label(&self) -> String {
        match (&self.saved, self.preset) {
            (Some(text), i) if i >= PRESETS.len() => format!("saved: {text}"),
            (_, i) => PRESETS[i.min(PRESETS.len() - 1)].0.to_string(),
        }
    }

    /// Starts from the chain in the config file: a preset when it is one, else its own step.
    pub fn use_saved_chain(&mut self, saved: &[String]) {
        let text = saved.join(",");
        match PRESETS.iter().position(|(_, t)| *t == text) {
            Some(i) => self.preset = i,
            None => {
                self.saved = Some(text);
                self.preset = PRESETS.len();
            }
        }
    }

    /// Applies the current rule chain to every group of identical files (manual marks there are
    /// replaced). Groups of the other tiers are never marked by a rule.
    pub fn apply_preset(&mut self) {
        let rules = self.rules();
        for g in &mut self.groups {
            g.apply_rules(&rules);
        }
    }

    pub fn next_preset(&mut self) {
        let steps = PRESETS.len() + usize::from(self.saved.is_some());
        self.preset = (self.preset + 1) % steps;
        self.apply_preset();
    }

    pub fn current(&self) -> Option<&DupGroup> {
        self.groups
            .get(self.group)
            .filter(|g| self.filter.is_none_or(|t| g.tier == t))
    }

    /// Indices of the groups the tier filter shows, in order.
    pub fn visible(&self) -> Vec<usize> {
        self.groups
            .iter()
            .enumerate()
            .filter(|(_, g)| self.filter.is_none_or(|t| g.tier == t))
            .map(|(i, _)| i)
            .collect()
    }

    /// How many groups each tier has, in display order, leaving out empty tiers.
    pub fn tier_counts(&self) -> Vec<(Tier, usize)> {
        Tier::ALL
            .into_iter()
            .map(|t| (t, self.groups.iter().filter(|g| g.tier == t).count()))
            .filter(|(_, n)| *n > 0)
            .collect()
    }

    /// Groups that are not identical copies and have nothing marked yet.
    pub fn undecided(&self) -> usize {
        self.groups
            .iter()
            .filter(|g| !g.tier.is_exact() && g.delete_count() == 0)
            .count()
    }

    /// `v`: all tiers, then each tier that has groups, then all again.
    pub fn cycle_filter(&mut self) {
        let present: Vec<Tier> = self.tier_counts().into_iter().map(|(t, _)| t).collect();
        self.filter = match self.filter {
            None => present.first().copied(),
            Some(now) => present
                .iter()
                .position(|t| *t == now)
                .and_then(|i| present.get(i + 1))
                .copied(),
        };
        self.group = self.visible().first().copied().unwrap_or(0);
        self.copy = 0;
        self.pane = Pane::Groups;
    }

    pub fn filter_label(&self) -> &'static str {
        self.filter.map_or("all", Tier::label)
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
                let visible = self.visible();
                let Some(last) = visible.len().checked_sub(1) else {
                    return;
                };
                let at = visible.iter().position(|&i| i == self.group).unwrap_or(0);
                let g = visible[at.saturating_add_signed(delta as isize).min(last)];
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
        if self.current().is_none() {
            return Toggled::Nothing;
        }
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
        if self.current().is_some()
            && let Some(g) = self.groups.get_mut(self.group)
            && self.copy < g.members.len()
        {
            g.set_keeper(self.copy);
        }
    }

    /// Clears every mark in the selected group.
    pub fn keep_all(&mut self) {
        if self.current().is_some()
            && let Some(g) = self.groups.get_mut(self.group)
        {
            g.keep_all();
        }
    }
}
