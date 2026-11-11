//! State of the History screen: past runs, newest first, and the selected one.

use yojaka_core::exec::RunSummary;

#[derive(Debug, Default)]
pub struct HistoryState {
    pub runs: Vec<RunSummary>,
    pub selected: usize,
    /// The list has been read at least once.
    pub loaded: bool,
    pub page_rows: u16,
}

impl HistoryState {
    pub fn set(&mut self, runs: Vec<RunSummary>) {
        self.runs = runs;
        self.loaded = true;
        self.selected = self.selected.min(self.runs.len().saturating_sub(1));
    }

    pub fn current(&self) -> Option<&RunSummary> {
        self.runs.get(self.selected)
    }

    pub fn move_by(&mut self, delta: i64) {
        let last = self.runs.len().saturating_sub(1);
        self.selected = self
            .selected
            .saturating_add_signed(delta as isize)
            .min(last);
    }

    pub fn page(&mut self, forward: bool) {
        let step = i64::from(self.page_rows.max(1));
        self.move_by(if forward { step } else { -step });
    }

    pub fn jump(&mut self, bottom: bool) {
        self.move_by(if bottom { i64::MAX / 2 } else { i64::MIN / 2 });
    }
}
