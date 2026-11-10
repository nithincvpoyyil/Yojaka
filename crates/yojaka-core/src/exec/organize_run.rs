//! Running an organize plan: create folders, then move or copy each file, never replacing one.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path};

use super::apply::{ApplyOptions, ApplyReport, OpNote, check_file, free_bytes};
use super::journal::{self, Journal, Run, State};
use super::transfer::{self, Moved};
use crate::hash::full_hash;
use crate::model::Fingerprint;
use crate::plan::{Op, OpMode, Plan, SkipReason};
use crate::protect::Protected;
use crate::{
    CancelToken, CoreEvent, EventSink, FileIssue, IssueKind, Result, TaskKind, YojakaError,
};

/// Spare room left on the destination volume after the copies.
const FREE_MARGIN: u64 = 64 * 1024 * 1024;

fn bad(msg: String) -> YojakaError {
    YojakaError::Internal(msg)
}

fn no_parent_dirs(p: &Path) -> bool {
    !p.components().any(|c| c == Component::ParentDir)
}

/// Checks an organize plan without touching any file.
pub(super) fn check(plan: &Plan, opts: ApplyOptions) -> Result<()> {
    if opts.permanent {
        return Err(bad("organize plans never delete files".into()));
    }
    let dest = plan
        .dest
        .as_deref()
        .ok_or_else(|| bad("the plan has no destination folder".into()))?;
    if !no_parent_dirs(dest) || !dest.is_absolute() {
        return Err(bad(format!(
            "{} is not a usable destination",
            dest.display()
        )));
    }
    let protected = Protected::system();
    let root = plan
        .root
        .canonicalize()
        .map_err(|e| YojakaError::io(&plan.root, e))?;
    protected.check_root(&root)?;
    // The destination may not exist yet: judge the nearest folder that does.
    let existing = dest.ancestors().find(|a| a.exists()).unwrap_or(dest);
    let canon = existing
        .canonicalize()
        .map_err(|e| YojakaError::io(existing, e))?;
    if let Some(reason) = protected.reason(&canon) {
        return Err(YojakaError::Protected {
            path: dest.to_path_buf(),
            reason,
        });
    }
    let want_copy = plan.mode == OpMode::Copy;
    let mut dsts: HashSet<String> = HashSet::new();
    for op in &plan.ops {
        match op {
            Op::Move { src, dst, .. } | Op::Copy { src, dst, .. } => {
                if matches!(op, Op::Copy { .. }) != want_copy {
                    return Err(bad("the plan mixes copies and moves".into()));
                }
                if !no_parent_dirs(src) || !(src.starts_with(&plan.root) || src.starts_with(&root))
                {
                    return Err(bad(format!(
                        "{} is outside the plan's folder; the plan was not run",
                        src.display()
                    )));
                }
                if !no_parent_dirs(dst) || !dst.starts_with(dest) {
                    return Err(bad(format!(
                        "{} is outside the destination folder; the plan was not run",
                        dst.display()
                    )));
                }
                if !dsts.insert(dst.to_string_lossy().to_lowercase()) {
                    return Err(bad(format!("two files would go to {}", dst.display())));
                }
            }
            Op::MkDir { path, .. } => {
                if !no_parent_dirs(path) || !path.starts_with(dest) {
                    return Err(bad(format!(
                        "{} is outside the destination folder; the plan was not run",
                        path.display()
                    )));
                }
            }
            Op::Skip { .. } => {}
            Op::Trash { .. } | Op::Delete { .. } => {
                return Err(bad("an organize plan cannot remove files".into()));
            }
        }
    }
    Ok(())
}

pub(super) fn run(
    plan: &Plan,
    jr: &mut Journal,
    prior: &HashMap<u64, journal::Entry>,
    sink: &EventSink,
    cancel: &CancelToken,
    report: &mut ApplyReport,
) -> Result<()> {
    let dest = plan.dest.clone().unwrap_or_default();
    let protected = Protected::system();
    let root = plan
        .root
        .canonicalize()
        .map_err(|e| YojakaError::io(&plan.root, e))?;

    // Room for what will be copied.
    let existing = dest.ancestors().find(|a| a.exists()).unwrap_or(&dest);
    let needed = bytes_to_write(plan, existing);
    if needed > 0
        && let Some(free) = free_bytes(existing)
        && free < needed + FREE_MARGIN
    {
        return Err(bad(format!(
            "not enough free space at the destination: {} needed, {} free; nothing was changed",
            super::apply::human(needed + FREE_MARGIN),
            super::apply::human(free)
        )));
    }

    let mut pairs: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, op) in plan.ops.iter().enumerate() {
        if let Op::Move { pair: Some(p), .. } | Op::Copy { pair: Some(p), .. } = op {
            pairs.entry(*p).or_default().push(i);
        }
    }
    let ctx = Ctx {
        plan,
        dest,
        root,
        protected,
        prior,
    };
    let total = plan.ops.len() as u64;
    let mut seen = 0u64;
    let mut done_idx: HashSet<usize> = HashSet::new();
    for (i, op) in plan.ops.iter().enumerate() {
        if done_idx.contains(&i) {
            continue;
        }
        if cancel.is_cancelled() {
            report.cancelled = true;
            break;
        }
        let unit: Vec<usize> = match op {
            Op::Move { pair: Some(p), .. } | Op::Copy { pair: Some(p), .. } => {
                pairs.get(p).cloned().unwrap_or_else(|| vec![i])
            }
            _ => vec![i],
        };
        done_idx.extend(unit.iter().copied());
        match op {
            Op::MkDir { .. } => ctx.mkdir(op, jr, report)?,
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
                if *why == SkipReason::AlreadyThere {
                    report.already_there += 1;
                } else {
                    report.skipped.push(OpNote {
                        op: *id,
                        path: path.clone(),
                        reason: format!("{why:?}"),
                    });
                }
            }
            _ => ctx.unit(&unit, jr, report, sink)?,
        }
        seen += unit.len() as u64;
        sink.progress(CoreEvent::Progress {
            task: TaskKind::Execute,
            done: seen,
            total: Some(total),
            bytes: report.bytes,
            current: op_path(op),
        });
    }
    Ok(())
}

fn op_path(op: &Op) -> Option<std::path::PathBuf> {
    match op {
        Op::Move { src, .. } | Op::Copy { src, .. } => Some(src.clone()),
        Op::MkDir { path, .. } => Some(path.clone()),
        _ => None,
    }
}

/// Bytes the plan will write on the destination volume.
fn bytes_to_write(plan: &Plan, dest_anchor: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    let dev = dest_anchor.metadata().ok().map(|m| m.dev());
    plan.ops
        .iter()
        .map(|op| match op {
            Op::Copy { expect, .. } => expect.size,
            Op::Move { src, expect, .. } => {
                let other = src.metadata().ok().map(|m| m.dev());
                if other.is_some() && other != dev {
                    expect.size
                } else {
                    0
                }
            }
            _ => 0,
        })
        .sum()
}

struct Ctx<'a> {
    plan: &'a Plan,
    dest: std::path::PathBuf,
    root: std::path::PathBuf,
    protected: Protected,
    prior: &'a HashMap<u64, journal::Entry>,
}

struct Xfer<'a> {
    id: u64,
    src: &'a Path,
    dst: &'a Path,
    expect: &'a Fingerprint,
    is_move: bool,
}

fn xfer(op: &Op) -> Option<Xfer<'_>> {
    match op {
        Op::Move {
            id,
            src,
            dst,
            expect,
            ..
        } => Some(Xfer {
            id: *id,
            src,
            dst,
            expect,
            is_move: true,
        }),
        Op::Copy {
            id,
            src,
            dst,
            expect,
            ..
        } => Some(Xfer {
            id: *id,
            src,
            dst,
            expect,
            is_move: false,
        }),
        _ => None,
    }
}

impl Ctx<'_> {
    fn mkdir(&self, op: &Op, jr: &mut Journal, report: &mut ApplyReport) -> Result<()> {
        let Op::MkDir { id, path } = op else {
            return Ok(());
        };
        if matches!(self.prior.get(id), Some(e) if matches!(e.state, State::Done | State::Skipped))
        {
            report.already_done += 1;
            return Ok(());
        }
        jr.append(Run::Apply, *id, State::Begin, None, None)?;
        match std::fs::symlink_metadata(path) {
            Ok(md) if md.is_dir() => {
                jr.append(Run::Apply, *id, State::Done, Some("existed".into()), None)?;
            }
            Ok(_) => {
                let msg = "a file with that name is in the way".to_string();
                jr.append(Run::Apply, *id, State::Failed, Some(msg.clone()), None)?;
                report.failed.push(OpNote {
                    op: *id,
                    path: path.clone(),
                    reason: msg,
                });
            }
            Err(_) => match std::fs::create_dir(path) {
                Ok(()) => {
                    if let Some(p) = path.parent() {
                        transfer::sync_dir(p);
                    }
                    jr.append(Run::Apply, *id, State::Done, Some("created".into()), None)?;
                    report.created += 1;
                }
                Err(e) => {
                    let msg = e.to_string();
                    jr.append(Run::Apply, *id, State::Failed, Some(msg.clone()), None)?;
                    report.failed.push(OpNote {
                        op: *id,
                        path: path.clone(),
                        reason: msg,
                    });
                }
            },
        }
        Ok(())
    }

    /// Checks a file's source and destination just before it is touched.
    fn validate(&self, x: &Xfer<'_>) -> std::result::Result<(), String> {
        let parent = x.src.parent().ok_or("no parent folder")?;
        let parent = parent
            .canonicalize()
            .map_err(|e| format!("the source folder is gone ({e})"))?;
        if !parent.starts_with(&self.root) {
            return Err("the file now resolves outside the plan's folder".into());
        }
        if let Some(why) = self.protected.reason(&parent) {
            return Err(format!("protected location ({why})"));
        }
        check_file(x.src, x.expect, "the file")?;
        if x.dst.symlink_metadata().is_ok() {
            return Err("something is already at the destination".into());
        }
        let dparent = x.dst.parent().ok_or("no destination folder")?;
        let dcanon = dparent
            .canonicalize()
            .map_err(|_| "the destination folder was not created".to_string())?;
        let dest_canon = self
            .dest
            .canonicalize()
            .unwrap_or_else(|_| self.dest.clone());
        if !dcanon.starts_with(&dest_canon) {
            return Err("the destination folder resolves outside the destination".into());
        }
        if let Some(why) = self.protected.reason(&dcanon) {
            return Err(format!("protected location ({why})"));
        }
        Ok(())
    }

    fn unit(
        &self,
        idx: &[usize],
        jr: &mut Journal,
        report: &mut ApplyReport,
        sink: &EventSink,
    ) -> Result<()> {
        let mut todo: Vec<Xfer<'_>> = Vec::new();
        for &i in idx {
            let Some(x) = xfer(&self.plan.ops[i]) else {
                continue;
            };
            match self.prior.get(&x.id).map(|e| e.state) {
                Some(State::Done | State::Skipped) => report.already_done += 1,
                Some(State::Begin) if self.finished_before_crash(&x, jr)? => {
                    report.already_done += 1;
                }
                _ => todo.push(x),
            }
        }
        if todo.is_empty() {
            return Ok(());
        }
        // One bad half spares both.
        let problems: Vec<(u64, String)> = todo
            .iter()
            .filter_map(|x| self.validate(x).err().map(|e| (x.id, e)))
            .collect();
        if !problems.is_empty() {
            let first = problems[0].1.clone();
            for x in &todo {
                let reason = match problems.iter().find(|(id, _)| *id == x.id) {
                    Some((_, why)) => why.clone(),
                    None => {
                        format!("left alone because its Live Photo partner was skipped ({first})")
                    }
                };
                jr.append(Run::Apply, x.id, State::Skipped, Some(reason.clone()), None)?;
                note(sink, report, true, x, reason);
            }
            return Ok(());
        }

        let mut finished: Vec<(&Xfer<'_>, Option<crate::hash::ContentHash>)> = Vec::new();
        for x in &todo {
            jr.append(Run::Apply, x.id, State::Begin, None, None)?;
            let res: Result<(String, Option<crate::hash::ContentHash>)> = if x.is_move {
                transfer::move_file(x.src, x.dst, x.id, x.expect.size, x.expect.mtime_ns).map(|m| {
                    match m {
                        Moved::Renamed => ("renamed".to_string(), None),
                        Moved::Copied(h) => {
                            ("copied across volumes, then removed".to_string(), Some(h))
                        }
                    }
                })
            } else {
                transfer::copy_verified(x.src, x.dst, x.id, x.expect.size, x.expect.mtime_ns)
                    .map(|h| ("copied".to_string(), Some(h)))
            };
            match res {
                Ok((detail, hash)) => {
                    jr.append_hashed(Run::Apply, x.id, State::Done, Some(detail), None, hash)?;
                    finished.push((x, hash));
                    report.bytes += x.expect.size;
                    if x.is_move {
                        report.moved += 1;
                    } else {
                        report.copied += 1;
                    }
                }
                Err(e) => {
                    let msg = e.to_string();
                    jr.append(Run::Apply, x.id, State::Failed, Some(msg.clone()), None)?;
                    // Put the earlier half back so the pair stays together.
                    for (done, _) in finished.drain(..) {
                        let back = if done.is_move {
                            transfer::move_back(
                                done.dst,
                                done.src,
                                done.id,
                                done.expect.size,
                                done.expect.mtime_ns,
                            )
                            .is_ok()
                        } else {
                            std::fs::remove_file(done.dst).is_ok()
                        };
                        if back {
                            jr.append(
                                Run::Apply,
                                done.id,
                                State::Skipped,
                                Some("put back because its Live Photo partner failed".into()),
                                None,
                            )?;
                            report.bytes = report.bytes.saturating_sub(done.expect.size);
                            if done.is_move {
                                report.moved -= 1;
                            } else {
                                report.copied -= 1;
                            }
                            note(
                                sink,
                                report,
                                true,
                                done,
                                "put back because its Live Photo partner failed".into(),
                            );
                        }
                    }
                    note(sink, report, false, x, msg);
                    let after: Vec<&Xfer<'_>> =
                        todo.iter().skip_while(|t| t.id != x.id).skip(1).collect();
                    for rest in after {
                        jr.append(
                            Run::Apply,
                            rest.id,
                            State::Skipped,
                            Some("an earlier step failed".into()),
                            None,
                        )?;
                        note(sink, report, true, rest, "an earlier step failed".into());
                    }
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    /// A `begin` with no `done`: if the work was in fact finished, record it and say so.
    fn finished_before_crash(&self, x: &Xfer<'_>, jr: &mut Journal) -> Result<bool> {
        let dst_ok = transfer::is_same_file(x.dst, x.expect.size, None);
        let src_there = x.src.symlink_metadata().is_ok();
        let finished = dst_ok && (if x.is_move { !src_there } else { src_there });
        if finished {
            let hash = full_hash(x.dst, x.expect.size).ok().map(|h| h.hash);
            jr.append_hashed(
                Run::Apply,
                x.id,
                State::Done,
                Some("recovered after an interruption".into()),
                None,
                hash,
            )?;
        }
        Ok(finished)
    }
}

fn note(sink: &EventSink, report: &mut ApplyReport, skipped: bool, x: &Xfer<'_>, reason: String) {
    sink.important(CoreEvent::Issue(FileIssue {
        path: x.src.to_path_buf(),
        kind: if skipped {
            IssueKind::Changed
        } else {
            IssueKind::Other
        },
        message: reason.clone(),
    }));
    let n = OpNote {
        op: x.id,
        path: x.src.to_path_buf(),
        reason,
    };
    if skipped {
        report.skipped.push(n);
    } else {
        report.failed.push(n);
    }
}
