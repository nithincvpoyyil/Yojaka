//! `yojaka organize`: plan an arrangement of the scanned files. Changes nothing.

use std::path::{Path, PathBuf};
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};
use yojaka_core::organize::{
    OrganizeOptions, Template, Transfer, configured_template, configured_transfer, transfer_word,
};
use yojaka_core::{CancelToken, Engine, EventSink};

use super::exec::print_summary;
use crate::cli::PresetArg;
use crate::exit::Exit;

pub struct Args<'a> {
    pub path: Option<&'a Path>,
    pub dest: &'a Path,
    pub template: Option<&'a str>,
    pub preset: Option<PresetArg>,
    pub r#move: bool,
    pub copy: bool,
    pub remember: bool,
    pub infer_location: bool,
    pub plan_out: Option<&'a Path>,
    pub full_tree: bool,
    pub local_index: bool,
}

pub fn run(engine: &Engine, a: Args<'_>) -> anyhow::Result<Exit> {
    // Pattern and copy / move: the flags, else what is saved in the config file (a bad saved
    // value is ignored with a warning).
    let text = match a.template.or(a.preset.map(PresetArg::template)) {
        Some(t) => t.to_string(),
        None => {
            let (t, warning) = configured_template(&engine.config.organize.template);
            if let Some(w) = warning {
                eprintln!("warning: {w}");
            }
            t
        }
    };
    let template = match Template::parse(&text) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: {e}");
            return Ok(Exit::Usage);
        }
    };
    let transfer = if a.r#move {
        Transfer::Move
    } else if a.copy {
        Transfer::Copy
    } else {
        let (t, warning) = configured_transfer(&engine.config.organize.mode);
        if let Some(w) = warning {
            eprintln!("warning: {w}");
        }
        t
    };
    if a.remember {
        let file = engine.paths.config_file();
        yojaka_core::Config::remember(&file, |c| {
            c.organize.template = text.clone();
            c.organize.mode = transfer_word(transfer).to_string();
        })?;
        eprintln!(
            "Saved the folder pattern and {} as the defaults in {}",
            transfer_word(transfer),
            file.display()
        );
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
    let dest: PathBuf = std::path::absolute(a.dest)?;
    if dest
        .components()
        .any(|c| c == std::path::Component::ParentDir)
    {
        eprintln!("error: the destination may not contain `..`");
        return Ok(Exit::Usage);
    }
    let mut location = engine.config.location.clone();
    location.infer |= a.infer_location;
    let opts = OrganizeOptions {
        dest,
        template,
        transfer,
        infer_location: location.infer,
        location,
    };

    let root = a.path.unwrap_or_else(|| Path::new("."));
    let cancel = CancelToken::new();
    {
        let cancel = cancel.clone();
        ctrlc::set_handler(move || cancel.cancel())?;
    }
    let bar = ProgressBar::new_spinner();
    bar.set_style(
        ProgressStyle::with_template("{spinner} planning")
            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );
    bar.enable_steady_tick(Duration::from_millis(100));
    let result = engine.organize(root, a.local_index, &opts, &EventSink::null(), &cancel);
    bar.finish_and_clear();
    let planned = match result {
        Ok(p) => p,
        Err(yojaka_core::YojakaError::Cancelled) => {
            eprintln!("cancelled");
            return Ok(Exit::Partial);
        }
        Err(e) => return Err(e.into()),
    };

    println!("Template: {}", opts.template.source());
    let st = &planned.stats;
    if opts.template.uses_place() {
        println!(
            "Location: {:.0}% of {} files have a place{}",
            st.coverage() * 100.0,
            st.files,
            if st.inferred > 0 {
                format!(" ({} inferred)", st.inferred)
            } else {
                String::new()
            }
        );
    }
    print_summary(&planned.plan, a.full_tree);
    if let Some(out) = a.plan_out {
        planned.plan.save(out)?;
        println!(
            "\nPlan saved to {}. Nothing has been changed.\nRun it with: yojaka --write apply {}",
            out.display(),
            out.display()
        );
    } else {
        println!("\nNothing has been changed. Add --plan-out FILE to save this plan.");
    }
    Ok(Exit::Ok)
}
