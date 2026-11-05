//! Single source of truth for key bindings; the help overlay is generated from it.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{Msg, Screen};

/// (keys, description) pairs shown in the help overlay.
pub const HELP: &[(&str, &str)] = &[
    ("q", "quit"),
    ("?", "toggle this help"),
    (
        "1-5",
        "switch screen (Dashboard, Browser, Duplicates, Organize, History)",
    ),
    ("Esc", "close help"),
    ("arrows / hjkl", "move (from Iteration 3)"),
];

pub fn translate(key: KeyEvent) -> Option<Msg> {
    if key.kind == ratatui::crossterm::event::KeyEventKind::Release {
        return None;
    }
    #[cfg(debug_assertions)]
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('\\') {
        return Some(Msg::DebugPanic);
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Some(Msg::Quit);
    }
    Some(match key.code {
        KeyCode::Char('q') => Msg::Quit,
        KeyCode::Char('?') => Msg::ToggleHelp,
        KeyCode::Esc => Msg::Back,
        KeyCode::Char('1') => Msg::Goto(Screen::Dashboard),
        KeyCode::Char('2') => Msg::Goto(Screen::Browser),
        KeyCode::Char('3') => Msg::Goto(Screen::Duplicates),
        KeyCode::Char('4') => Msg::Goto(Screen::Organize),
        KeyCode::Char('5') => Msg::Goto(Screen::History),
        _ => return None,
    })
}
