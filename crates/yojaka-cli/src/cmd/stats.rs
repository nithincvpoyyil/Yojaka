use std::path::Path;

use yojaka_core::Engine;

use super::fmt::{bytes, duration};
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
    if stats.video.with_duration > 0 {
        println!(
            "\n  Video: {} in total ({} with known length)",
            duration(stats.video.total_duration_ms),
            stats.video.with_duration
        );
        if !stats.video.codecs.is_empty() {
            let c: Vec<String> = stats
                .video
                .codecs
                .iter()
                .map(|c| format!("{} {}", c.codec, c.count))
                .collect();
            println!("    codecs: {}", c.join(", "));
        }
        println!("    longest:");
        for v in stats.video.longest.iter().take(5) {
            println!(
                "      {:>10}  {}",
                duration(v.duration_ms),
                v.path.display()
            );
        }
        println!("    largest:");
        for v in stats.video.largest.iter().take(5) {
            println!("      {:>10}  {}", bytes(v.size_bytes), v.path.display());
        }
    }
    if stats.live_photo_pairs > 0 {
        println!("\n  Live Photos: {} pairs", stats.live_photo_pairs);
    }
    if !stats.by_year.is_empty() {
        println!("\n  By year");
        let max = stats
            .by_year
            .iter()
            .map(|y| y.count)
            .max()
            .unwrap_or(1)
            .max(1);
        for y in &stats.by_year {
            let bar = "█".repeat(((y.count * 30).div_ceil(max)) as usize);
            println!("    {}  {:>7}  {bar}", y.year, y.count);
        }
    }
    println!("    undated (no embedded date): {}", stats.undated);
    if !stats.cameras.is_empty() {
        println!("\n  Top cameras");
        for c in stats.cameras.iter().take(5) {
            println!("    {:<28} {:>7}", c.camera, c.count);
        }
    }
    let loc = &stats.location;
    if loc.with_gps > 0 || loc.with_place > 0 {
        println!(
            "\n  Location: {} with a place, {} without ({} with GPS)",
            loc.with_place, loc.without_place, loc.with_gps
        );
        for c in loc.countries.iter().take(5) {
            println!("    {:<28} {:>7}", c.country, c.count);
        }
        for c in loc.cities.iter().take(5) {
            println!(
                "    {:<28} {:>7}",
                format!("{}, {}", c.city, c.country),
                c.count
            );
        }
    }
    println!("\n  Largest files");
    for f in stats.largest_files.iter().take(5) {
        println!("    {:>10}  {}", bytes(f.size_bytes), f.path.display());
    }
    if stats.metadata_pending > 0 {
        println!(
            "\n  {} files have not had their metadata read yet (run `yojaka scan` again)",
            stats.metadata_pending
        );
    }
    if stats.metadata_errors > 0 {
        println!(
            "\n  {} files have unreadable metadata (damaged or truncated)",
            stats.metadata_errors
        );
    }
    println!("\n  Errors: {}", stats.errors);
    println!("  (hard links to the same file are counted once)");
    Ok(Exit::Ok)
}
