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
        IndexVersion(_) => Some("update Yojaka, or remove the index file and scan again"),
        _ => None,
    }
}

fn run(cli: Cli) -> anyhow::Result<Exit> {
    let paths = AppPaths::from_env_or(cli.home.as_deref())?;
    paths.ensure()?;
    let _log_guard = logging::init(&paths.logs);
    let mut config = Config::load(&paths.config_file())?;
    if let Some(t) = cli.threads {
        config.threads = t;
    }
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "yojaka starting");
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
            yojaka_tui::run(engine, root, yojaka_tui::TuiOptions { mode })?;
            Ok(Exit::Ok)
        }
        Some(Cmd::Doctor { json }) => Ok(cmd::doctor::run(&engine, *json)),
        Some(other) => Ok(cmd::stub::not_yet(other)),
    }
}
