//! Duplicate detection. Iteration 4 finds the **exact** tier (identical bytes); the likely and
//! name tiers come later. Nothing here modifies a user file: results are data, and a [`Plan`]
//! built from them is only a proposal.
//!
//! [`Plan`]: crate::plan::Plan

mod exact;
mod keep;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::FileIssue;
use crate::hash::ContentHash;
use crate::model::{FileId, MediaKind};
use crate::plan::path_serde;

pub use exact::{DupOptions, find_exact};
pub use keep::{DEFAULT_KEEP, KeepRule, ParseKeepRuleError, parse_rules, pick_keeper};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Exact,
    Likely,
    Name,
    Similar,
}

impl Tier {
    pub fn label(self) -> &'static str {
        match self {
            Tier::Exact => "exact",
            Tier::Likely => "likely",
            Tier::Name => "name",
            Tier::Similar => "similar",
        }
    }
}

/// One file inside a duplicate group, with what the keep rules and the plan need.
#[derive(Debug, Clone, Serialize)]
pub struct DupFile {
    pub id: FileId,
    #[serde(with = "path_serde")]
    pub path: PathBuf,
    pub size: u64,
    pub mtime_ns: i64,
    pub kind: MediaKind,
    pub taken_at: Option<i64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<i64>,
    pub bitrate: Option<i64>,
    pub hash: ContentHash,
}

/// One thing that can be kept or deleted: a single file, or both halves of a Live Photo
/// (image first, then video). The halves are never separated.
#[derive(Debug, Clone, Serialize)]
pub struct DupItem {
    pub files: Vec<DupFile>,
}

impl DupItem {
    /// The file that names the item: the image of a Live Photo pair.
    pub fn primary(&self) -> &DupFile {
        &self.files[0]
    }

    pub fn is_pair(&self) -> bool {
        self.files.len() > 1
    }

    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

/// Items with identical content. `delete[i]` marks `members[i]` for removal; at least one member
/// is always left unmarked.
#[derive(Debug, Clone, Serialize)]
pub struct DupGroup {
    pub id: u32,
    pub tier: Tier,
    pub members: Vec<DupItem>,
    pub delete: Vec<bool>,
}

impl DupGroup {
    /// A group with nothing marked yet. Panics if fewer than two members are given.
    pub fn new(id: u32, tier: Tier, members: Vec<DupItem>) -> Self {
        assert!(members.len() >= 2, "a duplicate group needs two members");
        let delete = vec![false; members.len()];
        Self {
            id,
            tier,
            members,
            delete,
        }
    }

    /// Keeps exactly the member the rules pick and marks the others.
    pub fn apply_rules(&mut self, rules: &[KeepRule]) {
        let keep = pick_keeper(&self.members, rules);
        self.set_keeper(keep);
    }

    /// Keeps `index` and marks every other member.
    pub fn set_keeper(&mut self, index: usize) {
        for (i, d) in self.delete.iter_mut().enumerate() {
            *d = i != index;
        }
    }

    /// Flips the mark on `index`. Refuses to mark the last member that is still kept.
    /// Returns whether anything changed.
    pub fn toggle(&mut self, index: usize) -> bool {
        let Some(&marked) = self.delete.get(index) else {
            return false;
        };
        if !marked && self.delete.iter().filter(|d| !**d).count() <= 1 {
            return false;
        }
        self.delete[index] = !marked;
        true
    }

    /// Clears all marks.
    pub fn keep_all(&mut self) {
        self.delete.iter_mut().for_each(|d| *d = false);
    }

    pub fn delete_count(&self) -> usize {
        self.delete.iter().filter(|d| **d).count()
    }

    /// The first member that is kept, or `None` if (against the invariant) all are marked.
    pub fn keeper(&self) -> Option<usize> {
        self.delete.iter().position(|d| !*d)
    }

    /// Bytes freed by deleting the marked members.
    pub fn reclaim(&self) -> u64 {
        self.members
            .iter()
            .zip(&self.delete)
            .filter(|(_, d)| **d)
            .map(|(m, _)| m.total_size())
            .sum()
    }

    /// Bytes freed if all but one member were deleted (what the group is worth).
    pub fn potential_reclaim(&self) -> u64 {
        let total: u64 = self.members.iter().map(DupItem::total_size).sum();
        total
            - self
                .members
                .iter()
                .map(DupItem::total_size)
                .max()
                .unwrap_or(0)
    }
}

/// What a duplicate search read and found.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DupStats {
    /// Files whose size is shared with another file.
    pub candidate_files: u64,
    /// Sum of the sizes of those files.
    pub candidate_bytes: u64,
    /// Extra names for a file that is already counted (hard links); never grouped.
    pub hardlinks_skipped: u64,
    pub partial_hashed: u64,
    pub full_hashed: u64,
    /// Hashes that were already saved from an earlier run.
    pub cached_hashes: u64,
    /// Bytes actually read from the user's files.
    pub bytes_read: u64,
    pub groups: u64,
    /// Total across the groups, with the default keep rules not applied yet.
    pub potential_reclaim: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DupReport {
    pub groups: Vec<DupGroup>,
    pub stats: DupStats,
    pub issues: Vec<FileIssue>,
}

impl DupReport {
    /// Applies `rules` to every group.
    pub fn apply_rules(&mut self, rules: &[KeepRule]) {
        for g in &mut self.groups {
            g.apply_rules(rules);
        }
    }

    pub fn reclaim(&self) -> u64 {
        self.groups.iter().map(DupGroup::reclaim).sum()
    }
}
