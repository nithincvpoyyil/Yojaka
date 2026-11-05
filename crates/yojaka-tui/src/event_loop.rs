use std::time::Duration;

use ratatui::crossterm::event::{self, Event};

use crate::app::{App, Msg};
use crate::keymap;
use crate::terminal::TerminalGuard;
use crate::view;

pub fn run(
    guard: &mut TerminalGuard,
    app: &mut App,
    core_rx: &crossbeam_channel::Receiver<yojaka_core::CoreEvent>,
) -> std::io::Result<()> {
    let mut dirty = true;
    loop {
        if dirty {
            guard.term.draw(|f| view::render(app, f))?;
            dirty = false;
        }
        if event::poll(Duration::from_millis(33))? {
            match event::read()? {
                Event::Key(k) => {
                    if let Some(msg) = keymap::translate(k) {
                        dirty |= app.update(msg);
                    }
                }
                Event::Resize(..) => dirty |= app.update(Msg::Resize),
                _ => {}
            }
        }
        while let Ok(ev) = core_rx.try_recv() {
            dirty |= app.update(Msg::Core(ev));
        }
        if app.should_quit {
            return Ok(());
        }
    }
}
