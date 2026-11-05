//! Terminal setup/restore. `ratatui::try_init` also installs a panic hook that restores.

#[derive(Debug)]
pub struct TerminalGuard {
    pub term: ratatui::DefaultTerminal,
}

impl TerminalGuard {
    pub fn enter() -> std::io::Result<Self> {
        Ok(Self {
            term: ratatui::try_init()?,
        })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        ratatui::restore();
    }
}
