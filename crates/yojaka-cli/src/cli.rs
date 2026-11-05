//! The full v1 command-line surface (tech spec §8). Most commands are stubs until their
//! iteration; `--help` is final from day one.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

pub const LONG_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("YOJAKA_GIT_SHA"),
    " ",
    env!("YOJAKA_BUILD_DATE"),
    ") ",
    env!("YOJAKA_TARGET")
);

#[derive(Debug, Parser)]
#[command(name = "yojaka", version = LONG_VERSION, about = "Index, de-duplicate and organize photos & videos")]
pub struct Cli {
    /// Folder or volume to open (default: current dir)
    pub path: Option<PathBuf>,
    /// Allow write operations (still requires dry-run + confirm)
    #[arg(long, global = true)]
    pub write: bool,
    /// Worker threads (default: auto)
    #[arg(long, global = true)]
    pub threads: Option<usize>,
    /// Keep the index inside the scanned folder instead of the app data dir
    #[arg(long, global = true)]
    pub local_index: bool,
    #[arg(long, global = true, env = "YOJAKA_HOME", hide = true)]
    pub home: Option<PathBuf>,
    #[command(subcommand)]
    pub cmd: Option<Cmd>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum KindArg {
    Image,
    Video,
    All,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ExportFmt {
    Csv,
    Json,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum TierArg {
    Exact,
    Likely,
    Name,
    Similar,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    /// Scan a folder and build or update its index
    Scan {
        path: Option<PathBuf>,
        #[arg(long, value_enum, default_value = "all")]
        kind: KindArg,
        /// Sniff file contents even for unknown extensions
        #[arg(long)]
        sniff_all: bool,
        /// Delete this folder's index and rebuild it from scratch
        #[arg(long)]
        force: bool,
    },
    /// Show library statistics from the index
    Stats {
        path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Show metadata for one file (Iteration 2)
    Info { file: PathBuf },
    /// List places found in the library (Iteration 2)
    Places {
        path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Export the index (Iteration 2)
    Export {
        path: Option<PathBuf>,
        #[arg(long, value_enum, default_value = "csv")]
        format: ExportFmt,
        /// Include GPS coordinates (left out by default)
        #[arg(long)]
        include_gps: bool,
    },
    /// Find duplicates (Iteration 4)
    Dupes {
        path: Option<PathBuf>,
        #[arg(long, value_enum, default_value = "exact")]
        tier: TierArg,
        #[arg(long)]
        keep: Vec<String>,
        #[arg(long)]
        plan_out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Organize into a folder template (Iteration 6)
    Organize {
        path: Option<PathBuf>,
        dest: Option<PathBuf>,
        #[arg(long)]
        template: Option<String>,
        #[arg(long)]
        copy: bool,
        #[arg(long = "move")]
        r#move: bool,
        #[arg(long)]
        infer_location: bool,
        #[arg(long)]
        plan_out: Option<PathBuf>,
    },
    /// Inspect plans (Iteration 5)
    Plan {
        #[command(subcommand)]
        sub: PlanCmd,
    },
    /// Execute a plan (Iteration 5)
    Apply {
        file: PathBuf,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        resume: bool,
    },
    /// Undo an applied plan (Iteration 5)
    Undo {
        plan_id: Option<String>,
        #[arg(long)]
        last: bool,
    },
    /// List executed plans (Iteration 5)
    History,
    /// Check the environment and print paths
    Doctor {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum PlanCmd {
    /// Show a saved plan
    Show {
        file: PathBuf,
        #[arg(long)]
        full_tree: bool,
    },
}

impl Cmd {
    /// (command name, iteration that implements it) for stubbed commands.
    pub fn stub_info(&self) -> Option<(&'static str, u8)> {
        Some(match self {
            Cmd::Scan { .. } | Cmd::Stats { .. } => return None,
            Cmd::Info { .. } => ("info", 2),
            Cmd::Places { .. } => ("places", 2),
            Cmd::Export { .. } => ("export", 2),
            Cmd::Dupes { .. } => ("dupes", 4),
            Cmd::Organize { .. } => ("organize", 6),
            Cmd::Plan { .. } => ("plan", 5),
            Cmd::Apply { .. } => ("apply", 5),
            Cmd::Undo { .. } => ("undo", 5),
            Cmd::History => ("history", 5),
            Cmd::Doctor { .. } => return None,
        })
    }
}
