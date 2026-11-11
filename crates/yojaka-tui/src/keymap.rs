//! Single source of truth for key bindings; the help overlay is generated from it.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::app::{Msg, Screen};

/// (keys, description) pairs shown in the help overlay. `""` keys start a new section.
pub const HELP: &[(&str, &str)] = &[
    ("", "Everywhere"),
    ("q  Ctrl-C", "quit (Ctrl-C stops a running scan first)"),
    ("?  Esc", "open / close this help"),
    (
        "1-5  < >",
        "Dashboard, File browser, Duplicates, Organize, History",
    ),
    (
        "Shift ← →",
        "previous / next screen (also ← → outside the Browser)",
    ),
    ("r  c", "scan again for changes / cancel the running scan"),
    (
        "R",
        "forget what is saved and scan from scratch (asks first)",
    ),
    ("W", "write mode on (asks first) / off"),
    ("D", "debug overlay: frame time, fetch time"),
    ("", "File browser"),
    ("Tab", "switch between folders and files"),
    (
        "Enter",
        "files: open the file with the system's default app",
    ),
    ("j k  ↓ ↑", "move"),
    ("h l  ← →", "folders pane: collapse / expand"),
    ("PgUp PgDn  g G", "page, first / last row"),
    ("/", "filter by name, camera or place (Esc clears)"),
    ("t", "show all / photos / videos"),
    ("s  S", "next sort column / reverse order"),
    (
        "E",
        "export the files shown to a new CSV file (in Yojaka's data folder)",
    ),
    ("", "Duplicates (removing needs write mode)"),
    ("f  c", "look for duplicates / stop the search"),
    ("F", "also compare how pictures look (reads every picture)"),
    ("v", "show all / exact / likely / similar / name groups"),
    ("j k  ↓ ↑", "move in the list; Tab switches groups / copies"),
    ("Space", "mark / unmark the selected copy for removal"),
    ("Enter", "groups: open it; copies: keep only this copy"),
    (
        "p  x",
        "next keep rule (identical only) / keep all in group",
    ),
    ("a", "review the marked copies, then move them to the Trash"),
    (
        "P",
        "save the plan to a file (changes no photos; no write mode needed)",
    ),
    ("", "Organize (writes only after you review)"),
    ("e  d", "edit the folder pattern / the destination folder"),
    ("p", "next ready-made pattern"),
    ("m  i", "copy or move / infer places for files without GPS"),
    ("Enter", "work out the plan (changes nothing)"),
    ("a", "review the plan, then run it (needs write mode)"),
    (
        "P",
        "save the plan to a file (changes no photos; no write mode needed)",
    ),
    ("", "History"),
    ("j k  ↓ ↑", "move"),
    (
        "u",
        "put back the files of the selected run (needs write mode)",
    ),
];

/// Keys while a question, plan review or progress box is open: only text, Enter, Esc, Backspace.
pub fn translate_modal(key: KeyEvent) -> Option<Msg> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return (key.code == KeyCode::Char('c')).then_some(Msg::Interrupt);
    }
    if key
        .modifiers
        .intersects(KeyModifiers::ALT | KeyModifiers::SUPER)
    {
        return None;
    }
    match key.code {
        KeyCode::Esc => Some(Msg::Back),
        KeyCode::Enter => Some(Msg::Enter),
        KeyCode::Backspace => Some(Msg::Backspace),
        KeyCode::Char(c) => Some(Msg::Char(c)),
        _ => None,
    }
}

fn is_plain(mods: KeyModifiers) -> bool {
    !mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
}

pub fn translate(key: KeyEvent, editing_filter: bool, confirming: bool) -> Option<Msg> {
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
    if confirming {
        return match key.code {
            KeyCode::Char('y' | 'Y') | KeyCode::Enter => Some(Msg::ConfirmYes),
            KeyCode::Char('n' | 'N') | KeyCode::Esc => Some(Msg::ConfirmNo),
            _ => None,
        };
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
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        match key.code {
            KeyCode::Left => return Some(Msg::PrevScreen),
            KeyCode::Right => return Some(Msg::NextScreen),
            _ => {}
        }
    }
    Some(match key.code {
        KeyCode::Char('T') => Msg::TabMode,
        KeyCode::Char('<') => Msg::PrevScreen,
        KeyCode::Char('>') => Msg::NextScreen,
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
        KeyCode::Char('R') => Msg::FullRescan,
        KeyCode::Char('c') => Msg::CancelScan,
        KeyCode::Char('D') => Msg::ToggleDebug,
        KeyCode::Char(' ') => Msg::Toggle,
        KeyCode::Char('f') => Msg::FindDupes,
        KeyCode::Char('F') => Msg::FindSimilar,
        KeyCode::Char('v') => Msg::CycleTier,
        KeyCode::Char('p') => Msg::NextRule,
        KeyCode::Char('x') => Msg::KeepAllInGroup,
        KeyCode::Char('W') => Msg::ToggleWrite,
        KeyCode::Char('a') => Msg::ReviewPlan,
        KeyCode::Char('P') => Msg::SavePlan,
        KeyCode::Char('E') => Msg::ExportView,
        KeyCode::Char('u') => Msg::UndoRun,
        KeyCode::Char('e') => Msg::EditTemplate,
        KeyCode::Char('d') => Msg::EditDest,
        KeyCode::Char('m') => Msg::ToggleTransfer,
        KeyCode::Char('i') => Msg::ToggleInfer,
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
            let a = translate(plain(vim), false, false).map(|m| format!("{m:?}"));
            let b =
                translate(key(arrow, KeyModifiers::NONE), false, false).map(|m| format!("{m:?}"));
            assert_eq!(a, b);
            assert!(a.is_some());
        }
    }

    #[test]
    fn typing_a_filter_turns_command_keys_into_text() {
        assert!(matches!(
            translate(plain('q'), true, false),
            Some(Msg::Char('q'))
        ));
        assert!(matches!(
            translate(plain('q'), false, false),
            Some(Msg::Quit)
        ));
        assert!(matches!(
            translate(key(KeyCode::Esc, KeyModifiers::NONE), true, false),
            Some(Msg::FilterCancel)
        ));
        assert!(matches!(
            translate(key(KeyCode::Enter, KeyModifiers::NONE), true, false),
            Some(Msg::FilterAccept)
        ));
    }

    #[test]
    fn ctrl_c_always_interrupts_even_while_typing() {
        let k = key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(matches!(translate(k, true, false), Some(Msg::Interrupt)));
        assert!(matches!(translate(k, false, false), Some(Msg::Interrupt)));
    }

    #[test]
    fn other_ctrl_combinations_are_ignored() {
        assert!(translate(key(KeyCode::Char('q'), KeyModifiers::CONTROL), false, false).is_none());
    }

    #[test]
    fn key_releases_are_ignored() {
        let mut k = plain('q');
        k.kind = KeyEventKind::Release;
        assert!(translate(k, false, false).is_none());
    }

    #[test]
    fn capital_letters_are_distinct() {
        assert!(matches!(
            translate(plain('s'), false, false),
            Some(Msg::CycleSort)
        ));
        assert!(matches!(
            translate(plain('S'), false, false),
            Some(Msg::ReverseSort)
        ));
        assert!(matches!(
            translate(plain('G'), false, false),
            Some(Msg::Bottom)
        ));
    }

    #[test]
    fn angle_brackets_and_shift_arrows_switch_screens() {
        assert!(matches!(
            translate(plain('<'), false, false),
            Some(Msg::PrevScreen)
        ));
        assert!(matches!(
            translate(plain('>'), false, false),
            Some(Msg::NextScreen)
        ));
        assert!(matches!(
            translate(key(KeyCode::Right, KeyModifiers::SHIFT), false, false),
            Some(Msg::NextScreen)
        ));
        assert!(matches!(
            translate(key(KeyCode::Left, KeyModifiers::SHIFT), false, false),
            Some(Msg::PrevScreen)
        ));
        // typing a filter keeps '<' as text
        assert!(matches!(
            translate(plain('<'), true, false),
            Some(Msg::Char('<'))
        ));
    }

    #[test]
    fn help_lists_every_binding_it_documents() {
        assert!(HELP.iter().any(|(k, _)| k.contains("Tab")));
        assert!(HELP.iter().any(|(k, _)| *k == "t"));
    }
}
