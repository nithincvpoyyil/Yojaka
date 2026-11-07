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

/// Printed by `yojaka --about`. The GeoNames text is a licence requirement (CC BY 4.0): keep the
/// credit, the licence link and the note that the data was changed.
pub const ABOUT: &str = concat!(
    "Yojaka ",
    env!("CARGO_PKG_VERSION"),
    "\nIndex, de-duplicate and organize photos & videos.\n",
    "Released under the MIT License.\n",
    "\n",
    "Third-party data\n",
    "  Place names come from GeoNames (https://www.geonames.org), licensed under\n",
    "  Creative Commons Attribution 4.0 (https://creativecommons.org/licenses/by/4.0/).\n",
    "  The data (populated places with 1,000 or more people, plus country and\n",
    "  region names) was converted into a compact format so places can be looked up\n",
    "  offline; the names are used as provided. GeoNames does not endorse Yojaka.\n",
    "  Coordinates never leave your machine.\n"
);

#[derive(Debug, Parser)]
#[command(name = "yojaka", version = LONG_VERSION, about = "Index, de-duplicate and organize photos & videos")]
pub struct Cli {
    /// Folder or volume to open (default: current dir)
    pub path: Option<PathBuf>,
    /// Allow write operations (still requires dry-run + confirm)
    #[arg(long, global = true)]
    pub write: bool,
    /// Scan on open if the last scan is over an hour old (never clears anything)
    #[arg(long)]
    pub refresh: bool,
    /// Show the license and the credits for the data Yojaka uses, then exit
    #[arg(long)]
    pub about: bool,
    /// Ask first, then forget what is saved about the folder and scan it from scratch (slower)
    #[arg(long)]
    pub force: bool,
    /// Worker threads (default: auto)
    #[arg(long, global = true)]
    pub threads: Option<usize>,
    /// For tests and tooling: keep the saved data inside the scanned folder
    #[arg(long, global = true, hide = true)]
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
    /// Scan a folder; only new and changed files are read
    Scan {
        path: Option<PathBuf>,
        #[arg(long, value_enum, default_value = "all")]
        kind: KindArg,
        /// Sniff file contents even for unknown extensions
        #[arg(long)]
        sniff_all: bool,
        /// Forget what is saved about this folder and scan it from scratch
        #[arg(long)]
        force: bool,
        /// Only index files; skip reading metadata, Live Photo pairing and places
        #[arg(long)]
        no_metadata: bool,
    },
    /// Show statistics for a scanned folder
    Stats {
        path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Show metadata for one file, including GPS and place
    Info {
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// List the places (countries and cities) found in the library
    Places {
        path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Export the file list as CSV or JSON (GPS is left out unless --include-gps)
    Export {
        path: Option<PathBuf>,
        #[arg(long, value_enum, default_value = "csv")]
        format: ExportFmt,
        /// Include GPS coordinates (left out by default)
        #[arg(long)]
        include_gps: bool,
        /// Write to this new file instead of stdout (an existing file is never overwritten)
        #[arg(long)]
        out: Option<PathBuf>,
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
            Cmd::Info { .. } | Cmd::Places { .. } | Cmd::Export { .. } => return None,
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
