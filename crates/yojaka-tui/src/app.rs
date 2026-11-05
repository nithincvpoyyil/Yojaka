use std::path::PathBuf;

use yojaka_core::CoreEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Dashboard,
    Browser,
    Duplicates,
    Organize,
    History,
}

impl Screen {
    pub fn title(self) -> &'static str {
        match self {
            Screen::Dashboard => "Dashboard",
            Screen::Browser => "Browser",
            Screen::Duplicates => "Duplicates",
            Screen::Organize => "Organize",
            Screen::History => "History",
        }
    }
    /// Iteration in which the screen gets real content.
    pub fn arrives_in(self) -> u8 {
        match self {
            Screen::Dashboard => 1,
            Screen::Browser => 3,
            Screen::Duplicates => 4,
            Screen::Organize => 6,
            Screen::History => 5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    ReadOnly,
    Write,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Quit,
    ToggleHelp,
    Back,
    Goto(Screen),
    Resize,
    Core(CoreEvent),
    /// Debug builds only (Ctrl-\): proves the terminal is restored after a panic.
    #[cfg(debug_assertions)]
    DebugPanic,
}

#[derive(Debug)]
pub struct App {
    pub screen: Screen,
    pub show_help: bool,
    pub mode: Mode,
    pub root: PathBuf,
    pub should_quit: bool,
}

impl App {
    pub fn new(root: PathBuf, mode: Mode) -> Self {
        Self {
            screen: Screen::Dashboard,
            show_help: false,
            mode,
            root,
            should_quit: false,
        }
    }

    /// Returns true when the screen needs a redraw.
    pub fn update(&mut self, msg: Msg) -> bool {
        match msg {
            Msg::Quit => {
                self.should_quit = true;
                true
            }
            Msg::ToggleHelp => {
                self.show_help = !self.show_help;
                true
            }
            Msg::Back => std::mem::take(&mut self.show_help),
            Msg::Goto(s) => {
                let changed = self.screen != s || self.show_help;
                self.screen = s;
                self.show_help = false;
                changed
            }
            Msg::Resize => true,
            Msg::Core(ev) => {
                tracing::debug!(?ev, "core event");
                false
            }
            #[cfg(debug_assertions)]
            #[allow(clippy::panic)]
            Msg::DebugPanic => panic!("debug panic requested (Ctrl-\\)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        App::new("/tmp".into(), Mode::ReadOnly)
    }

    #[test]
    fn q_quits() {
        let mut a = app();
        assert!(a.update(Msg::Quit));
        assert!(a.should_quit);
    }

    #[test]
    fn question_mark_toggles_help() {
        let mut a = app();
        a.update(Msg::ToggleHelp);
        assert!(a.show_help);
        a.update(Msg::ToggleHelp);
        assert!(!a.show_help);
    }

    #[test]
    fn three_switches_to_duplicates() {
        let mut a = app();
        a.update(Msg::Goto(Screen::Duplicates));
        assert_eq!(a.screen, Screen::Duplicates);
    }

    #[test]
    fn esc_closes_help_only_when_open() {
        let mut a = app();
        assert!(!a.update(Msg::Back));
        a.update(Msg::ToggleHelp);
        assert!(a.update(Msg::Back));
        assert!(!a.show_help);
    }
}
