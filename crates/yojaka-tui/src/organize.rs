//! State of the Organize screen: a short form of settings, and the plan they produce. The plan
//! follows the settings by itself (working it out only reads), so the screen is: choose, look,
//! review.

use std::path::{Path, PathBuf};

use yojaka_core::organize::{OrganizePlan, PRESETS, Template, Transfer};

/// The rows of the settings form, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// A ready-made pattern, or "Custom".
    Layout,
    Pattern,
    Dest,
    Action,
    /// What happens to files without GPS.
    NoGps,
}

pub const ROWS: [Row; 5] = [
    Row::Layout,
    Row::Pattern,
    Row::Dest,
    Row::Action,
    Row::NoGps,
];

impl Row {
    pub fn label(self) -> &'static str {
        match self {
            Row::Layout => "Layout",
            Row::Pattern => "Pattern",
            Row::Dest => "Destination",
            Row::Action => "Action",
            Row::NoGps => "Without GPS",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Editing {
    Template,
    Dest,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Phase {
    /// Nothing planned for the current settings yet.
    #[default]
    Idle,
    Planning,
    Ready,
    Failed(String),
}

/// The error text of a plan stopped with Esc.
pub const STOPPED: &str = "Stopped";

/// What the runtime needs to plan.
#[derive(Debug, Clone)]
pub struct PlanRequest {
    pub dest: PathBuf,
    pub template: Template,
    pub transfer: Transfer,
    pub infer: bool,
}

#[derive(Debug)]
pub struct OrganizeState {
    pub template: String,
    /// Index into `PRESETS` while the template is a preset.
    pub preset: Option<usize>,
    pub dest: String,
    pub transfer: Transfer,
    pub infer: bool,
    /// The selected row of the form (index into `ROWS`).
    pub cursor: usize,
    pub editing: Option<Editing>,
    backup: String,
    pub phase: Phase,
    pub plan: Option<Box<OrganizePlan>>,
    /// Why the template or folder cannot be used, shown under the settings.
    pub problem: Option<String>,
    /// The settings changed while a plan was being worked out: drop that plan when it comes.
    stale: bool,
    /// The stale plan was asked to stop.
    cancel_sent: bool,
}

impl OrganizeState {
    pub fn new(root: &Path) -> Self {
        // Next to the scanned folder, never inside it.
        let dest = root
            .parent()
            .unwrap_or(root)
            .join("Yojaka Organized")
            .display()
            .to_string();
        Self {
            template: PRESETS[0].1.to_string(),
            preset: Some(0),
            dest,
            transfer: Transfer::Copy,
            infer: false,
            cursor: 0,
            editing: None,
            backup: String::new(),
            phase: Phase::Idle,
            plan: None,
            problem: None,
            stale: false,
            cancel_sent: false,
        }
    }

    /// Starts from the choices saved in the config file. Returns a warning for a value that was
    /// ignored.
    pub fn use_config(&mut self, config: &yojaka_core::Config) -> Option<String> {
        let (template, w1) = yojaka_core::organize::configured_template(&config.organize.template);
        let (transfer, w2) = yojaka_core::organize::configured_transfer(&config.organize.mode);
        self.preset = PRESETS.iter().position(|(_, t)| *t == template);
        self.template = template;
        self.transfer = transfer;
        self.infer |= config.location.infer;
        w1.or(w2)
    }

    /// Settings changed: what was planned no longer matches.
    pub fn invalidate(&mut self) {
        self.plan = None;
        self.problem = None;
        if self.phase == Phase::Planning {
            self.stale = true;
        } else {
            self.phase = Phase::Idle;
        }
    }

    pub fn reset(&mut self) {
        self.invalidate();
    }

    /// A plan came back. One for settings that changed since is dropped (and planned again).
    pub fn planned(&mut self, result: Result<Box<OrganizePlan>, String>) {
        if std::mem::take(&mut self.stale) {
            self.cancel_sent = false;
            self.phase = Phase::Idle;
            return;
        }
        match result {
            Ok(p) => {
                self.plan = Some(p);
                self.phase = Phase::Ready;
            }
            Err(e) => self.phase = Phase::Failed(e),
        }
    }

    /// True once when the plan being worked out is out of date and should be stopped.
    pub fn take_cancel(&mut self) -> bool {
        let stop = self.phase == Phase::Planning && self.stale && !self.cancel_sent;
        self.cancel_sent |= stop;
        stop
    }

    /// The settings are complete and nothing is planned for them yet.
    pub fn wants_plan(&self) -> bool {
        self.phase == Phase::Idle && self.problem.is_none() && self.editing.is_none()
    }

    pub fn row(&self) -> Row {
        ROWS[self.cursor.min(ROWS.len() - 1)]
    }

    pub fn move_cursor(&mut self, delta: isize) {
        self.cursor = self.cursor.saturating_add_signed(delta).min(ROWS.len() - 1);
    }

    pub fn jump(&mut self, to_end: bool) {
        self.cursor = if to_end { ROWS.len() - 1 } else { 0 };
    }

    /// Enter on the selected row: type in a text box, or take the next choice.
    pub fn activate(&mut self) {
        match self.row() {
            Row::Pattern => self.start_edit(Editing::Template),
            Row::Dest => self.start_edit(Editing::Dest),
            _ => {
                self.change(true);
            }
        }
    }

    /// ← / → on a choice row: the previous / next choice. False on a text row (nothing to do).
    pub fn change(&mut self, forward: bool) -> bool {
        match self.row() {
            Row::Layout => self.step_preset(forward),
            Row::Action => self.toggle_transfer(),
            Row::NoGps => self.toggle_infer(),
            Row::Pattern | Row::Dest => return false,
        }
        true
    }

    /// The next (or previous) ready-made pattern. From a custom pattern: the first (or last).
    fn step_preset(&mut self, forward: bool) {
        let n = PRESETS.len();
        let i = match (self.preset, forward) {
            (Some(i), true) => (i + 1) % n,
            (Some(i), false) => (i + n - 1) % n,
            (None, true) => 0,
            (None, false) => n - 1,
        };
        self.preset = Some(i);
        self.template = PRESETS[i].1.to_string();
        self.invalidate();
    }

    fn toggle_transfer(&mut self) {
        self.transfer = match self.transfer {
            Transfer::Copy => Transfer::Move,
            Transfer::Move => Transfer::Copy,
        };
        self.invalidate();
    }

    fn toggle_infer(&mut self) {
        self.infer = !self.infer;
        self.invalidate();
    }

    pub fn start_edit(&mut self, what: Editing) {
        self.backup = self.text(what).clone();
        self.editing = Some(what);
    }

    fn text(&self, what: Editing) -> &String {
        match what {
            Editing::Template => &self.template,
            Editing::Dest => &self.dest,
        }
    }

    fn text_mut(&mut self, what: Editing) -> &mut String {
        match what {
            Editing::Template => &mut self.template,
            Editing::Dest => &mut self.dest,
        }
    }

    pub fn type_char(&mut self, c: char) {
        if let Some(e) = self.editing {
            self.text_mut(e).push(c);
        }
    }

    pub fn backspace(&mut self) {
        if let Some(e) = self.editing {
            self.text_mut(e).pop();
        }
    }

    /// Ctrl-U while typing: empty the box (Esc still brings the old text back).
    pub fn clear_text(&mut self) {
        if let Some(e) = self.editing {
            self.text_mut(e).clear();
        }
    }

    pub fn accept_edit(&mut self) {
        let Some(e) = self.editing.take() else { return };
        if *self.text(e) != self.backup {
            if e == Editing::Template {
                self.preset = PRESETS.iter().position(|(_, t)| *t == self.template);
            }
            self.invalidate();
        }
    }

    pub fn cancel_edit(&mut self) {
        if let Some(e) = self.editing.take() {
            let old = std::mem::take(&mut self.backup);
            *self.text_mut(e) = old;
        }
    }

    /// A request, or the reason the settings cannot be used yet.
    pub fn request(&self) -> Result<PlanRequest, String> {
        let template = Template::parse(&self.template).map_err(|e| e.to_string())?;
        let dest = self.dest.trim();
        if dest.is_empty() {
            return Err("Type a destination folder".into());
        }
        let dest = std::path::absolute(expand_home(dest)).map_err(|e| e.to_string())?;
        Ok(PlanRequest {
            dest,
            template,
            transfer: self.transfer,
            infer: self.infer,
        })
    }

    pub fn preset_label(&self) -> &'static str {
        self.preset.map_or("Custom", |i| PRESETS[i].0)
    }

    /// False when the pattern has no place folders, so the GPS choice changes nothing.
    pub fn uses_place(&self) -> bool {
        Template::parse(&self.template).map_or(true, |t| t.uses_place())
    }

    /// Up to `n` example moves, picking different kinds of file: a photo, a video, one with a
    /// place, one without, one inferred.
    pub fn examples(&self, n: usize) -> Vec<(String, String, bool)> {
        use yojaka_core::plan::Op;
        let Some(p) = &self.plan else { return vec![] };
        let dest = &p.plan.dest;
        let mut out: Vec<(String, String, bool)> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let class = |src: &Path, dst: &Path, inferred: bool| {
            let video = matches!(
                src.extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_lowercase)
                    .as_deref(),
                Some("mov" | "mp4" | "m4v" | "avi" | "mkv" | "webm" | "mts" | "3gp")
            );
            let s = dst.to_string_lossy();
            format!(
                "{video}{inferred}{}{}",
                s.contains("_undated"),
                s.contains("No Location")
            )
        };
        for op in &p.plan.ops {
            if out.len() >= n {
                break;
            }
            let (Op::Move {
                src, dst, inferred, ..
            }
            | Op::Copy {
                src, dst, inferred, ..
            }) = op
            else {
                continue;
            };
            if !seen.insert(class(src, dst, *inferred)) {
                continue;
            }
            let rel = dest
                .as_deref()
                .and_then(|d| dst.strip_prefix(d).ok())
                .unwrap_or(dst);
            out.push((
                src.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                rel.display().to_string(),
                *inferred,
            ));
        }
        out
    }
}

/// `~` and `~/x` mean the home folder, as in a shell.
fn expand_home(s: &str) -> PathBuf {
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty());
    match home {
        Some(h) if s == "~" => PathBuf::from(h),
        Some(h) if s.starts_with("~/") => PathBuf::from(h).join(&s[2..]),
        _ => PathBuf::from(s),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_tilde_means_the_home_folder() {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        assert_eq!(expand_home("~"), home);
        assert_eq!(expand_home("~/Pictures/x"), home.join("Pictures/x"));
        assert_eq!(expand_home("/a/~/b"), PathBuf::from("/a/~/b"));
        assert_eq!(expand_home("~bob/x"), PathBuf::from("~bob/x"));
    }

    #[test]
    fn the_cursor_stays_on_the_form() {
        let mut o = OrganizeState::new(Path::new("/photos"));
        o.move_cursor(-1);
        assert_eq!(o.row(), Row::Layout);
        o.move_cursor(99);
        assert_eq!(o.row(), Row::NoGps);
        o.jump(false);
        assert_eq!(o.cursor, 0);
    }

    #[test]
    fn layout_steps_both_ways_and_a_custom_pattern_starts_again() {
        let mut o = OrganizeState::new(Path::new("/photos"));
        assert!(o.change(false));
        assert_eq!(o.preset_label(), PRESETS[PRESETS.len() - 1].0);
        assert!(o.change(true));
        assert_eq!(o.preset_label(), "Date");
        o.template = "{year}/{name}".into();
        o.preset = None;
        assert_eq!(o.preset_label(), "Custom");
        o.change(true);
        assert_eq!(o.template, PRESETS[0].1);
    }

    #[test]
    fn text_rows_have_no_choices() {
        let mut o = OrganizeState::new(Path::new("/photos"));
        o.cursor = 1;
        assert!(!o.change(true));
        o.activate();
        assert_eq!(o.editing, Some(Editing::Template));
        o.clear_text();
        assert!(o.template.is_empty());
        o.cancel_edit();
        assert_eq!(o.template, PRESETS[0].1, "Esc brings the old text back");
    }

    #[test]
    fn a_plan_for_old_settings_is_dropped_and_stopped_once() {
        let mut o = OrganizeState::new(Path::new("/photos"));
        o.phase = Phase::Planning;
        assert!(!o.take_cancel());
        o.cursor = 3;
        o.activate(); // copy -> move
        assert!(o.take_cancel());
        assert!(!o.take_cancel(), "asked to stop only once");
        o.planned(Err(STOPPED.into()));
        assert_eq!(o.phase, Phase::Idle);
        assert!(o.wants_plan());
    }
}
