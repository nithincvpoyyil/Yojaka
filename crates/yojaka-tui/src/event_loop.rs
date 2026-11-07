//! Runs the UI: input, background results and redraws. Owns the threads `App` asks for.

use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use ratatui::crossterm::event::{self, Event};
use yojaka_core::scan::ScanOptions;
use yojaka_core::{CancelToken, CoreEvent, Engine, EventSink};

use crate::app::{App, Cmd, Msg};
use crate::keymap;
use crate::terminal::TerminalGuard;
use crate::view;
use crate::worker::{self, Bg, Request};

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
}

impl Runtime {
    fn run(&mut self, cmds: Vec<Cmd>) {
        for cmd in cmds {
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
                Cmd::OpenFile(path) => self.open_file(path),
            }
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
                let result = engine.scan(&opts, &sink, &token).map_err(|e| e.to_string());
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

    /// Joins the scan thread once it has reported back.
    fn reap_scan(&mut self) {
        if self.scan.as_ref().is_some_and(|(_, h)| h.is_finished())
            && let Some((_, h)) = self.scan.take()
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
    }
}

pub fn run(
    guard: &mut TerminalGuard,
    app: &mut App,
    engine: Arc<Engine>,
    local_index: bool,
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
                    let editing =
                        app.screen == crate::app::Screen::Browser && app.browser.editing_filter;
                    msgs.extend(keymap::translate(k, editing, app.confirm_flush));
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
