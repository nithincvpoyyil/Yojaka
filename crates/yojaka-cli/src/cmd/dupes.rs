use std::path::Path;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};
use yojaka_core::dupes::{DupGroup, DupReport, KeepRule, parse_rules};
use yojaka_core::plan::Plan;
use yojaka_core::{CancelToken, CoreEvent, Engine, EventSink, TaskKind, YojakaError};

use super::fmt::bytes;
use crate::cli::TierArg;
use crate::exit::Exit;

pub struct Args<'a> {
    pub path: Option<&'a Path>,
    pub tier: TierArg,
    pub keep: &'a [String],
    pub plan_out: Option<&'a Path>,
    pub json: bool,
    pub local_index: bool,
}

pub fn run(engine: &Engine, a: Args<'_>) -> anyhow::Result<Exit> {
    if !matches!(a.tier, TierArg::Exact) {
        return Err(
            YojakaError::NotYet("the likely, name and similar tiers", "a later iteration").into(),
        );
    }
    let mut rules = match parse_rules(a.keep) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return Ok(Exit::Usage);
        }
    };
    // Relative preferred folders are resolved against the current directory.
    for r in &mut rules {
        if let KeepRule::PreferPath(p) = r {
            *p = p.canonicalize().unwrap_or_else(|_| p.clone());
        }
    }
    if let Some(out) = a.plan_out
        && out.exists()
    {
        eprintln!(
            "error: {} already exists; Yojaka never overwrites a file",
            out.display()
        );
        return Ok(Exit::Usage);
    }

    let root = a.path.unwrap_or_else(|| Path::new("."));
    let cancel = CancelToken::new();
    {
        let cancel = cancel.clone();
        ctrlc::set_handler(move || cancel.cancel())?;
    }
    let bar = ProgressBar::new_spinner();
    bar.set_style(
        ProgressStyle::with_template("{spinner} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );
    bar.enable_steady_tick(Duration::from_millis(100));

    let (sink, rx) = EventSink::channel(1024);
    let result = std::thread::scope(|s| {
        let worker = s.spawn(|| {
            let sink = sink;
            engine.dupes(root, a.local_index, &sink, &cancel)
        });
        for ev in rx.iter() {
            if let CoreEvent::Progress {
                task: TaskKind::Hash,
                done,
                total,
                bytes: b,
                ..
            } = ev
            {
                bar.set_message(format!(
                    "comparing files {done}/{} ({} read)",
                    total.map_or_else(|| "?".into(), |t| t.to_string()),
                    bytes(b)
                ));
            }
        }
        worker
            .join()
            .unwrap_or_else(|_| Err(YojakaError::Internal("duplicate search panicked".into())))
    });
    bar.finish_and_clear();

    let mut report = match result {
        Ok(r) => r,
        Err(YojakaError::Cancelled) => {
            eprintln!("cancelled; hashes found so far are saved for next time");
            return Ok(Exit::Partial);
        }
        Err(e) => return Err(e.into()),
    };
    report.apply_rules(&rules);

    let plan = Plan::from_dupes(
        &root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
        &report.groups,
    );
    if let Some(out) = a.plan_out {
        plan.save(out)?;
    }

    if a.json {
        print_json(&report, &plan, a.plan_out)?;
    } else {
        print_text(&report, &plan, a.plan_out);
    }
    Ok(if report.issues.is_empty() {
        Exit::Ok
    } else {
        Exit::Partial
    })
}

fn print_json(report: &DupReport, plan: &Plan, plan_out: Option<&Path>) -> anyhow::Result<()> {
    let groups: Vec<serde_json::Value> = report
        .groups
        .iter()
        .map(|g| {
            let mut v = serde_json::to_value(g)?;
            v["reclaim"] = g.reclaim().into();
            Ok(v)
        })
        .collect::<anyhow::Result<_>>()?;
    let out = serde_json::json!({
        "tier": "exact",
        "stats": report.stats,
        "groups": groups,
        "reclaim": report.reclaim(),
        "plan_summary": plan.summary,
        "plan_warnings": plan.warnings,
        "plan_file": plan_out,
        "issues": report.issues,
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

fn print_text(report: &DupReport, plan: &Plan, plan_out: Option<&Path>) {
    let s = &report.stats;
    if report.groups.is_empty() {
        println!(
            "No exact duplicates found among {} files that share a size.",
            s.candidate_files
        );
    }
    for g in &report.groups {
        print_group(g);
    }
    if !report.groups.is_empty() {
        let files: usize = plan.ops.len();
        println!(
            "\n{} groups of identical files; keeping one of each frees {} ({} files).",
            report.groups.len(),
            bytes(report.reclaim()),
            files
        );
    }
    println!(
        "Read {} to compare {} candidate files ({} saved hashes reused).",
        bytes(s.bytes_read),
        s.candidate_files,
        s.cached_hashes
    );
    if s.hardlinks_skipped > 0 {
        println!(
            "{} extra names of the same file (hard links) were ignored: they use no extra space.",
            s.hardlinks_skipped
        );
    }
    for i in &report.issues {
        eprintln!("skipped {}: {}", i.path.display(), i.message);
    }
    for w in &plan.warnings {
        eprintln!("warning: {}", w.message);
    }
    match plan_out {
        Some(p) => println!("Plan saved to {}. Nothing has been changed.", p.display()),
        None => println!(
            "Preview only: nothing has been deleted. Add --plan-out FILE to save this as a plan."
        ),
    }
}

fn print_group(g: &DupGroup) {
    println!(
        "\nGroup {} · {} copies · {} each · frees {}",
        g.id,
        g.members.len(),
        bytes(g.members[0].total_size()),
        bytes(g.reclaim())
    );
    for (m, del) in g.members.iter().zip(&g.delete) {
        let mut line = display(&m.primary().path);
        if m.is_pair() {
            line.push_str(&format!(
                "  (+ video {})",
                m.files[1]
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ));
        }
        println!("  {}  {line}", if *del { "delete" } else { "keep  " });
    }
}

fn display(p: &Path) -> String {
    p.display().to_string()
}
