//! Actions: ready-made jobs for the Actions tab (FR-70..74, plan `10-actions-plan.md`).
//!
//! Each action is an organize preset plus fixed choices: identical files are copied or moved
//! once, and places come from GPS only. The list lives here so the TUI, the CLI help and a later
//! GUI show the same jobs with the same words.

use crate::organize::{PRESETS, Transfer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Action {
    /// Stable name (also the CLI `--preset` value).
    pub id: &'static str,
    pub title: &'static str,
    /// What it does, in one or two plain sentences (without the identical-files sentence).
    pub description: &'static str,
    /// The label of its organize preset in [`PRESETS`].
    pub preset: &'static str,
    /// Example paths inside the destination, shown before a plan is ready.
    pub examples: [&'static str; 3],
}

pub const ACTIONS: [Action; 6] = [
    Action {
        id: "merge",
        title: "Merge into one folder",
        description: "Every photo in one folder, every video in another. A second file with the \
                      same name gets (1).",
        preset: "Merge",
        examples: [
            "Photos/IMG_0001.heic",
            "Photos/IMG_0001 (1).jpg",
            "Videos/MVI_0042.mov",
        ],
    },
    Action {
        id: "year",
        title: "By year",
        description: "One folder per year, from the date taken (a file without one goes by its \
                      file date).",
        preset: "Year",
        examples: ["2022/IMG_0001.heic", "2023/DSC_0042.jpg", "2024/scan.png"],
    },
    Action {
        id: "date",
        title: "By year and month",
        description: "A folder per year, with a folder per month inside.",
        preset: "Date",
        examples: [
            "2023/August/IMG_0001.heic",
            "2023/September/DSC_0042.jpg",
            "2024/January/MVI_0007.mov",
        ],
    },
    Action {
        id: "year-place",
        title: "By year and place",
        description: "Year first, then country and town from the GPS. Files without GPS go to No Location.",
        preset: "Year + Place",
        examples: [
            "2023/India/Panaji/IMG_0001.heic",
            "2023/France/Paris/DSC_0042.jpg",
            "2023/No Location/MVI_0007.mov",
        ],
    },
    Action {
        id: "place",
        title: "By place",
        description: "Country and town from the GPS. Files without GPS go to No Location.",
        preset: "Place",
        examples: [
            "India/Panaji/IMG_0001.heic",
            "France/Paris/DSC_0042.jpg",
            "No Location/MVI_0007.mov",
        ],
    },
    Action {
        id: "format",
        title: "By file format",
        description: "Photos and videos apart, then one folder per format.",
        preset: "Format",
        examples: [
            "Photos/HEIC/IMG_0001.heic",
            "Photos/JPEG/DSC_0042.jpg",
            "Videos/MOV/MVI_0007.mov",
        ],
    },
];

impl Action {
    /// The folder pattern this action uses.
    pub fn template(&self) -> &'static str {
        crate::organize::preset(self.preset).unwrap_or(PRESETS[0].1)
    }

    /// The sentence about identical files, which depends on copy or move.
    pub fn identical_note(transfer: Transfer) -> &'static str {
        match transfer {
            Transfer::Copy => "Identical files are copied once.",
            Transfer::Move => {
                "Identical files are moved once; the other copies stay where they are."
            }
        }
    }

    /// Whether this action needs places (GPS).
    pub fn uses_place(&self) -> bool {
        self.template().contains("{country}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::organize::Template;

    #[test]
    fn every_action_names_a_preset_that_parses() {
        for a in ACTIONS {
            assert!(
                crate::organize::preset(a.preset).is_some(),
                "{}: no preset {}",
                a.id,
                a.preset
            );
            Template::parse(a.template()).unwrap_or_else(|e| panic!("{}: {e}", a.id));
        }
        assert_eq!(ACTIONS[0].template(), "{kind}/{name}");
        assert!(ACTIONS.iter().any(Action::uses_place));
        assert!(!ACTIONS[0].uses_place());
    }

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<&str> = ACTIONS.iter().map(|a| a.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), ACTIONS.len());
    }
}
