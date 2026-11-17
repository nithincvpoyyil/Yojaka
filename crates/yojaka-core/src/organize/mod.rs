//! Organize: turn a path template into a plan of moves or copies (Iteration 6).

pub mod planner;
pub mod template;

pub use planner::{OrganizeOptions, OrganizePlan, OrganizeStats, Transfer, plan};
pub use template::{Facts, PRESETS, Rendered, Template, UNDATED_DIR, Where, Ymd, preset};

/// Refuses to copy into the scanned folder or a folder inside it (FR-43a): the next scan would
/// find every copy as a duplicate of its original. Moving there is allowed (tidying a folder in
/// place). Both paths must be canonical (or, for a destination that does not exist yet, its
/// nearest existing folder canonical and the rest as typed).
pub fn check_dest_outside(
    dest: &std::path::Path,
    root: &std::path::Path,
    transfer: Transfer,
) -> crate::Result<()> {
    if transfer == Transfer::Copy && dest.starts_with(root) {
        return Err(crate::YojakaError::Refused(
            "copies cannot go inside the scanned folder: choose a folder outside it, or move instead"
                .into(),
        ));
    }
    Ok(())
}

/// The folder pattern saved in `config.toml`. One that does not parse is ignored with a warning,
/// so a bad saved setting never stops the app.
pub fn configured_template(saved: &str) -> (String, Option<String>) {
    match Template::parse(saved) {
        Ok(_) => (saved.trim().to_string(), None),
        Err(e) => (
            PRESETS[0].1.to_string(),
            Some(format!("Saved folder pattern ignored: {e}")),
        ),
    }
}

/// `copy` or `move` as saved in `config.toml`. Anything else means copy, with a warning.
pub fn configured_transfer(saved: &str) -> (Transfer, Option<String>) {
    match saved.trim() {
        "copy" => (Transfer::Copy, None),
        "move" => (Transfer::Move, None),
        other => (
            Transfer::Copy,
            Some(format!(
                "Saved organize mode \"{other}\" ignored (use copy or move)"
            )),
        ),
    }
}

/// The word saved for a [`Transfer`].
pub fn transfer_word(t: Transfer) -> &'static str {
    match t {
        Transfer::Copy => "copy",
        Transfer::Move => "move",
    }
}

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn saved_settings_are_used_and_bad_ones_fall_back_with_a_warning() {
        let (t, w) = configured_template("{country}/{name}");
        assert_eq!((t.as_str(), w), ("{country}/{name}", None));
        let (t, w) = configured_template("{nope}/{name}");
        assert_eq!(t, PRESETS[0].1);
        assert!(w.unwrap().contains("nope"));
        assert_eq!(configured_transfer("move").0, Transfer::Move);
        let (t, w) = configured_transfer("teleport");
        assert_eq!(t, Transfer::Copy);
        assert!(w.unwrap().contains("teleport"));
    }
}
