//! Running a plan. Every file is re-checked against its fingerprint first, every step is
//! journaled before and after, and a Live Photo pair is acted on as one unit.

use std::collections::{HashMap, HashSet};
use std::os::macos::fs::MetadataExt as _;
use std::path::{Component, Path, PathBuf};

use uuid::Uuid;

use super::journal::{self, Journal, Run, State};
use super::trash::{Trasher, restore};
use crate::hash::{ContentHash, full_hash};
use crate::model::Fingerprint;
use crate::plan::{Op, OpMode, Plan, PlanKind};
use crate::protect::Protected;
use crate::sys::macos::is_dataless;
use crate::{
    AppPaths, CancelToken, CoreEvent, EventSink, FileIssue, IssueKind, Result, TaskKind,
    TaskOutcome, YojakaError,
};

/// Room the journal needs on its volume before anything is changed.
const MIN_FREE_FOR_JOURNAL: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default)]
pub struct ApplyOptions {
    /// Allow `Delete` ops (no Trash). The caller must have asked the user to confirm.
    pub permanent: bool,
    /// Continue a plan whose journal already exists.
    pub resume: bool,
}

#[derive(Debug, Clone)]
pub struct OpNote {
    pub op: u64,
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct ApplyReport {
    pub plan_id: Option<Uuid>,
    /// Files moved to the Trash by this run.
    pub trashed: usize,
    /// Files permanently deleted by this run.
    pub deleted: usize,
    /// Organize: files moved to their new place (renamed, or copied across volumes and removed).
    pub moved: usize,
    /// Organize: copies made (the originals are untouched).
    pub copied: usize,
    /// Organize: folders created.
    pub created: usize,
    /// Organize: files left where they were because the same file was already at the destination.
    pub already_there: usize,
    /// Bytes freed (dedupe) or transferred (organize) by this run.
    pub bytes: u64,
    /// Ops finished by an earlier run (resume).
    pub already_done: usize,
    pub skipped: Vec<OpNote>,
    pub failed: Vec<OpNote>,
    pub cancelled: bool,
}

impl ApplyReport {
    pub fn is_clean(&self) -> bool {
        self.skipped.is_empty() && self.failed.is_empty() && !self.cancelled
    }
}

/// Checks a plan without touching any file: kind, paths inside the root, nothing protected, and
/// no file that the plan also marks as the copy to keep.
pub fn check_plan(plan: &Plan, opts: ApplyOptions) -> Result<()> {
    if plan.kind == PlanKind::Organize {
        return super::organize_run::check(plan, opts);
    }
    if (plan.mode == OpMode::DeletePermanent
        || plan.ops.iter().any(|o| matches!(o, Op::Delete { .. })))
        && !opts.permanent
    {
        return Err(YojakaError::Internal(
            "this plan deletes files for good; that needs --permanent and a typed confirmation"
                .into(),
        ));
    }
    let root = plan
        .root
        .canonicalize()
        .map_err(|e| YojakaError::io(&plan.root, e))?;
    let protected = Protected::system();
    protected.check_root(&root)?;

    let mut kept: HashSet<&Path> = HashSet::new();
    let mut targets: HashSet<&Path> = HashSet::new();
    for op in &plan.ops {
        match op {
            Op::Trash { path, reason, .. } | Op::Delete { path, reason, .. } => {
                if path.components().any(|c| c == Component::ParentDir)
                    || !(path.starts_with(&plan.root) || path.starts_with(&root))
                {
                    return Err(YojakaError::Internal(format!(
                        "{} is outside the plan's folder; the plan was not run",
                        path.display()
                    )));
                }
                kept.insert(&reason.kept);
                targets.insert(path);
            }
            Op::Skip { .. } => {}
            Op::Move { .. } | Op::Copy { .. } | Op::MkDir { .. } => {
                return Err(YojakaError::Internal(
                    "a duplicate-removal plan cannot move or copy files".into(),
                ));
            }
        }
    }
    if let Some(p) = targets.iter().find(|p| kept.contains(**p)) {
        return Err(YojakaError::Internal(format!(
            "{} is both removed and kept by this plan; the plan was not run",
            p.display()
        )));
    }
    Ok(())
}

pub fn apply(
    plan: &Plan,
    paths: &AppPaths,
    trasher: &dyn Trasher,
    opts: ApplyOptions,
    sink: &EventSink,
    cancel: &CancelToken,
) -> Result<ApplyReport> {
    check_plan(plan, opts)?;
    paths.ensure()?;
    if let Some(free) = free_bytes(&paths.journal_dir())
        && free < MIN_FREE_FOR_JOURNAL
    {
        return Err(YojakaError::Internal(
            "the disk holding Yojaka's data is nearly full; nothing was changed".into(),
        ));
    }

    let mut prior: HashMap<u64, journal::Entry> = HashMap::new();
    let mut jr = if opts.resume {
        let lines = journal::read(paths, plan.id)?;
        for e in journal::last_states(&lines, Run::Apply) {
            prior.insert(e.op, e);
        }
        Journal::open(paths, plan.id)?
    } else {
        Journal::create(paths, plan)?
    };

    if plan.kind == PlanKind::Organize {
        let mut report = ApplyReport {
            plan_id: Some(plan.id),
            ..Default::default()
        };
        sink.important(CoreEvent::TaskStarted {
            task: TaskKind::Execute,
            label: "Applying plan".into(),
        });
        let outcome = super::organize_run::run(plan, &mut jr, &prior, sink, cancel, &mut report);
        sink.important(CoreEvent::TaskFinished {
            task: TaskKind::Execute,
            outcome: match &outcome {
                Err(e) => TaskOutcome::Failed(e.to_string()),
                Ok(()) if report.cancelled => TaskOutcome::Cancelled,
                Ok(()) if report.is_clean() => TaskOutcome::Complete,
                Ok(()) => TaskOutcome::Partial,
            },
        });
        return outcome.map(|()| report);
    }

    let root = plan
        .root
        .canonicalize()
        .map_err(|e| YojakaError::io(&plan.root, e))?;
    let mut run = Runner {
        root,
        protected: Protected::system(),
        trasher,
        permanent: opts.permanent,
        hashes: HashMap::new(),
        report: ApplyReport {
            plan_id: Some(plan.id),
            ..Default::default()
        },
        sink,
        total: plan.ops.len() as u64,
        seen: 0,
    };
    sink.important(CoreEvent::TaskStarted {
        task: TaskKind::Execute,
        label: "Applying plan".into(),
    });

    let outcome = run.go(plan, &mut jr, &prior, cancel);
    let report = run.report;
    let finished = match &outcome {
        Err(e) => TaskOutcome::Failed(e.to_string()),
        Ok(()) if report.cancelled => TaskOutcome::Cancelled,
        Ok(()) if report.is_clean() => TaskOutcome::Complete,
        Ok(()) => TaskOutcome::Partial,
    };
    sink.important(CoreEvent::TaskFinished {
        task: TaskKind::Execute,
        outcome: finished,
    });
    outcome.map(|()| report)
}

struct Runner<'a> {
    root: PathBuf,
    protected: Protected,
    trasher: &'a dyn Trasher,
    permanent: bool,
    hashes: HashMap<PathBuf, Option<ContentHash>>,
    report: ApplyReport,
    sink: &'a EventSink,
    total: u64,
    seen: u64,
}

/// A removal op with the fields the runner needs.
struct Item<'p> {
    id: u64,
    path: &'p Path,
    expect: &'p Fingerprint,
    kept: &'p Path,
    /// `Some` for a non-exact match: the kept copy is a different file, checked on its own.
    kept_expect: Option<&'p Fingerprint>,
    exact: bool,
    partner: Option<&'p Path>,
    permanent: bool,
}

impl Runner<'_> {
    fn go(
        &mut self,
        plan: &Plan,
        jr: &mut Journal,
        prior: &HashMap<u64, journal::Entry>,
        cancel: &CancelToken,
    ) -> Result<()> {
        let mut items: Vec<Item<'_>> = Vec::new();
        for op in &plan.ops {
            match op {
                Op::Trash {
                    id,
                    path,
                    expect,
                    reason,
                }
                | Op::Delete {
                    id,
                    path,
                    expect,
                    reason,
                } => {
                    items.push(Item {
                        id: *id,
                        path,
                        expect,
                        kept: &reason.kept,
                        kept_expect: reason.kept_expect.as_ref(),
                        exact: reason.tier.is_exact(),
                        partner: reason.partner.as_deref(),
                        permanent: matches!(op, Op::Delete { .. }),
                    });
                }
                Op::Skip { id, path, why } => {
                    if !matches!(prior.get(id), Some(e) if e.state == State::Skipped) {
                        jr.append(
                            Run::Apply,
                            *id,
                            State::Skipped,
                            Some(format!("{why:?}")),
                            None,
                        )?;
                    }
                    self.report.skipped.push(OpNote {
                        op: *id,
                        path: path.clone(),
                        reason: format!("{why:?}"),
                    });
                    self.tick(path);
                }
                Op::Move { .. } | Op::Copy { .. } | Op::MkDir { .. } => {}
            }
        }

        // Units: an op alone, or a Live Photo pair together.
        let by_path: HashMap<&Path, usize> = items
            .iter()
            .enumerate()
            .map(|(i, it)| (it.path, i))
            .collect();
        let mut used = vec![false; items.len()];
        let mut units: Vec<Vec<usize>> = Vec::new();
        for i in 0..items.len() {
            if used[i] {
                continue;
            }
            used[i] = true;
            let mut unit = vec![i];
            if let Some(p) = items[i].partner
                && let Some(&j) = by_path.get(p)
                && !used[j]
            {
                used[j] = true;
                unit.push(j);
            }
            units.push(unit);
        }

        for unit in units {
            if cancel.is_cancelled() {
                self.report.cancelled = true;
                break;
            }
            let members: Vec<&Item<'_>> = unit.iter().map(|&i| &items[i]).collect();
            self.run_unit(&members, jr, prior)?;
        }
        Ok(())
    }

    fn tick(&mut self, current: &Path) {
        self.seen += 1;
        self.sink.progress(CoreEvent::Progress {
            task: TaskKind::Execute,
            done: self.seen,
            total: Some(self.total),
            bytes: self.report.bytes,
            current: Some(current.to_path_buf()),
        });
    }

    fn note(&mut self, skipped: bool, it: &Item<'_>, reason: String) {
        self.sink.important(CoreEvent::Issue(FileIssue {
            path: it.path.to_path_buf(),
            kind: if skipped {
                IssueKind::Changed
            } else {
                IssueKind::Other
            },
            message: reason.clone(),
        }));
        let note = OpNote {
            op: it.id,
            path: it.path.to_path_buf(),
            reason,
        };
        if skipped {
            self.report.skipped.push(note);
        } else {
            self.report.failed.push(note);
        }
    }

    fn run_unit(
        &mut self,
        members: &[&Item<'_>],
        jr: &mut Journal,
        prior: &HashMap<u64, journal::Entry>,
    ) -> Result<()> {
        // Resume: what an earlier run finished, or started and did not finish.
        let mut todo: Vec<&Item<'_>> = Vec::new();
        for &it in members {
            match prior.get(&it.id).map(|e| (e.state, e)) {
                Some((State::Done | State::Skipped, _)) => {
                    self.report.already_done += 1;
                    self.tick(it.path);
                }
                Some((State::Begin, _)) if std::fs::symlink_metadata(it.path).is_err() => {
                    // The file is gone: the crash came after the move. Find where it went.
                    let found = (!it.permanent)
                        .then(|| self.trasher.find(it.path, it.expect))
                        .flatten();
                    let detail = if it.permanent || found.is_some() {
                        "recovered after an interruption".to_string()
                    } else {
                        "recovered after an interruption; the Trash location is unknown".to_string()
                    };
                    jr.append(Run::Apply, it.id, State::Done, Some(detail), found)?;
                    self.count_done(it);
                    self.tick(it.path);
                }
                _ => todo.push(it),
            }
        }
        if todo.is_empty() {
            return Ok(());
        }

        // Check every member first; one bad half spares both.
        let mut problems: Vec<(usize, String)> = Vec::new();
        for (i, it) in todo.iter().enumerate() {
            if let Err(why) = self.validate(it) {
                problems.push((i, why));
            }
        }
        if !problems.is_empty() {
            let first = problems[0].1.clone();
            for (i, it) in todo.iter().enumerate() {
                let reason = match problems.iter().find(|(p, _)| *p == i) {
                    Some((_, why)) => why.clone(),
                    None => {
                        format!("left alone because its Live Photo partner was skipped ({first})")
                    }
                };
                jr.append(
                    Run::Apply,
                    it.id,
                    State::Skipped,
                    Some(reason.clone()),
                    None,
                )?;
                self.note(true, it, reason);
                self.tick(it.path);
            }
            return Ok(());
        }

        // Act. If a later half fails, put the earlier ones back.
        let mut moved: Vec<(&Item<'_>, Option<PathBuf>)> = Vec::new();
        for it in &todo {
            jr.append(Run::Apply, it.id, State::Begin, None, None)?;
            let res = if it.permanent {
                if !self.permanent {
                    Err(YojakaError::Internal(
                        "permanent delete was not allowed".into(),
                    ))
                } else {
                    std::fs::remove_file(it.path)
                        .map(|()| None)
                        .map_err(|e| YojakaError::io(it.path, e))
                }
            } else {
                self.trasher.trash(it.path).map(Some)
            };
            match res {
                Ok(to) => {
                    jr.append(Run::Apply, it.id, State::Done, None, to.clone())?;
                    moved.push((it, to));
                }
                Err(e) => {
                    let msg = e.to_string();
                    jr.append(Run::Apply, it.id, State::Failed, Some(msg.clone()), None)?;
                    for (done, to) in moved.drain(..) {
                        let back = match &to {
                            Some(t) => restore(t, done.path).is_ok(),
                            None => false,
                        };
                        if back {
                            jr.append(
                                Run::Apply,
                                done.id,
                                State::Skipped,
                                Some("put back because its Live Photo partner failed".into()),
                                None,
                            )?;
                            self.note(
                                true,
                                done,
                                "put back because its Live Photo partner failed".into(),
                            );
                        } else {
                            // Could not put it back; leave the `done` entry.
                            self.count_done(done);
                        }
                    }
                    self.note(false, it, msg);
                    for rest in todo.iter().skip_while(|r| r.id != it.id).skip(1) {
                        jr.append(
                            Run::Apply,
                            rest.id,
                            State::Skipped,
                            Some("an earlier step failed".into()),
                            None,
                        )?;
                        self.note(true, rest, "an earlier step failed".into());
                    }
                    self.tick(it.path);
                    return Ok(());
                }
            }
        }
        for (it, _) in moved {
            self.count_done(it);
            self.tick(it.path);
        }
        Ok(())
    }

    fn count_done(&mut self, it: &Item<'_>) {
        if it.permanent {
            self.report.deleted += 1;
        } else {
            self.report.trashed += 1;
        }
        self.report.bytes += it.expect.size;
    }

    /// Is `it` still exactly what the plan saw, and is its kept copy still there?
    fn validate(&mut self, it: &Item<'_>) -> std::result::Result<(), String> {
        // The file must still be inside the plan's folder after following any symlinked parent.
        let parent = it.path.parent().ok_or("no parent folder")?;
        let parent = parent
            .canonicalize()
            .map_err(|e| format!("folder is gone ({e})"))?;
        if !parent.starts_with(&self.root) {
            return Err("it now resolves outside the plan's folder".into());
        }
        if let Some(why) = self.protected.reason(&parent) {
            return Err(format!("protected location ({why})"));
        }
        check_file(it.path, it.expect, "this file")?;
        if it.kept == it.path {
            return Err("it is the copy that should be kept".into());
        }
        if !it.exact {
            // A look-alike, not an identical copy: it cannot be compared with the removed file,
            // so it must still be exactly the file the person reviewed.
            let Some(expect) = it.kept_expect else {
                return Err("the plan does not say what the copy to keep looked like".into());
            };
            let what = format!("the copy to keep ({})", it.kept.display());
            return check_file(it.kept, expect, &what);
        }
        let kept = std::fs::symlink_metadata(it.kept)
            .map_err(|_| format!("the copy to keep ({}) is missing", it.kept.display()))?;
        if !kept.file_type().is_file() || kept.len() != it.expect.size {
            return Err(format!(
                "the copy to keep ({}) has changed",
                it.kept.display()
            ));
        }
        if let Some(want) = it.expect.hash {
            let got = self
                .hashes
                .entry(it.kept.to_path_buf())
                .or_insert_with(|| full_hash(it.kept, kept.len()).ok().map(|h| h.hash));
            if *got != Some(want) {
                return Err(format!(
                    "the copy to keep ({}) no longer matches",
                    it.kept.display()
                ));
            }
        }
        Ok(())
    }
}

/// Size, modification time and (when known) content must still match the fingerprint.
pub(super) fn check_file(
    path: &Path,
    expect: &Fingerprint,
    what: &str,
) -> std::result::Result<(), String> {
    let md = std::fs::symlink_metadata(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => format!("{what} is missing"),
        _ => format!("{what} cannot be read ({e})"),
    })?;
    if !md.file_type().is_file() {
        return Err(format!("{what} is no longer a regular file"));
    }
    if is_dataless(&md) {
        return Err(format!("{what} is stored only in iCloud"));
    }
    let mtime = md.st_mtime() * 1_000_000_000 + md.st_mtime_nsec();
    if md.len() != expect.size || mtime != expect.mtime_ns {
        return Err(format!("{what} changed after the plan was made"));
    }
    if let Some(want) = expect.hash {
        match full_hash(path, md.len()) {
            Ok(h) if h.hash == want => {}
            Ok(_) => return Err(format!("{what} changed after the plan was made")),
            Err(e) => return Err(format!("{what} cannot be read ({e})")),
        }
    }
    Ok(())
}

/// Free bytes on the volume holding `dir`, or `None` if it cannot be found out.
pub fn free_bytes(dir: &Path) -> Option<u64> {
    let out = std::process::Command::new("/bin/df")
        .arg("-k")
        .arg(dir)
        .output()
        .ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let line = text.lines().nth(1)?;
    let kib: u64 = line.split_whitespace().nth(3)?.parse().ok()?;
    Some(kib * 1024)
}

/// `1.5 GB`-style size for messages.
pub(super) fn human(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let (mut v, mut i) = (n as f64, 0);
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}
