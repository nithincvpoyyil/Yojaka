use std::path::Path;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};
use yojaka_core::dupes::{DupGroup, DupItem, DupOptions, DupReport, KeepRule, Tier, parse_rules};
use yojaka_core::plan::Plan;
use yojaka_core::{CancelToken, CoreEvent, Engine, EventSink, TaskKind, YojakaError};

use super::fmt::bytes;
use crate::cli::TierArg;
use crate::exit::Exit;

pub struct Args<'a> {
    pub path: Option<&'a Path>,
    pub tier: TierArg,
    pub similar: bool,
    pub similar_distance: Option<u32>,
    pub keep: &'a [String],
    pub plan_out: Option<&'a Path>,
    pub json: bool,
    pub local_index: bool,
}

pub fn run(engine: &Engine, a: Args<'_>) -> anyhow::Result<Exit> {
    let mut opts = match a.tier {
        TierArg::Exact => DupOptions::only(Tier::Exact),
        TierArg::Likely => DupOptions::only(Tier::Likely),
        TierArg::Name => DupOptions::only(Tier::Name),
        TierArg::Similar => DupOptions::only(Tier::Similar),
        TierArg::All => DupOptions::all(),
    };
    opts.similar |= a.similar;
    opts.similar_distance = a.similar_distance;
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
            engine.dupes(root, a.local_index, opts, &sink, &cancel)
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
        print_json(&report, &plan, a.plan_out, tier_name(a.tier))?;
    } else {
        print_text(&report, &plan, a.plan_out, opts);
    }
    Ok(if report.issues.is_empty() {
        Exit::Ok
    } else {
        Exit::Partial
    })
}

fn tier_name(t: TierArg) -> &'static str {
    match t {
        TierArg::Exact => "exact",
        TierArg::Likely => "likely",
        TierArg::Name => "name",
        TierArg::Similar => "similar",
        TierArg::All => "all",
    }
}

fn print_json(
    report: &DupReport,
    plan: &Plan,
    plan_out: Option<&Path>,
    tier: &str,
) -> anyhow::Result<()> {
    let groups: Vec<serde_json::Value> = report
        .groups
        .iter()
        .map(|g| {
            let mut v = serde_json::to_value(g)?;
            v["reclaim"] = g.reclaim().into();
            v["needs_review"] = (!g.tier.is_exact()).into();
            Ok(v)
        })
        .collect::<anyhow::Result<_>>()?;
    let out = serde_json::json!({
        "tier": tier,
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

fn print_text(report: &DupReport, plan: &Plan, plan_out: Option<&Path>, opts: DupOptions) {
    let s = &report.stats;
    if report.groups.is_empty() {
        if opts.exact && !(opts.likely || opts.name || opts.similar) {
            println!(
                "No exact duplicates found among {} files that share a size.",
                s.candidate_files
            );
        } else {
            println!("No duplicates found.");
        }
    }
    for g in &report.groups {
        print_group(g);
    }
    if s.exact_groups > 0 {
        println!(
            "\n{} groups of identical files; keeping one of each frees {} ({} files).",
            s.exact_groups,
            bytes(report.reclaim()),
            plan.ops.len()
        );
    }
    let review = s.likely_groups + s.similar_groups + s.name_groups;
    if review > 0 {
        let mut parts = Vec::new();
        for (n, what) in [
            (s.likely_groups, "likely"),
            (s.similar_groups, "similar"),
            (s.name_groups, "name"),
        ] {
            if n > 0 {
                parts.push(format!("{n} {what}"));
            }
        }
        println!(
            "\n{} ({}): these are different files that may be the same picture or clip, so \
             nothing in them is marked. Open the folder in Yojaka and review them on the \
             Duplicates screen.",
            if review == 1 {
                "1 group needs a look".to_string()
            } else {
                format!("{review} groups need a look")
            },
            parts.join(", ")
        );
    }
    if opts.exact || s.candidate_files > 0 {
        println!(
            "Read {} to compare {} candidate files ({} saved hashes reused).",
            bytes(s.bytes_read - s.picture_bytes),
            s.candidate_files,
            s.cached_hashes
        );
    }
    if opts.similar {
        println!(
            "Compared how {} pictures look ({} looked at now, {} of files; the rest remembered).",
            s.pictures_hashed + s.pictures_cached,
            s.pictures_hashed,
            bytes(s.picture_bytes)
        );
        if s.pictures_unsupported > 0 {
            println!(
                "{} pictures are in a format that cannot be compared by look.",
                s.pictures_unsupported
            );
        }
        if s.pictures_unusable > 0 {
            println!(
                "{} pictures could not be read as pictures, or are too plain to compare.",
                s.pictures_unusable
            );
        }
    }
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
    if !g.tier.is_exact() {
        println!(
            "\nGroup {} · {} · {} files ({})",
            g.id,
            g.tier.label(),
            g.members.len(),
            g.tier.reason()
        );
        for m in &g.members {
            println!("  review  {}", display(&m.primary().path));
            println!("          {}", facts(m));
        }
        return;
    }
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
            line.push_str(&video_note(m));
        }
        println!("  {}  {line}", if *del { "delete" } else { "keep  " });
    }
}

fn video_note(m: &DupItem) -> String {
    format!(
        "  (+ video {})",
        m.files[1]
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    )
}

/// What tells the copies of a non-exact group apart: size, pixels, date, camera, length.
fn facts(m: &DupItem) -> String {
    let f = m.primary();
    let mut parts = vec![bytes(f.size)];
    parts.extend(f.pixels_text());
    parts.extend(f.duration_ms.map(|d| super::fmt::duration(d.max(0) as u64)));
    parts.extend(f.taken_text());
    parts.extend(f.camera.clone());
    let mut line = parts.join(" · ");
    if m.is_pair() {
        line.push_str(&video_note(m));
    }
    line
}

fn display(p: &Path) -> String {
    p.display().to_string()
}
