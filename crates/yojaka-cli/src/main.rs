#![allow(clippy::print_stdout, clippy::print_stderr)]

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
compile_error!("Yojaka v1 supports macOS on Apple Silicon only");

mod cli;
mod cmd;
mod exit;
mod logging;

use std::process::ExitCode;

use clap::Parser;
use yojaka_core::{AppPaths, Config, Engine};

use cli::{Cli, Cmd};
use exit::Exit;

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let _ = e.print();
            return if e.use_stderr() {
                Exit::Usage.into()
            } else {
                Exit::Ok.into()
            };
        }
    };
    match run(cli) {
        Ok(code) => code.into(),
        Err(e) => {
            tracing::error!("fatal: {e:#}");
            eprintln!("error: {e}");
            if let Some(hint) = hint_for(&e) {
                eprintln!("hint: {hint}");
            }
            Exit::Error.into()
        }
    }
}

fn hint_for(e: &anyhow::Error) -> Option<&'static str> {
    use yojaka_core::YojakaError::*;
    match e.downcast_ref::<yojaka_core::YojakaError>()? {
        Config { .. } => {
            Some("fix or delete the config file; defaults are used when it is missing")
        }
        Protected { .. } => Some("pick a folder that contains your photos, such as ~/Pictures"),
        NoIndex(_) => Some("run `yojaka scan <path>` first"),
        IndexVersion(_) | Index(_) => Some(
            "scan it from scratch with `yojaka scan --force <path>` (your files are not touched)",
        ),
        _ => None,
    }
}

fn run(cli: Cli) -> anyhow::Result<Exit> {
    if cli.about {
        // Nothing is created or read for this: it works anywhere.
        println!("{}", cli::ABOUT.trim_end());
        return Ok(Exit::Ok);
    }
    let paths = AppPaths::from_env_or(cli.home.as_deref())?;
    paths.ensure()?;
    // Only with --debug. Printed before the TUI takes the terminal.
    let _debug_log = if cli.debug {
        let log = logging::init(&paths.logs);
        match &log {
            Some(l) => eprintln!("debug log: {}", l.path.display()),
            None => eprintln!("warning: cannot write the debug log"),
        }
        log
    } else {
        None
    };
    let mut config = Config::load(&paths.config_file())?;
    if let Some(t) = cli.threads {
        config.threads = t;
    }
    if _debug_log.is_some() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        logging::log_header(&args, config.threads);
    }
    let engine = Engine::new(paths, config);

    match &cli.cmd {
        None => {
            let root = match &cli.path {
                Some(p) => p.clone(),
                None => std::env::current_dir()?,
            };
            let mode = if cli.write {
                yojaka_tui::Mode::Write
            } else {
                yojaka_tui::Mode::ReadOnly
            };
            // stdout/stderr belong to the TUI from here on.
            yojaka_tui::run(
                engine,
                root,
                yojaka_tui::TuiOptions {
                    mode,
                    local_index: cli.local_index,
                    refresh: cli.refresh,
                    force: cli.force,
                    trash_dir: cli.trash_dir.clone(),
                },
            )?;
            Ok(Exit::Ok)
        }
        Some(Cmd::Doctor { json }) => Ok(cmd::doctor::run(&engine, *json)),
        Some(Cmd::Scan {
            path,
            kind,
            sniff_all,
            force,
            no_metadata,
        }) => Ok(cmd::scan::run(
            &engine,
            path.as_deref(),
            *kind,
            *sniff_all,
            cli.local_index,
            *force,
            !*no_metadata,
        )?),
        Some(Cmd::Info { file, json }) => Ok(cmd::info::run(&engine, file, *json)?),
        Some(Cmd::Places { path, json }) => Ok(cmd::places::run(
            &engine,
            path.as_deref(),
            *json,
            cli.local_index,
        )?),
        Some(Cmd::Export {
            path,
            format,
            include_gps,
            out,
            kind,
            year,
            find,
            folder,
        }) => Ok(cmd::export::run(
            &engine,
            path.as_deref(),
            *format,
            cmd::export::Filter {
                include_gps: *include_gps,
                kind: *kind,
                year: *year,
                find: find.clone(),
                folder: folder.clone(),
            },
            out.as_deref(),
            cli.local_index,
        )?),
        Some(Cmd::Stats { path, json }) => Ok(cmd::stats::run(
            &engine,
            path.as_deref(),
            *json,
            cli.local_index,
        )?),
        Some(Cmd::Dupes {
            path,
            tier,
            similar,
            similar_distance,
            keep,
            plan_out,
            json,
        }) => Ok(cmd::dupes::run(
            &engine,
            cmd::dupes::Args {
                path: path.as_deref(),
                tier: *tier,
                similar: *similar,
                similar_distance: *similar_distance,
                keep,
                plan_out: plan_out.as_deref(),
                json: *json,
                local_index: cli.local_index,
            },
        )?),
        Some(Cmd::Plan {
            sub: cli::PlanCmd::Show { file, full_tree },
        }) => cmd::exec::plan_show(file, *full_tree),
        Some(Cmd::Apply {
            file,
            yes,
            resume,
            permanent,
        }) => cmd::exec::apply(
            &engine,
            &cmd::exec::ApplyArgs {
                file,
                write: cli.write,
                yes: *yes,
                resume: *resume,
                permanent: *permanent,
                trash_dir: cli.trash_dir.as_deref(),
            },
        ),
        Some(Cmd::Undo { plan_id, last }) => {
            cmd::exec::undo(&engine, plan_id.as_deref(), *last, cli.write)
        }
        Some(Cmd::History { json }) => cmd::exec::history(&engine, *json),
        Some(Cmd::Organize {
            path,
            dest,
            template,
            preset,
            copy: _,
            r#move,
            infer_location,
            plan_out,
            full_tree,
        }) => cmd::organize::run(
            &engine,
            cmd::organize::Args {
                path: Some(path.as_path()),
                dest,
                template: template.as_deref(),
                preset: *preset,
                r#move: *r#move,
                infer_location: *infer_location,
                plan_out: plan_out.as_deref(),
                full_tree: *full_tree,
                local_index: cli.local_index,
            },
        ),
    }
}
