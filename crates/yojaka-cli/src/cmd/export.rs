use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::Path;

use yojaka_core::Engine;
use yojaka_core::export::{ExportFormat, ExportOptions};

use crate::cli::ExportFmt;
use crate::exit::Exit;

pub fn run(
    engine: &Engine,
    path: Option<&Path>,
    format: ExportFmt,
    include_gps: bool,
    out: Option<&Path>,
    local_index: bool,
) -> anyhow::Result<Exit> {
    let root = path.unwrap_or_else(|| Path::new("."));
    let format = match format {
        ExportFmt::Csv => ExportFormat::Csv,
        ExportFmt::Json => ExportFormat::Json,
    };
    let opts = ExportOptions { include_gps };
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
