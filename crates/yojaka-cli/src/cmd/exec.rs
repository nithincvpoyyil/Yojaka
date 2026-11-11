//! `plan show`, `apply`, `undo` and `history`.

use std::io::{BufRead, Write};
use std::path::Path;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};
use yojaka_core::exec::{self, ApplyOptions, DirTrash, SystemTrash, Trasher, history as hist};
use yojaka_core::plan::{OpMode, Plan};
use yojaka_core::{CancelToken, CoreEvent, Engine, EventSink, TaskKind, YojakaError};

use super::fmt::bytes;
use crate::exit::Exit;

pub struct ApplyArgs<'a> {
    pub file: &'a Path,
    pub write: bool,
    pub yes: bool,
    pub resume: bool,
    pub permanent: bool,
    pub trash_dir: Option<&'a Path>,
}

fn trasher(dir: Option<&Path>) -> Box<dyn Trasher> {
    match dir {
        Some(d) => Box::new(DirTrash(d.to_path_buf())),
        None => Box::new(SystemTrash),
    }
}

pub(crate) fn print_summary(plan: &Plan, full_tree: bool) {
    println!("Plan {}", plan.id);
    println!("  from:    {}", plan.root.display());
    if let Some(d) = &plan.dest {
        println!("  to:      {}", d.display());
    }
    let n = plan.file_ops();
    match plan.mode {
        OpMode::Copy | OpMode::Move => {
            let verb = if plan.mode == OpMode::Copy {
                "copy (originals stay)"
            } else {
                "move"
            };
            println!(
                "  action:  {verb} {n} files ({})",
                bytes(plan.summary.bytes)
            );
            if plan.summary.live_pairs > 0 {
                println!(
                    "  Live Photos: {} pairs (both halves go to the same folder)",
                    plan.summary.live_pairs
                );
            }
            let s = &plan.summary;
            if s.skipped > 0 {
                println!(
                    "  left alone: {} (already in place, or already at the destination)",
                    s.skipped
                );
            }
            if s.undated > 0 {
                println!("  {} files have no date and go to _undated", s.undated);
            }
            if s.no_location > 0 {
                println!("  {} files have no location", s.no_location);
            }
            if s.inferred > 0 {
                println!("  {} files use an inferred place (≈)", s.inferred);
            }
        }
        _ => {
            let verb = if plan.is_permanent() {
                "delete for good"
            } else {
                "move to the Trash"
            };
            println!(
                "  action:  {verb} {n} files ({}) from {} duplicate groups",
                bytes(plan.summary.bytes),
                plan.summary.groups
            );
            if plan.summary.live_pairs > 0 {
                println!(
                    "  Live Photos: {} pairs (both halves go together)",
                    plan.summary.live_pairs
                );
            }
            let lookalikes = plan.lookalike_ops();
            if lookalikes > 0 {
                println!(
                    "  note:    {lookalikes} of these are not identical to the copy that stays (matched by eye)"
                );
            }
        }
    }
    for w in &plan.warnings {
        println!("  warning: {}", w.message);
    }
    let organize = plan.dest.is_some();
    let tree = if organize {
        plan.destination_tree()
    } else {
        plan.removal_tree()
    };
    println!(
        "  {}",
        if organize {
            "files by new folder:"
        } else {
            "files removed, by folder:"
        }
    );
    let lines = if full_tree {
        tree.glimpse(usize::MAX, usize::MAX)
    } else {
        tree.glimpse(40, 8)
    };
    for l in lines {
        println!(
            "    {}{}  {}{} files ({})",
            "  ".repeat(l.depth),
            l.label,
            if organize { "+" } else { "−" },
            l.files,
            bytes(l.bytes)
        );
    }
}

pub fn plan_show(file: &Path, full_tree: bool) -> anyhow::Result<Exit> {
    let plan = Plan::load(file)?;
    print_summary(&plan, full_tree);
    Ok(Exit::Ok)
}

fn ask(prompt: &str) -> anyhow::Result<String> {
    print!("{prompt}");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

fn spinner(label: &'static str) -> ProgressBar {
    let bar = ProgressBar::new(0);
    bar.set_style(
        ProgressStyle::with_template("{msg} {pos}/{len} {bar:30}")
            .unwrap_or_else(|_| ProgressStyle::default_bar()),
    );
    bar.set_message(label);
    bar.enable_steady_tick(Duration::from_millis(100));
    bar
}

fn cancel_on_ctrl_c() -> anyhow::Result<CancelToken> {
    let cancel = CancelToken::new();
    let c = cancel.clone();
    ctrlc::set_handler(move || c.cancel())?;
    Ok(cancel)
}

pub fn apply(engine: &Engine, a: &ApplyArgs<'_>) -> anyhow::Result<Exit> {
    let mut plan = Plan::load(a.file)?;
    if a.permanent {
        plan = plan.into_permanent();
    }
    let opts = ApplyOptions {
        permanent: a.permanent,
        resume: a.resume,
    };
    // Problems with the plan itself are reported before anything is asked or changed.
    if let Err(e) = exec::check_plan(&plan, opts) {
        eprintln!("error: {e}");
        return Ok(Exit::Usage);
    }
    print_summary(&plan, false);

    if !a.write {
        println!("\nDry run: nothing was changed. Add --write to apply this plan.");
        return Ok(Exit::Ok);
    }
    if a.yes && plan.is_permanent() {
        println!("\n--yes does not skip the confirmation for a permanent delete.");
    }
    let skip_ask = a.yes && !plan.is_permanent() && !plan.needs_typed_confirmation();
    if !skip_ask {
        let phrase = plan.confirm_phrase();
        let ok = if plan.needs_typed_confirmation() {
            ask(&format!("\nType \"{phrase}\" to continue: "))? == phrase
        } else {
            ask(&format!("\n{} (y/N): ", phrase_question(&plan)))?.eq_ignore_ascii_case("y")
        };
        if !ok {
            println!("Not confirmed. Nothing was changed.");
            return Ok(Exit::Error);
        }
    }

    let cancel = cancel_on_ctrl_c()?;
    let bar = spinner("applying");
    bar.set_length(plan.ops.len() as u64);
    let (sink, rx) = EventSink::channel(1024);
    let t = trasher(a.trash_dir);
    let result = std::thread::scope(|s| {
        let worker = s.spawn(|| {
            let sink = sink;
            exec::apply(&plan, &engine.paths, t.as_ref(), opts, &sink, &cancel)
        });
        for ev in rx.iter() {
            if let CoreEvent::Progress {
                task: TaskKind::Execute,
                done,
                ..
            } = ev
            {
                bar.set_position(done);
            }
        }
        worker
            .join()
            .unwrap_or_else(|_| Err(YojakaError::Internal("the executor panicked".into())))
    });
    bar.finish_and_clear();
    let report = match result {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return Ok(Exit::Error);
        }
    };

    if plan.dest.is_some() {
        println!(
            "{} files moved, {} copied, {} folders created ({}).",
            report.moved,
            report.copied,
            report.created,
            bytes(report.bytes)
        );
        if report.already_there > 0 {
            println!("{} were already in place.", report.already_there);
        }
    } else {
        let what = if plan.is_permanent() {
            "deleted"
        } else {
            "moved to the Trash"
        };
        println!(
            "{} files {what} ({} freed).",
            report.trashed + report.deleted,
            bytes(report.bytes)
        );
    }
    if report.already_done > 0 {
        println!(
            "{} were already done by an earlier run.",
            report.already_done
        );
    }
    for n in &report.skipped {
        println!("skipped {}: {}", n.path.display(), n.reason);
    }
    for n in &report.failed {
        println!("FAILED  {}: {}", n.path.display(), n.reason);
    }
    if report.cancelled {
        println!(
            "Stopped. Finish it with: yojaka apply {} --resume --write",
            a.file.display()
        );
    } else if report.deleted == 0 {
        println!("Undo it with: yojaka undo {} --write", plan.id);
    }
    Ok(if report.is_clean() {
        Exit::Ok
    } else {
        Exit::Partial
    })
}

fn phrase_question(plan: &Plan) -> String {
    let what = match plan.mode {
        OpMode::Copy => "Copy",
        OpMode::Move => "Move",
        _ => "Move to the Trash:",
    };
    format!(
        "{what} {} files ({})?",
        plan.file_ops(),
        bytes(plan.summary.bytes)
    )
}

pub fn undo(
    engine: &Engine,
    plan_id: Option<&str>,
    last: bool,
    write: bool,
) -> anyhow::Result<Exit> {
    let run = match (plan_id, last) {
        (Some(id), _) => hist::find(&engine.paths, id)?,
        (None, true) => match hist::list(&engine.paths)?.into_iter().next() {
            Some(r) => r,
            None => {
                eprintln!("error: nothing has been run yet");
                return Ok(Exit::Error);
            }
        },
        (None, false) => {
            eprintln!("error: give a run id (see `yojaka history`) or --last");
            return Ok(Exit::Usage);
        }
    };
    println!(
        "Run {}: {} of {} files, {} to put back from {}",
        run.plan_id,
        run.done,
        run.ops,
        bytes(run.bytes),
        run.root.display()
    );
    if !write {
        println!("Preview only: nothing was changed. Add --write to put the files back.");
        return Ok(Exit::Ok);
    }
    let cancel = cancel_on_ctrl_c()?;
    let (sink, rx) = EventSink::channel(1024);
    let bar = spinner("restoring");
    bar.set_length(run.done as u64);
    let result = std::thread::scope(|s| {
        let worker = s.spawn(|| {
            let sink = sink;
            exec::undo(run.plan_id, &engine.paths, &sink, &cancel)
        });
        for ev in rx.iter() {
            if let CoreEvent::Progress {
                task: TaskKind::Undo,
                done,
                ..
            } = ev
            {
                bar.set_position(done);
            }
        }
        worker
            .join()
            .unwrap_or_else(|_| Err(YojakaError::Internal("undo panicked".into())))
    });
    bar.finish_and_clear();
    let report = match result {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return Ok(Exit::Error);
        }
    };
    println!(
        "{} files put back ({}).",
        report.restored,
        bytes(report.bytes)
    );
    if report.already_undone > 0 {
        println!("{} had been put back before.", report.already_undone);
    }
    for n in report.not_undoable.iter().chain(&report.failed) {
        println!("not restored {}: {}", n.path.display(), n.reason);
    }
    Ok(if report.is_clean() {
        Exit::Ok
    } else {
        Exit::Partial
    })
}

pub fn history(engine: &Engine, json: bool) -> anyhow::Result<Exit> {
    let runs = hist::list(&engine.paths)?;
    if json {
        let v: Vec<_> = runs
            .iter()
            .map(|r| {
                serde_json::json!({
                    "plan_id": r.plan_id, "started": r.started, "root": r.root,
                    "ops": r.ops, "done": r.done, "skipped": r.skipped, "failed": r.failed,
                    "undone": r.undone, "bytes": r.bytes, "status": r.status.label(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(Exit::Ok);
    }
    if runs.is_empty() {
        println!("Nothing has been run yet.");
        return Ok(Exit::Ok);
    }
    for r in runs {
        println!(
            "{}  {}  {:>13}  {} of {} files  {}  {}",
            r.plan_id,
            when(r.started),
            r.status.label(),
            r.done,
            r.ops,
            bytes(r.bytes),
            r.root.display()
        );
    }
    Ok(Exit::Ok)
}

/// `2026-11-09 14:03` in the local time zone (same as the History screen).
fn when(ts: i64) -> String {
    yojaka_core::meta::format_run_time(ts)
}
