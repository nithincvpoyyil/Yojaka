//! Plans. A plan is data: building or saving one never touches the user's media files.
//! (Executing a plan arrives in Iteration 5.)

pub mod tree;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use std::collections::HashSet;

use crate::dupes::{DupFile, DupGroup, Tier};
use crate::model::Fingerprint;
use crate::{Result, YojakaError};

/// Paths as JSON: a string when valid UTF-8, otherwise the raw bytes.
pub mod path_serde {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::path::{Path, PathBuf};

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    #[serde(untagged)]
    enum Repr {
        Text(String),
        Bytes(Vec<u8>),
    }

    pub fn serialize<S: Serializer>(p: &Path, s: S) -> Result<S::Ok, S::Error> {
        match p.to_str() {
            Some(t) => Repr::Text(t.to_owned()),
            None => Repr::Bytes(p.as_os_str().as_bytes().to_vec()),
        }
        .serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<PathBuf, D::Error> {
        Ok(match Repr::deserialize(d)? {
            Repr::Text(t) => PathBuf::from(t),
            Repr::Bytes(b) => PathBuf::from(std::ffi::OsString::from_vec(b)),
        })
    }
}

pub(crate) mod opt_path_serde {
    use std::path::{Path, PathBuf};

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(p: &Option<PathBuf>, s: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct W<'a>(#[serde(with = "super::path_serde")] &'a Path);
        p.as_deref().map(W).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<PathBuf>, D::Error> {
        #[derive(Deserialize)]
        struct W(#[serde(with = "super::path_serde")] PathBuf);
        Ok(Option::<W>::deserialize(d)?.map(|w| w.0))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanKind {
    Dedupe,
    Organize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpMode {
    Copy,
    Move,
    Trash,
    DeletePermanent,
}

/// Why a file is being removed: which duplicate group, and which file stays.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DupRef {
    pub group: u32,
    /// The copy that stays. For one half of a Live Photo, the matching half of the kept pair.
    #[serde(with = "path_serde")]
    pub kept: PathBuf,
    /// The other half of a Live Photo. Both halves must be acted on together or not at all.
    #[serde(default, with = "opt_path_serde")]
    pub partner: Option<PathBuf>,
    /// How sure the match is. Anything but `exact` means the kept copy is a *different* file
    /// that a person judged to be the same picture or clip.
    #[serde(default = "exact_tier", skip_serializing_if = "Tier::is_exact_ref")]
    pub tier: Tier,
    /// Non-exact tiers: what the kept copy looked like when the plan was made. It must still
    /// look like that when the plan runs (an exact copy is checked against the removed file).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kept_expect: Option<Fingerprint>,
}

fn exact_tier() -> Tier {
    Tier::Exact
}

fn fingerprint(f: &DupFile) -> Fingerprint {
    Fingerprint {
        size: f.size,
        mtime_ns: f.mtime_ns,
        hash: f.hash,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    Move {
        id: u64,
        #[serde(with = "path_serde")]
        src: PathBuf,
        #[serde(with = "path_serde")]
        dst: PathBuf,
        expect: Fingerprint,
        /// The destination folder came from a place borrowed from another file (marked `≈`).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        inferred: bool,
        /// Both halves of a Live Photo share this number: they are acted on together or not at all.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pair: Option<u32>,
    },
    Copy {
        id: u64,
        #[serde(with = "path_serde")]
        src: PathBuf,
        #[serde(with = "path_serde")]
        dst: PathBuf,
        expect: Fingerprint,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        inferred: bool,
        /// Both halves of a Live Photo share this number: they are acted on together or not at all.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pair: Option<u32>,
    },
    Trash {
        id: u64,
        #[serde(with = "path_serde")]
        path: PathBuf,
        expect: Fingerprint,
        reason: DupRef,
    },
    Delete {
        id: u64,
        #[serde(with = "path_serde")]
        path: PathBuf,
        expect: Fingerprint,
        reason: DupRef,
    },
    MkDir {
        id: u64,
        #[serde(with = "path_serde")]
        path: PathBuf,
    },
    Skip {
        id: u64,
        #[serde(with = "path_serde")]
        path: PathBuf,
        why: SkipReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    DestinationExists,
    AlreadyThere,
    SourceChanged,
    Dataless,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Warning {
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanSummary {
    pub ops: usize,
    /// Bytes the plan frees (dedupe) or moves/copies (organize).
    pub bytes: u64,
    pub skipped: usize,
    pub groups: usize,
    pub live_pairs: usize,
    /// Organize plans: moves whose destination is on another volume (copy, verify, then remove).
    #[serde(default)]
    pub cross_volume: usize,
    /// Organize plans: files whose folder uses an inferred place.
    #[serde(default)]
    pub inferred: usize,
    /// Organize plans: files placed in `_undated`.
    #[serde(default)]
    pub undated: usize,
    /// Organize plans: files placed in the "no location" folder.
    #[serde(default)]
    pub no_location: usize,
    /// Organize plans: files dated by their modification time because they have no recorded date.
    #[serde(default)]
    pub date_fallback: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub id: Uuid,
    pub created_at: i64,
    #[serde(with = "path_serde")]
    /// Where the files are now (the scanned folder).
    pub root: PathBuf,
    /// Organize plans: the folder the files go to.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "opt_path_serde"
    )]
    pub dest: Option<PathBuf>,
    pub kind: PlanKind,
    pub mode: OpMode,
    pub ops: Vec<Op>,
    pub summary: PlanSummary,
    pub warnings: Vec<Warning>,
}

impl Plan {
    pub(crate) fn empty(root: &Path, kind: PlanKind, mode: OpMode) -> Self {
        Self {
            id: Uuid::now_v7(),
            created_at: time::OffsetDateTime::now_utc().unix_timestamp(),
            root: root.to_path_buf(),
            dest: None,
            kind,
            mode,
            ops: Vec::new(),
            summary: PlanSummary::default(),
            warnings: Vec::new(),
        }
    }

    /// A dedupe plan from the marks on `groups`: one `Trash` op per marked file. Nothing is run.
    ///
    /// A group that marks every member is never planned (a warning is recorded instead), and a
    /// Live Photo pair always contributes both of its halves.
    pub fn from_dupes(root: &Path, groups: &[DupGroup]) -> Self {
        let mut plan = Self::empty(root, PlanKind::Dedupe, OpMode::Trash);
        let mut next_id = 1u64;
        // With several tiers a file can sit in more than one group. A file is removed at most
        // once, and a file one group keeps is never removed by another (nor the other way round).
        let mut kept_paths: HashSet<&Path> = HashSet::new();
        let mut removed: HashSet<&Path> = HashSet::new();
        for g in groups {
            if g.delete_count() == 0 {
                continue;
            }
            let Some(keeper) = g.keeper() else {
                plan.warnings.push(Warning {
                    message: format!(
                        "group {}: every copy was marked for deletion, so the group was left out",
                        g.id
                    ),
                });
                continue;
            };
            let keeper_files = &g.members[keeper].files;
            if let Some(f) = keeper_files
                .iter()
                .find(|f| removed.contains(f.path.as_path()))
            {
                plan.warnings.push(Warning {
                    message: format!(
                        "group {}: the copy to keep ({}) is removed by an earlier group, so the group was left out",
                        g.id,
                        f.path.display()
                    ),
                });
                continue;
            }
            let mut planned = false;
            for (item, _) in g
                .members
                .iter()
                .zip(&g.delete)
                .filter(|(_, marked)| **marked)
            {
                if item
                    .files
                    .iter()
                    .any(|f| removed.contains(f.path.as_path()))
                {
                    continue; // already removed by an earlier group
                }
                if let Some(f) = item
                    .files
                    .iter()
                    .find(|f| kept_paths.contains(f.path.as_path()))
                {
                    plan.warnings.push(Warning {
                        message: format!(
                            "group {}: {} is the copy another group keeps, so it stays",
                            g.id,
                            f.path.display()
                        ),
                    });
                    continue;
                }
                if !planned {
                    planned = true;
                    plan.summary.groups += 1;
                    kept_paths.extend(keeper_files.iter().map(|f| f.path.as_path()));
                }
                removed.extend(item.files.iter().map(|f| f.path.as_path()));
                let pair = item.files.len() > 1;
                if pair {
                    plan.summary.live_pairs += 1;
                }
                for (pos, f) in item.files.iter().enumerate() {
                    // The kept copy of this very file: for a Live Photo, the matching half.
                    let kept_file = keeper_files.get(pos).unwrap_or(&keeper_files[0]);
                    let kept = kept_file.path.clone();
                    let partner = item
                        .files
                        .iter()
                        .find(|o| o.id != f.id)
                        .map(|o| o.path.clone());
                    plan.ops.push(Op::Trash {
                        id: next_id,
                        path: f.path.clone(),
                        expect: fingerprint(f),
                        reason: DupRef {
                            group: g.id,
                            kept,
                            partner,
                            tier: g.tier,
                            kept_expect: (!g.tier.is_exact()).then(|| fingerprint(kept_file)),
                        },
                    });
                    next_id += 1;
                    plan.summary.bytes += f.size;
                }
            }
        }
        plan.summary.ops = plan.ops.len();
        plan
    }

    /// The same plan, but deleting for good instead of using the Trash. Cannot be undone.
    pub fn into_permanent(mut self) -> Self {
        self.mode = OpMode::DeletePermanent;
        for op in &mut self.ops {
            if let Op::Trash {
                id,
                path,
                expect,
                reason,
            } = op.clone()
            {
                *op = Op::Delete {
                    id,
                    path,
                    expect,
                    reason,
                };
            }
        }
        self
    }

    /// One line telling the user this run cannot be undone by Yojaka. Shown before they confirm,
    /// and on the History screen. Yojaka has no undo: the user decides, then it is done.
    pub fn no_undo_note(&self) -> &'static str {
        match self.mode {
            OpMode::DeletePermanent => "This deletes for good. Yojaka has no undo.",
            OpMode::Trash => "Yojaka has no undo. Files can be put back from the Trash in Finder.",
            OpMode::Move | OpMode::Copy => "Yojaka has no undo. Check the plan before you confirm.",
        }
    }

    /// Trash plans this big need the phrase typed, not just a "y".
    pub const LARGE_PLAN_FILES: usize = 50;

    pub fn is_permanent(&self) -> bool {
        self.mode == OpMode::DeletePermanent
    }

    /// Files the plan acts on (folders to create and skipped files do not count).
    pub fn file_ops(&self) -> usize {
        self.ops
            .iter()
            .filter(|o| {
                matches!(
                    o,
                    Op::Trash { .. } | Op::Delete { .. } | Op::Move { .. } | Op::Copy { .. }
                )
            })
            .count()
    }

    /// Removals where the copy that stays is not identical to the removed file (a likely,
    /// similar or name match that a person marked).
    pub fn lookalike_ops(&self) -> usize {
        self.ops
            .iter()
            .filter(|o| {
                matches!(o, Op::Trash { reason, .. } | Op::Delete { reason, .. }
                    if !reason.tier.is_exact())
            })
            .count()
    }

    /// A copy leaves the originals alone, so it only asks yes or no. A move or a permanent
    /// delete always needs the phrase typed; a Trash plan from `LARGE_PLAN_FILES` files.
    pub fn needs_typed_confirmation(&self) -> bool {
        match self.mode {
            OpMode::Copy => false,
            OpMode::Move | OpMode::DeletePermanent => true,
            OpMode::Trash => self.file_ops() >= Self::LARGE_PLAN_FILES,
        }
    }

    /// The words the user types to confirm, e.g. `trash 120 files` / `delete 4 files forever`.
    pub fn confirm_phrase(&self) -> String {
        let n = self.file_ops();
        let files = if n == 1 { "file" } else { "files" };
        match self.mode {
            OpMode::DeletePermanent => format!("delete {n} {files} forever"),
            OpMode::Move => format!("move {n} {files}"),
            OpMode::Copy => format!("copy {n} {files}"),
            OpMode::Trash => format!("trash {n} {files}"),
        }
    }

    /// Organize dry run: where the files will land, by folder (relative to the destination).
    pub fn destination_tree(&self) -> tree::DirTree {
        let mut t = tree::DirTree::new();
        let dest = self.dest.as_deref().unwrap_or(Path::new(""));
        for op in &self.ops {
            if let Op::Move { dst, expect, .. } | Op::Copy { dst, expect, .. } = op {
                t.add(dst.strip_prefix(dest).unwrap_or(dst), expect.size);
            }
        }
        t
    }

    /// A dry-run view: the files the plan removes, grouped by folder (relative to the plan root).
    pub fn removal_tree(&self) -> tree::DirTree {
        let mut t = tree::DirTree::new();
        for op in &self.ops {
            if let Op::Trash { path, expect, .. } | Op::Delete { path, expect, .. } = op {
                t.add(path.strip_prefix(&self.root).unwrap_or(path), expect.size);
            }
        }
        t
    }

    /// Writes the plan as JSON. Refuses to replace an existing file.
    pub fn save(&self, path: &Path) -> Result<()> {
        use std::io::Write;
        let json = serde_json::to_vec_pretty(self)
            .map_err(|e| YojakaError::Internal(format!("could not write the plan: {e}")))?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| YojakaError::io(path, e))?;
        file.write_all(&json)
            .map_err(|e| YojakaError::io(path, e))?;
        file.write_all(b"\n")
            .map_err(|e| YojakaError::io(path, e))?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path).map_err(|e| YojakaError::io(path, e))?;
        serde_json::from_slice(&bytes).map_err(|e| {
            YojakaError::Internal(format!("{} is not a valid plan: {e}", path.display()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Fingerprint;

    fn plan(mode: OpMode, files: usize) -> Plan {
        let mut p = Plan::from_dupes(Path::new("/p"), &[]);
        p.mode = mode;
        for i in 0..files {
            p.ops.push(Op::Trash {
                id: i as u64,
                path: format!("/p/{i}.jpg").into(),
                expect: Fingerprint {
                    size: 1,
                    mtime_ns: 0,
                    hash: None,
                },
                reason: DupRef {
                    group: 1,
                    kept: "/p/k.jpg".into(),
                    partner: None,
                    tier: crate::dupes::Tier::Exact,
                    kept_expect: None,
                },
            });
        }
        p
    }

    #[test]
    fn a_copy_asks_yes_or_no_and_a_move_always_wants_the_phrase() {
        assert!(!plan(OpMode::Copy, 5000).needs_typed_confirmation());
        assert!(plan(OpMode::Move, 1).needs_typed_confirmation());
        assert_eq!(plan(OpMode::Move, 1).confirm_phrase(), "move 1 file");
        let small = Plan::LARGE_PLAN_FILES - 1;
        assert!(!plan(OpMode::Trash, small).needs_typed_confirmation());
        assert!(plan(OpMode::Trash, Plan::LARGE_PLAN_FILES).needs_typed_confirmation());
        assert!(plan(OpMode::DeletePermanent, 1).needs_typed_confirmation());
    }
}
