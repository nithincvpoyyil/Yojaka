use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::Path;

use yojaka_core::Engine;
use yojaka_core::browse::{BrowseQuery, KindFilter};
use yojaka_core::export::{ExportFormat, ExportOptions};

use crate::cli::{ExportFmt, KindArg};
use crate::exit::Exit;

/// What to export: GPS columns and the same filters as the file browser.
pub struct Filter {
    pub include_gps: bool,
    pub kind: KindArg,
    pub year: Option<i32>,
    pub find: Option<String>,
    pub folder: Option<std::path::PathBuf>,
}

pub fn run(
    engine: &Engine,
    path: Option<&Path>,
    format: ExportFmt,
    filter: Filter,
    out: Option<&Path>,
    local_index: bool,
) -> anyhow::Result<Exit> {
    let include_gps = filter.include_gps;
    let root = path.unwrap_or_else(|| Path::new("."));
    let format = match format {
        ExportFmt::Csv => ExportFormat::Csv,
        ExportFmt::Json => ExportFormat::Json,
    };
    let root_dir = path.unwrap_or_else(|| Path::new("."));
    let opts = ExportOptions {
        include_gps,
        year: filter.year,
        query: BrowseQuery {
            kind: match filter.kind {
                KindArg::Image => KindFilter::Photos,
                KindArg::Video => KindFilter::Videos,
                KindArg::All => KindFilter::All,
            },
            text: filter.find.unwrap_or_default(),
            // A relative folder is taken inside the scanned one.
            folder: filter
                .folder
                .map(|f| if f.is_absolute() { f } else { root_dir.join(f) }),
            ..BrowseQuery::default()
        },
    };
    match out {
        Some(file) => {
            // create_new: an existing file is never overwritten.
            let f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(file)
                .map_err(|e| {
                    if e.kind() == std::io::ErrorKind::AlreadyExists {
                        anyhow::anyhow!(
                            "{} already exists; Yojaka never overwrites files",
                            file.display()
                        )
                    } else {
                        anyhow::anyhow!("cannot create {}: {e}", file.display())
                    }
                })?;
            let mut w = BufWriter::new(f);
            let n = engine.export(root, local_index, format, opts, &mut w)?;
            w.flush()?;
            println!("Exported {n} rows to {}", file.display());
            if !include_gps {
                println!("GPS coordinates were left out (use --include-gps to add them).");
            }
        }
        None => {
            let stdout = std::io::stdout();
            let mut w = BufWriter::new(stdout.lock());
            engine.export(root, local_index, format, opts, &mut w)?;
            w.flush()?;
        }
    }
    Ok(Exit::Ok)
}
