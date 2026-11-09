//! Plans. A plan is data: building or saving one never touches the user's media files.
//! (Executing a plan arrives in Iteration 5.)

pub mod tree;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::dupes::DupGroup;
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
    },
    Copy {
        id: u64,
        #[serde(with = "path_serde")]
        src: PathBuf,
        #[serde(with = "path_serde")]
        dst: PathBuf,
        expect: Fingerprint,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub id: Uuid,
    pub created_at: i64,
    #[serde(with = "path_serde")]
    pub root: PathBuf,
    pub kind: PlanKind,
    pub mode: OpMode,
    pub ops: Vec<Op>,
    pub summary: PlanSummary,
    pub warnings: Vec<Warning>,
}

impl Plan {
    fn empty(root: &Path, kind: PlanKind, mode: OpMode) -> Self {
        Self {
            id: Uuid::now_v7(),
            created_at: time::OffsetDateTime::now_utc().unix_timestamp(),
            root: root.to_path_buf(),
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
            plan.summary.groups += 1;
            for (item, _) in g
                .members
                .iter()
                .zip(&g.delete)
                .filter(|(_, marked)| **marked)
            {
                let pair = item.files.len() > 1;
                if pair {
                    plan.summary.live_pairs += 1;
                }
                for (pos, f) in item.files.iter().enumerate() {
                    // The kept copy of this very file: for a Live Photo, the matching half.
                    let kept = keeper_files
                        .get(pos)
                        .unwrap_or(&keeper_files[0])
                        .path
                        .clone();
                    let partner = item
                        .files
                        .iter()
                        .find(|o| o.id != f.id)
                        .map(|o| o.path.clone());
                    plan.ops.push(Op::Trash {
                        id: next_id,
                        path: f.path.clone(),
                        expect: Fingerprint {
                            size: f.size,
                            mtime_ns: f.mtime_ns,
                            hash: Some(f.hash),
                        },
                        reason: DupRef {
                            group: g.id,
                            kept,
                            partner,
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

    /// Plans this big (or any permanent delete) need the phrase typed, not just a "y".
    pub const LARGE_PLAN_FILES: usize = 50;

    pub fn is_permanent(&self) -> bool {
        self.mode == OpMode::DeletePermanent
    }

    pub fn needs_typed_confirmation(&self) -> bool {
        self.is_permanent() || self.ops.len() >= Self::LARGE_PLAN_FILES
    }

    /// The words the user types to confirm, e.g. `trash 120 files` / `delete 4 files forever`.
    pub fn confirm_phrase(&self) -> String {
        let n = self.ops.len();
        let files = if n == 1 { "file" } else { "files" };
        if self.is_permanent() {
            format!("delete {n} {files} forever")
        } else {
            format!("trash {n} {files}")
        }
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
