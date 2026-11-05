use std::path::Path;

use yojaka_core::Engine;

use super::fmt::bytes;
use crate::exit::Exit;

pub fn run(
    engine: &Engine,
    path: Option<&Path>,
    json: bool,
    local_index: bool,
) -> anyhow::Result<Exit> {
    let root = path.unwrap_or_else(|| Path::new("."));
    let stats = engine.stats(root, local_index)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&stats)?);
        return Ok(Exit::Ok);
    }

    println!("{}", stats.root.display());
    if let Some(s) = &stats.last_scan {
        println!(
            "  last scan: {} ({} files seen, {} issues)",
            s.status, s.files_seen, s.errors
        );
    }
    let total = stats.photos.count + stats.videos.count;
    println!(
        "\n  Photos  {:>9}  {:>10}",
        stats.photos.count,
        bytes(stats.photos.bytes)
    );
    println!(
        "  Videos  {:>9}  {:>10}",
        stats.videos.count,
        bytes(stats.videos.bytes)
    );
    println!(
        "  Total   {:>9}  {:>10}",
        total,
        bytes(stats.photos.bytes + stats.videos.bytes)
    );
    if stats.cloud_only > 0 {
        println!("  Cloud-only (not downloaded): {}", stats.cloud_only);
    }
    println!("\n  By format");
    for f in &stats.formats {
        println!(
            "    {:<10} {:<6} {:>9}  {:>10}",
            f.format,
            f.kind,
            f.count,
            bytes(f.bytes)
        );
    }
    println!("\n  Errors: {}", stats.errors);
    println!("  (hard links to the same file are counted once)");
    Ok(Exit::Ok)
}
