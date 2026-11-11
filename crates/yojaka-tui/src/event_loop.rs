//! Runs the UI: input, background results and redraws. Owns the threads `App` asks for.

use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use ratatui::crossterm::event::{self, Event};
use yojaka_core::exec::{self, ApplyOptions, DirTrash, SystemTrash, Trasher};
use yojaka_core::scan::ScanOptions;
use yojaka_core::{CancelToken, CoreEvent, Engine, EventSink};

use crate::app::{App, Cmd, Msg};
use crate::keymap;
use crate::terminal::TerminalGuard;
use crate::view;
use crate::worker::{self, Bg, DupOutcome, ExecOutcome, Request};

/// What the `exec` thread does.
enum Job {
    Apply(Box<yojaka_core::plan::Plan>),
    Undo(uuid::Uuid),
}

const TICK: Duration = Duration::from_millis(250);

/// Everything the commands from `App::update` need.
struct Runtime {
    engine: Arc<Engine>,
    root: std::path::PathBuf,
    local_index: bool,
    fast: Sender<Request>,
    slow: Sender<Request>,
    bg_tx: Sender<Bg>,
    sink: EventSink,
    scan: Option<(CancelToken, JoinHandle<()>)>,
    dupes: Option<(CancelToken, JoinHandle<()>)>,
    exec: Option<(CancelToken, JoinHandle<()>)>,
    plan: Option<(CancelToken, JoinHandle<()>)>,
    trasher: Arc<dyn Trasher>,
}

impl Runtime {
    fn run(&mut self, cmds: Vec<Cmd>) {
        for cmd in cmds {
            match &cmd {
                Cmd::Worker(req) => tracing::trace!(?req, "ui: command"),
                other => tracing::debug!(cmd = ?other, "ui: command"),
            }
            match cmd {
                Cmd::Worker(req) => {
                    if matches!(req, Request::Reload) {
                        let _ = self.slow.send(req.clone());
                        let _ = self.fast.send(req);
                    } else if req.is_slow() {
                        let _ = self.slow.send(req);
                    } else {
                        let _ = self.fast.send(req);
                    }
                }
                Cmd::CancelScan => {
                    if let Some((token, _)) = &self.scan {
                        token.cancel();
                    }
                }
                Cmd::StartScan { force } => self.start_scan(force),
                Cmd::FindDupes { similar } => self.start_dupes(similar),
                Cmd::CancelDupes => {
                    if let Some((token, _)) = &self.dupes {
                        token.cancel();
                    }
                }
                Cmd::OpenFile(path) => self.open_file(path),
                Cmd::ApplyPlan(plan) => self.start_exec(Job::Apply(plan)),
                Cmd::UndoRun(id) => self.start_exec(Job::Undo(id)),
                Cmd::CancelExec => {
                    if let Some((token, _)) = &self.exec {
                        token.cancel();
                    }
                }
                Cmd::LoadHistory => self.load_history(),
                Cmd::PlanOrganize(req) => self.start_plan(*req),
                Cmd::SavePlan(plan) => self.save_plan(&plan),
                Cmd::Remember(what) => self.remember(&what),
                Cmd::ExportView(query) => self.export_view(*query),
                Cmd::CancelPlan => {
                    if let Some((token, _)) = &self.plan {
                        token.cancel();
                    }
                }
            }
        }
    }

    /// Writes the choice to `config.toml` (only on request: this is the `M` key).
    fn remember(&self, what: &crate::app::Remember) {
        use crate::app::Remember;
        let file = self.engine.paths.config_file();
        let result = yojaka_core::Config::remember(&file, |c| match what {
            Remember::KeepRules(chain) => {
                c.dupes.keep_rules = chain.split(',').map(str::to_string).collect();
            }
            Remember::Organize { template, mode } => {
                c.organize.template.clone_from(template);
                c.organize.mode.clone_from(mode);
            }
        });
        let msg = match (result, what) {
            (Ok(()), Remember::KeepRules(_)) => "Saved this keep rule as the default".to_string(),
            (Ok(()), Remember::Organize { .. }) => {
                "Saved the folder pattern and copy / move as the defaults".to_string()
            }
            (Err(e), _) => format!("Could not save: {e}"),
        };
        let _ = self.bg_tx.send(Bg::Notice(msg));
    }

    /// Writes the plan to a new file in the data folder. Never overwrites, touches no photos.
    fn save_plan(&self, plan: &yojaka_core::plan::Plan) {
        let dir = self.engine.paths.plans_dir();
        let name = format!(
            "plan-{}.json",
            &uuid::Uuid::now_v7().simple().to_string()[..12]
        );
        let file = dir.join(&name);
        let result = std::fs::create_dir_all(&dir)
            .map_err(|e| e.to_string())
            .and_then(|()| plan.save(&file).map_err(|e| e.to_string()));
        let msg = match result {
            Ok(()) => format!(
                "Saved the plan to {}  (yojaka plan show <file>)",
                file.display()
            ),
            Err(e) => format!("Could not save the plan: {e}"),
        };
        let _ = self.bg_tx.send(Bg::Notice(msg));
    }

    /// Writes the files the browser shows to a new CSV in the data folder (GPS left out).
    fn export_view(&self, query: yojaka_core::browse::BrowseQuery) {
        let (engine, tx) = (self.engine.clone(), self.bg_tx.clone());
        let (root, local) = (self.root.clone(), self.local_index);
        let spawned = std::thread::Builder::new()
            .name("export".into())
            .spawn(move || {
                let dir = engine.paths.exports_dir();
                let file = dir.join(format!(
                    "export-{}.csv",
                    &uuid::Uuid::now_v7().simple().to_string()[..12]
                ));
                let result = (|| -> Result<u64, String> {
                    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                    let f = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&file)
                        .map_err(|e| e.to_string())?;
                    let mut w = std::io::BufWriter::new(f);
                    let opts = yojaka_core::export::ExportOptions {
                        query,
                        ..Default::default()
                    };
                    let n = engine
                        .export(
                            &root,
                            local,
                            yojaka_core::export::ExportFormat::Csv,
                            opts,
                            &mut w,
                        )
                        .map_err(|e| e.to_string())?;
                    std::io::Write::flush(&mut w).map_err(|e| e.to_string())?;
                    Ok(n)
                })();
                let msg = match result {
                    Ok(n) => format!("Exported {n} files to {}", file.display()),
                    Err(e) => format!("Could not export: {e}"),
                };
                let _ = tx.send(Bg::Notice(msg));
            });
        if let Err(e) = spawned {
            let _ = self
                .bg_tx
                .send(Bg::Notice(format!("Could not export: {e}")));
        }
    }

    /// Asks macOS to open `path` with the default app, and lets the OS decide which one.
    /// Output is discarded: nothing may be printed while the TUI runs.
    fn open_file(&self, path: std::path::PathBuf) {
        use std::process::{Command, Stdio};
        if !path.is_file() {
            let _ = self.bg_tx.send(Bg::Notice(format!(
                "{} is not there any more (r to rescan)",
                path.file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
            )));
            return;
        }
        // The index holds absolute paths, so the argument can never look like an option.
        let child = Command::new("/usr/bin/open")
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match child {
            Ok(mut child) => {
                let tx = self.bg_tx.clone();
                std::thread::spawn(move || {
                    if !child.wait().is_ok_and(|s| s.success()) {
                        let _ = tx.send(Bg::Notice("The system could not open that file".into()));
                    }
                });
            }
            Err(e) => {
                let _ = self.bg_tx.send(Bg::Notice(format!("Cannot open: {e}")));
            }
        }
    }

    fn start_scan(&mut self, force: bool) {
        self.reap_scan();
        if self.scan.is_some() {
            return;
        }
        if force {
            // Only Yojaka's own index files go; the workers drop their open handles too.
            if let Err(e) = self.engine.flush(&self.root, self.local_index) {
                let _ = self.bg_tx.send(Bg::ScanFinished(Err(format!(
                    "cannot reset this folder: {e}"
                ))));
                return;
            }
            let _ = self.slow.send(Request::Reload);
            let _ = self.fast.send(Request::Reload);
        }
        let cancel = CancelToken::new();
        let opts = ScanOptions {
            root: self.root.clone(),
            local_index: self.local_index,
            metadata: true,
            ..Default::default()
        };
        let (engine, sink, tx, token) = (
            self.engine.clone(),
            self.sink.clone(),
            self.bg_tx.clone(),
            cancel.clone(),
        );
        let spawned = std::thread::Builder::new()
            .name("scan".into())
            .spawn(move || {
                tracing::info!("scan thread started");
                let t = std::time::Instant::now();
                let result = engine.scan(&opts, &sink, &token).map_err(|e| e.to_string());
                match &result {
                    Ok(_) => {
                        tracing::info!(ms = t.elapsed().as_millis() as u64, "scan thread finished")
                    }
                    Err(e) => {
                        tracing::error!(ms = t.elapsed().as_millis() as u64, "scan failed: {e}")
                    }
                }
                let _ = tx.send(Bg::ScanFinished(result));
            });
        match spawned {
            Ok(h) => self.scan = Some((cancel, h)),
            Err(e) => {
                let _ = self
                    .bg_tx
                    .send(Bg::ScanFinished(Err(format!("cannot start the scan: {e}"))));
            }
        }
    }

    /// Looks for duplicates on its own thread. Reads files, changes none.
    fn start_dupes(&mut self, similar: bool) {
        if self.dupes.as_ref().is_some_and(|(_, h)| h.is_finished())
            && let Some((_, h)) = self.dupes.take()
        {
            let _ = h.join();
        }
        if self.dupes.is_some() {
            return;
        }
        let cancel = CancelToken::new();
        let (engine, sink, tx, token) = (
            self.engine.clone(),
            self.sink.clone(),
            self.bg_tx.clone(),
            cancel.clone(),
        );
        let (root, local) = (self.root.clone(), self.local_index);
        let spawned = std::thread::Builder::new()
            .name("dupes".into())
            .spawn(move || {
                tracing::info!("dupes thread started");
                let mut opts = yojaka_core::dupes::DupOptions::all();
                opts.similar = similar;
                let outcome = match engine.dupes(&root, local, opts, &sink, &token) {
                    Ok(report) => {
                        tracing::info!("dupes thread finished");
                        DupOutcome::Done(Box::new(report))
                    }
                    Err(yojaka_core::YojakaError::Cancelled) => {
                        tracing::info!("dupes cancelled");
                        DupOutcome::Cancelled
                    }
                    Err(e) => {
                        tracing::error!("dupes failed: {e}");
                        DupOutcome::Failed(e.to_string())
                    }
                };
                let _ = tx.send(Bg::DupesFinished(outcome));
            });
        match spawned {
            Ok(h) => self.dupes = Some((cancel, h)),
            Err(e) => {
                let _ = self
                    .bg_tx
                    .send(Bg::DupesFinished(DupOutcome::Failed(format!(
                        "cannot start the search: {e}"
                    ))));
            }
        }
    }

    /// Applies a plan or undoes a run on its own thread. Everything it does is journaled.
    fn start_exec(&mut self, job: Job) {
        self.reap_exec();
        if self.exec.is_some() {
            let _ = self.bg_tx.send(Bg::ExecFinished(ExecOutcome::Failed(
                "another run is still going".into(),
            )));
            return;
        }
        let cancel = CancelToken::new();
        let (engine, sink, tx, token, trasher) = (
            self.engine.clone(),
            self.sink.clone(),
            self.bg_tx.clone(),
            cancel.clone(),
            self.trasher.clone(),
        );
        let spawned = std::thread::Builder::new()
            .name("exec".into())
            .spawn(move || {
                let outcome = match job {
                    Job::Apply(plan) => {
                        tracing::info!(plan = %plan.id, ops = plan.ops.len(), "apply started");
                        match exec::apply(
                            &plan,
                            &engine.paths,
                            trasher.as_ref(),
                            ApplyOptions::default(),
                            &sink,
                            &token,
                        ) {
                            Ok(r) => ExecOutcome::Applied(Box::new(r)),
                            Err(e) => ExecOutcome::Failed(e.to_string()),
                        }
                    }
                    Job::Undo(id) => {
                        tracing::info!(plan = %id, "undo started");
                        match exec::undo(id, &engine.paths, &sink, &token) {
                            Ok(r) => ExecOutcome::Undone(Box::new(r)),
                            Err(e) => ExecOutcome::Failed(e.to_string()),
                        }
                    }
                };
                tracing::info!("exec thread finished");
                let _ = tx.send(Bg::ExecFinished(outcome));
            });
        match spawned {
            Ok(h) => self.exec = Some((cancel, h)),
            Err(e) => {
                let _ = self
                    .bg_tx
                    .send(Bg::ExecFinished(ExecOutcome::Failed(format!(
                        "cannot start: {e}"
                    ))));
            }
        }
    }

    /// Works out an organize plan on its own thread. Reads the index; changes no file.
    fn start_plan(&mut self, req: crate::organize::PlanRequest) {
        self.reap_plan();
        if self.plan.is_some() {
            return;
        }
        let cancel = CancelToken::new();
        let (engine, sink, tx, token) = (
            self.engine.clone(),
            self.sink.clone(),
            self.bg_tx.clone(),
            cancel.clone(),
        );
        let (root, local) = (self.root.clone(), self.local_index);
        let spawned = std::thread::Builder::new()
            .name("plan".into())
            .spawn(move || {
                let mut location = engine.config.location.clone();
                location.infer |= req.infer;
                let opts = yojaka_core::organize::OrganizeOptions {
                    dest: req.dest,
                    template: req.template,
                    transfer: req.transfer,
                    infer_location: req.infer,
                    location,
                };
                let result = engine
                    .organize(&root, local, &opts, &sink, &token)
                    .map(Box::new)
                    .map_err(|e| match e {
                        yojaka_core::YojakaError::Cancelled => "Stopped".to_string(),
                        other => other.to_string(),
                    });
                let _ = tx.send(Bg::Planned(result));
            });
        match spawned {
            Ok(h) => self.plan = Some((cancel, h)),
            Err(e) => {
                let _ = self
                    .bg_tx
                    .send(Bg::Planned(Err(format!("cannot start planning: {e}"))));
            }
        }
    }

    fn reap_plan(&mut self) {
        if self.plan.as_ref().is_some_and(|(_, h)| h.is_finished())
            && let Some((_, h)) = self.plan.take()
        {
            let _ = h.join();
        }
    }

    fn load_history(&self) {
        let (engine, tx) = (self.engine.clone(), self.bg_tx.clone());
        let _ = std::thread::Builder::new()
            .name("history".into())
            .spawn(move || match exec::history::list(&engine.paths) {
                Ok(runs) => {
                    let _ = tx.send(Bg::History(runs));
                }
                Err(e) => {
                    let _ = tx.send(Bg::Notice(format!("Cannot read the history: {e}")));
                }
            });
    }

    fn reap_exec(&mut self) {
        if self.exec.as_ref().is_some_and(|(_, h)| h.is_finished())
            && let Some((_, h)) = self.exec.take()
        {
            let _ = h.join();
        }
    }

    /// Joins the scan thread once it has reported back.
    fn reap_scan(&mut self) {
        if self.scan.as_ref().is_some_and(|(_, h)| h.is_finished())
            && let Some((_, h)) = self.scan.take()
        {
            let _ = h.join();
        }
    }

    fn reap_dupes(&mut self) {
        if self.dupes.as_ref().is_some_and(|(_, h)| h.is_finished())
            && let Some((_, h)) = self.dupes.take()
        {
            let _ = h.join();
        }
    }

    /// Stops a running scan (it flushes its current batch) and waits for it.
    fn shutdown(&mut self) {
        if let Some((token, h)) = self.scan.take() {
            token.cancel();
            let _ = h.join();
        }
        if let Some((token, h)) = self.dupes.take() {
            token.cancel();
            let _ = h.join();
        }
        if let Some((token, h)) = self.plan.take() {
            token.cancel();
            let _ = h.join();
        }
        // A run stops after the file it is on; `apply --resume` finishes it later.
        if let Some((token, h)) = self.exec.take() {
            token.cancel();
            let _ = h.join();
        }
    }
}

pub fn run(
    guard: &mut TerminalGuard,
    app: &mut App,
    engine: Arc<Engine>,
    local_index: bool,
    trash_dir: Option<std::path::PathBuf>,
) -> std::io::Result<()> {
    let (sink, core_rx): (EventSink, Receiver<CoreEvent>) = EventSink::channel(1024);
    let (bg_tx, bg_rx) = crossbeam_channel::bounded::<Bg>(256);
    let (fast_tx, fast_rx) = crossbeam_channel::unbounded();
    let (slow_tx, slow_rx) = crossbeam_channel::unbounded();
    let workers = [
        worker::spawn(
            "browse",
            engine.clone(),
            app.root.clone(),
            local_index,
            fast_rx,
            bg_tx.clone(),
        )?,
        worker::spawn(
            "stats",
            engine.clone(),
            app.root.clone(),
            local_index,
            slow_rx,
            bg_tx.clone(),
        )?,
    ];
    let mut rt = Runtime {
        engine,
        root: app.root.clone(),
        local_index,
        fast: fast_tx,
        slow: slow_tx,
        bg_tx,
        sink,
        scan: None,
        dupes: None,
        exec: None,
        plan: None,
        trasher: match trash_dir {
            Some(dir) => Arc::new(DirTrash(dir)),
            None => Arc::new(SystemTrash),
        },
    };

    let size = guard.term.size()?;
    app.size = (size.width, size.height);
    let init = app.init();
    rt.run(init);

    let mut last_tick = Instant::now();
    let result = loop {
        if app.dirty {
            let t = Instant::now();
            if let Err(e) = guard.term.draw(|f| view::render(app, f)) {
                break Err(e);
            }
            app.dirty = false;
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            app.debug_info.frame_ms = ms;
            // decays slowly so a single slow frame stays visible for a moment
            app.debug_info.frame_max_ms = (app.debug_info.frame_max_ms * 0.98).max(ms);
        }

        let mut msgs: Vec<Msg> = vec![];
        match event::poll(Duration::from_millis(16)) {
            Ok(true) => match event::read() {
                Ok(Event::Key(k)) => {
                    let editing = (app.screen == crate::app::Screen::Browser
                        && app.browser.editing_filter)
                        || (app.screen == crate::app::Screen::Organize
                            && app.organize.editing.is_some());
                    if app.modal.is_some() {
                        msgs.extend(keymap::translate_modal(k));
                    } else {
                        msgs.extend(keymap::translate(k, editing, app.confirm_flush));
                    }
                }
                Ok(Event::Resize(w, h)) => msgs.push(Msg::Resize(w, h)),
                Ok(_) => {}
                Err(e) => break Err(e),
            },
            Ok(false) => {}
            Err(e) => break Err(e),
        }
        msgs.extend(core_rx.try_iter().take(512).map(Msg::Core));
        while let Ok(bg) = bg_rx.try_recv() {
            if matches!(bg, Bg::ScanFinished(_)) {
                rt.reap_scan();
            }
            if matches!(bg, Bg::DupesFinished(_)) {
                rt.reap_dupes();
            }
            if matches!(bg, Bg::ExecFinished(_)) {
                rt.reap_exec();
            }
            if matches!(bg, Bg::Planned(_)) {
                rt.reap_plan();
            }
            msgs.push(Msg::Bg(bg));
        }
        if last_tick.elapsed() >= TICK {
            last_tick = Instant::now();
            msgs.push(Msg::Tick);
        }

        let mut dirty = false;
        for m in msgs {
            let cmds = app.update(m);
            dirty |= app.dirty;
            rt.run(cmds);
        }
        app.dirty = dirty || app.dirty;
        if app.should_quit {
            break Ok(());
        }
    };

    rt.shutdown();
    drop(rt); // closes the worker channels
    for w in workers {
        let _ = w.join();
    }
    result
}
