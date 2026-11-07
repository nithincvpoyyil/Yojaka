//! Single source of truth for key bindings; the help overlay is generated from it.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::app::{Msg, Screen};

/// (keys, description) pairs shown in the help overlay. `""` keys start a new section.
pub const HELP: &[(&str, &str)] = &[
    ("", "Everywhere"),
    ("q  Ctrl-C", "quit (Ctrl-C stops a running scan first)"),
    ("?  Esc", "open / close this help"),
    ("1-5", "Dashboard, Browser, Duplicates, Organize, History"),
    ("r  c", "rescan this folder / cancel the running scan"),
    ("D", "debug overlay: frame time, fetch time"),
    ("", "Browser"),
    ("Tab", "switch between folders and files"),
    ("j k  ↓ ↑", "move"),
    ("h l  ← →", "folders: collapse / expand"),
    ("PgUp PgDn  g G", "page, first / last row"),
    ("/", "filter by name, camera or place (Esc clears)"),
    ("t", "show all / photos / videos"),
    ("s  S", "next sort column / reverse order"),
];

fn is_plain(mods: KeyModifiers) -> bool {
    !mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
}

pub fn translate(key: KeyEvent, editing_filter: bool) -> Option<Msg> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    #[cfg(debug_assertions)]
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('\\') {
        return Some(Msg::DebugPanic);
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Some(Msg::Interrupt);
    }
    if editing_filter {
        return match key.code {
            KeyCode::Esc => Some(Msg::FilterCancel),
            KeyCode::Enter => Some(Msg::FilterAccept),
            KeyCode::Backspace => Some(Msg::Backspace),
            KeyCode::Char(c) if is_plain(key.modifiers) => Some(Msg::Char(c)),
            _ => None,
        };
    }
    if !is_plain(key.modifiers) {
        return None;
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
        KeyCode::Up | KeyCode::Char('k') => Msg::Up,
        KeyCode::Down | KeyCode::Char('j') => Msg::Down,
        KeyCode::Left | KeyCode::Char('h') => Msg::Left,
        KeyCode::Right | KeyCode::Char('l') => Msg::Right,
        KeyCode::PageUp => Msg::PageUp,
        KeyCode::PageDown => Msg::PageDown,
        KeyCode::Home | KeyCode::Char('g') => Msg::Top,
        KeyCode::End | KeyCode::Char('G') => Msg::Bottom,
        KeyCode::Tab | KeyCode::BackTab => Msg::Tab,
        KeyCode::Enter => Msg::Enter,
        KeyCode::Char('t') => Msg::CycleKind,
        KeyCode::Char('s') => Msg::CycleSort,
        KeyCode::Char('S') => Msg::ReverseSort,
        KeyCode::Char('/') => Msg::StartFilter,
        KeyCode::Char('r') => Msg::Rescan,
        KeyCode::Char('c') => Msg::CancelScan,
        KeyCode::Char('D') => Msg::ToggleDebug,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::KeyEventState;

    use super::*;

    fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: mods,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }
    fn plain(c: char) -> KeyEvent {
        key(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn vim_and_arrow_keys_agree() {
        for (vim, arrow) in [
            ('j', KeyCode::Down),
            ('k', KeyCode::Up),
            ('h', KeyCode::Left),
            ('l', KeyCode::Right),
        ] {
            let a = translate(plain(vim), false).map(|m| format!("{m:?}"));
            let b = translate(key(arrow, KeyModifiers::NONE), false).map(|m| format!("{m:?}"));
            assert_eq!(a, b);
            assert!(a.is_some());
        }
    }

    #[test]
    fn typing_a_filter_turns_command_keys_into_text() {
        assert!(matches!(translate(plain('q'), true), Some(Msg::Char('q'))));
        assert!(matches!(translate(plain('q'), false), Some(Msg::Quit)));
        assert!(matches!(
            translate(key(KeyCode::Esc, KeyModifiers::NONE), true),
            Some(Msg::FilterCancel)
        ));
        assert!(matches!(
            translate(key(KeyCode::Enter, KeyModifiers::NONE), true),
            Some(Msg::FilterAccept)
        ));
    }

    #[test]
    fn ctrl_c_always_interrupts_even_while_typing() {
        let k = key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(matches!(translate(k, true), Some(Msg::Interrupt)));
        assert!(matches!(translate(k, false), Some(Msg::Interrupt)));
    }

    #[test]
    fn other_ctrl_combinations_are_ignored() {
        assert!(translate(key(KeyCode::Char('q'), KeyModifiers::CONTROL), false).is_none());
    }

    #[test]
    fn key_releases_are_ignored() {
        let mut k = plain('q');
        k.kind = KeyEventKind::Release;
        assert!(translate(k, false).is_none());
    }

    #[test]
    fn capital_letters_are_distinct() {
        assert!(matches!(translate(plain('s'), false), Some(Msg::CycleSort)));
        assert!(matches!(
            translate(plain('S'), false),
            Some(Msg::ReverseSort)
        ));
        assert!(matches!(translate(plain('G'), false), Some(Msg::Bottom)));
    }

    #[test]
    fn help_lists_every_binding_it_documents() {
        assert!(HELP.iter().any(|(k, _)| k.contains("Tab")));
        assert!(HELP.iter().any(|(k, _)| *k == "t"));
    }
}
