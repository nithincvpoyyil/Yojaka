use std::path::Path;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};
use yojaka_core::model::MediaKind;
use yojaka_core::scan::{ScanOptions, ScanStatus};
use yojaka_core::{CancelToken, CoreEvent, Engine, EventSink, FileIssue};

use super::fmt::bytes;
use crate::cli::KindArg;
use crate::exit::Exit;

pub fn run(
    engine: &Engine,
    path: Option<&Path>,
    kind: KindArg,
    sniff_all: bool,
    local_index: bool,
    force: bool,
) -> anyhow::Result<Exit> {
    let root = path.unwrap_or_else(|| Path::new("."));
    let kinds = match kind {
        KindArg::Image => vec![MediaKind::Image],
        KindArg::Video => vec![MediaKind::Video],
        KindArg::All => vec![],
    };
    let opts = ScanOptions {
        root: root.to_path_buf(),
        kinds,
        sniff_all,
        local_index,
        force,
    };

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
    let mut issues: Vec<FileIssue> = Vec::new();
    let started = std::time::Instant::now();

    let result = std::thread::scope(|s| {
        // `sink` moves into the worker, so the channel closes when the scan returns.
        let worker = s.spawn(|| {
            let sink = sink;
            engine.scan(&opts, &sink, &cancel)
        });
        for ev in rx.iter() {
            match ev {
                CoreEvent::Progress {
                    done,
                    bytes: b,
                    current,
                    ..
                } => {
                    let secs = started.elapsed().as_secs_f64().max(0.001);
                    bar.set_message(format!(
                        "{done} files · {} · {:.0} files/s · {}",
                        bytes(b),
                        done as f64 / secs,
                        current.map(|p| p.display().to_string()).unwrap_or_default()
                    ));
                }
                CoreEvent::Issue(i) if issues.len() < 5 => issues.push(i),
                _ => {}
            }
        }
        worker.join()
    });
    bar.finish_and_clear();
    let summary = match result {
        Ok(r) => r?,
        Err(_) => anyhow::bail!("scan thread panicked"),
    };

    let secs = summary.elapsed_ms as f64 / 1000.0;
    if summary.status == ScanStatus::Partial {
        println!("Scan cancelled: partial results kept (nothing was removed from the index).");
    }
    if force {
        println!("Index flushed and rebuilt from scratch.");
    }
    println!("Scanned {} in {secs:.1}s", root.display());
    println!(
        "  {} media files indexed ({}) from {} files seen",
        summary.files_indexed,
        bytes(summary.bytes),
        summary.files_seen
    );
    println!(
        "  {} new or changed, {} unchanged, {} removed from the index",
        summary.new_or_changed, summary.unchanged, summary.pruned
    );
    if summary.cloud_only > 0 {
        println!("  {} cloud-only files (not downloaded)", summary.cloud_only);
    }
    if summary.issues > 0 {
        println!("  {} issues:", summary.issues);
        for i in &issues {
            println!("    {}: {}", i.path.display(), i.message);
        }
        if summary.issues as usize > issues.len() {
            println!(
                "    … and {} more (see the log file)",
                summary.issues as usize - issues.len()
            );
        }
    }
    if summary.permission_denied > 0 {
        println!(
            "  Some folders were not readable. If they are in a protected area, grant your terminal\n  Full Disk Access: System Settings → Privacy & Security → Full Disk Access."
        );
    }
    Ok(if summary.status == ScanStatus::Partial {
        Exit::Partial
    } else {
        Exit::Ok
    })
}
