//! State of the Organize screen: the settings, and the plan they produce.

use std::path::{Path, PathBuf};

use yojaka_core::organize::{OrganizePlan, PRESETS, Template, Transfer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Editing {
    Template,
    Dest,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Phase {
    /// Settings changed (or nothing planned yet): press Enter to plan.
    #[default]
    Idle,
    Planning,
    Ready,
    Failed(String),
}

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
    pub editing: Option<Editing>,
    backup: String,
    pub phase: Phase,
    pub plan: Option<Box<OrganizePlan>>,
    /// Why the template or folder cannot be used, shown under the settings.
    pub problem: Option<String>,
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
            editing: None,
            backup: String::new(),
            phase: Phase::Idle,
            plan: None,
            problem: None,
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
        if self.phase != Phase::Planning {
            self.phase = Phase::Idle;
        }
    }

    pub fn reset(&mut self) {
        self.invalidate();
    }

    pub fn next_preset(&mut self) {
        let i = self.preset.map_or(0, |i| (i + 1) % PRESETS.len());
        self.preset = Some(i);
        self.template = PRESETS[i].1.to_string();
        self.invalidate();
    }

    pub fn toggle_transfer(&mut self) {
        self.transfer = match self.transfer {
            Transfer::Copy => Transfer::Move,
            Transfer::Move => Transfer::Copy,
        };
        self.invalidate();
    }

    pub fn toggle_infer(&mut self) {
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
            return Err("Give a destination folder (press d)".into());
        }
        let dest = std::path::absolute(dest).map_err(|e| e.to_string())?;
        Ok(PlanRequest {
            dest,
            template,
            transfer: self.transfer,
            infer: self.infer,
        })
    }

    pub fn preset_label(&self) -> &'static str {
        self.preset.map_or("custom", |i| PRESETS[i].0)
    }

    /// Up to five example moves, picking different kinds of file: a photo, a video, one with a
    /// place, one without, one inferred. Never more than `n`.
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
            if out.len() >= n {
                break;
            }
        }
        out
    }
}
