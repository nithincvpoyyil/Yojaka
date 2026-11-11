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
    /// Write a detailed debug log (threads, steps, timings, errors) to the logs folder.
    /// The file name is printed at start. Without this flag no log is written.
    #[arg(long, global = true)]
    pub debug: bool,
    /// Worker threads (default: auto)
    #[arg(long, global = true)]
    pub threads: Option<usize>,
    /// For tests and tooling: keep the saved data inside the scanned folder
    #[arg(long, global = true, hide = true)]
    pub local_index: bool,
    /// For tests and tooling: use this folder as the Trash instead of the real one
    #[arg(long, global = true, hide = true)]
    pub trash_dir: Option<PathBuf>,
    #[arg(long, global = true, env = "YOJAKA_HOME", hide = true)]
    pub home: Option<PathBuf>,
    #[command(subcommand)]
    pub cmd: Option<Cmd>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum PresetArg {
    Date,
    Place,
    PlaceDate,
    KindDate,
}

impl PresetArg {
    pub fn template(self) -> &'static str {
        let i = match self {
            PresetArg::Date => 0,
            PresetArg::Place => 1,
            PresetArg::PlaceDate => 2,
            PresetArg::KindDate => 3,
        };
        yojaka_core::organize::PRESETS[i].1
    }
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
    /// Identical files
    Exact,
    /// Same date, camera and picture size, but different files
    Likely,
    /// Same name apart from a copy suffix, and nearly the same size
    Name,
    /// Pictures that look alike (reads every picture once)
    Similar,
    /// Exact, likely and name (add --similar to also compare how pictures look)
    All,
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
        /// Only photos or only videos
        #[arg(long, value_enum, default_value = "all")]
        kind: KindArg,
        /// Only files taken in this year
        #[arg(long)]
        year: Option<i32>,
        /// Only files whose name, camera or place contains this text (as `/` in the file browser)
        #[arg(long, value_name = "TEXT")]
        find: Option<String>,
        /// Only files in this folder (inside the scanned one)
        #[arg(long, value_name = "DIR")]
        folder: Option<PathBuf>,
    },
    /// Find duplicate files and preview what could be removed (changes nothing)
    Dupes {
        path: Option<PathBuf>,
        /// How sure a match must be. Only exact copies are ever marked for removal here;
        /// the other tiers are listed for you to review in the app
        #[arg(long, value_enum, default_value = "exact")]
        tier: TierArg,
        /// Also compare how pictures look, to find resized or re-saved copies. Slower the
        /// first time: every JPEG, PNG, GIF, WebP, TIFF and BMP picture is read once
        #[arg(long)]
        similar: bool,
        /// With --similar: how different two pictures may be (0 = same look, 6 = default, 32 = most)
        #[arg(long, value_name = "BITS", value_parser = clap::value_parser!(u32).range(0..=32))]
        similar_distance: Option<u32>,
        /// Which copy to keep, in order of preference, e.g. `prefer-path:~/Pictures,oldest-modified,shortest-path`
        #[arg(long, value_delimiter = ',')]
        keep: Vec<String>,
        /// Save the proposed removals as a plan file (never overwrites; nothing is deleted)
        #[arg(long)]
        plan_out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Plan how to arrange the scanned files into folders by a template. Nothing is changed:
    /// save the plan with --plan-out, then run it with `yojaka --write apply`
    Organize {
        /// Scanned folder to read from (`.` for the current folder)
        path: PathBuf,
        /// Folder the files go to (created if needed)
        dest: PathBuf,
        /// Folder pattern, e.g. `{year}/{month:02}/{name}` (default from the config file).
        /// Placeholders: year month day camera make model format ext name stem parent kind
        /// country country_code state district city
        #[arg(long, conflicts_with = "preset")]
        template: Option<String>,
        /// A ready-made pattern
        #[arg(long, value_enum)]
        preset: Option<PresetArg>,
        /// Copy the files (originals stay). This is the default
        #[arg(long, conflicts_with = "move")]
        copy: bool,
        /// Move the files instead of copying them
        #[arg(long = "move")]
        r#move: bool,
        /// Give files without GPS the place of a geotagged file of the same camera taken within
        /// two hours; marked as inferred in the plan
        #[arg(long)]
        infer_location: bool,
        /// Save the plan to this new file (an existing file is never overwritten)
        #[arg(long)]
        plan_out: Option<PathBuf>,
        /// Print every folder instead of the short overview
        #[arg(long)]
        full_tree: bool,
    },
    /// Inspect plans
    Plan {
        #[command(subcommand)]
        sub: PlanCmd,
    },
    /// Run a saved plan: moves the listed files to the Trash. Shows a dry run unless --write is given
    Apply {
        file: PathBuf,
        /// Skip the y/n question for small plans (large plans still need the phrase typed)
        #[arg(long)]
        yes: bool,
        /// Finish a plan that was started and interrupted
        #[arg(long)]
        resume: bool,
        /// Delete for good instead of using the Trash. Cannot be undone; always asks for a typed phrase
        #[arg(long)]
        permanent: bool,
    },
    /// Put back the files a run moved to the Trash. Shows a preview unless --write is given
    Undo {
        /// Run id (or the start of it) from `yojaka history`
        plan_id: Option<String>,
        /// The most recent run
        #[arg(long)]
        last: bool,
    },
    /// List what has been run, newest first
    History {
        #[arg(long)]
        json: bool,
    },
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
