#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use fixturegen::{Spec, generate};

#[derive(Debug, Parser)]
#[command(
    name = "fixturegen",
    about = "Generate a deterministic photo tree and manifest"
)]
struct Args {
    /// Output directory (must be empty or a previous fixturegen output)
    out_dir: PathBuf,
    /// Unique media files before duplicates
    #[arg(long, default_value_t = 1000)]
    files: usize,
    #[arg(long, default_value_t = 42)]
    seed: u64,
    /// Max folder nesting
    #[arg(long, default_value_t = 4)]
    depth: usize,
    /// Fraction duplicated (exact copies, renamed, other folders)
    #[arg(long, default_value_t = 0.10)]
    dupes: f64,
    /// Fraction corrupt (magic bytes then truncated body)
    #[arg(long, default_value_t = 0.01)]
    corrupt: f64,
    /// Only `image` in v1
    #[arg(long, default_value = "image")]
    kinds: String,
    /// Fraction with EXIF GPS
    #[arg(long, default_value_t = 0.6)]
    gps: f64,
    /// Fraction of files encoded as HEIC through `sips` (cached; needs macOS)
    #[arg(long, default_value_t = 0.0)]
    heic: f64,
    #[arg(long, default_value_t = 5)]
    hardlinks: usize,
    #[arg(long, default_value_t = 5)]
    symlinks: usize,
    /// Non-media files
    #[arg(long, default_value_t = 200)]
    noise: usize,
    /// Extra synthetic videos, as a fraction of --files (e.g. 0.1)
    #[arg(long, default_value_t = 0.0)]
    videos: f64,
    /// Live Photo pairs (JPEG + MOV) and decoys
    #[arg(long, default_value_t = 0)]
    live_photos: usize,
}

fn main() -> ExitCode {
    let a = Args::parse();
    if a.kinds != "image" {
        eprintln!("error: only --kinds image is supported; use --videos / --live-photos for video");
        return ExitCode::from(2);
    }
    let spec = Spec {
        files: a.files,
        seed: a.seed,
        depth: a.depth,
        dupes: a.dupes,
        corrupt: a.corrupt,
        gps: a.gps,
        heic: a.heic,
        hardlinks: a.hardlinks,
        symlinks: a.symlinks,
        noise: a.noise,
        videos: a.videos,
        live_photos: a.live_photos,
    };
    match generate(&spec, &a.out_dir) {
        Ok(m) => {
            for w in &m.warnings {
                eprintln!("warning: {w}");
            }
            println!(
                "wrote {} entries ({} to be indexed) to {}",
                m.files.len(),
                m.expected.indexed_count,
                a.out_dir.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
