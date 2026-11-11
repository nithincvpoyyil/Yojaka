//! Duplicate detection in confidence tiers (tech spec §6.3):
//!
//! - **exact**: identical bytes. The only tier keep rules may mark in bulk.
//! - **likely**: same date taken, camera and pixel size (photos), or same date, pixel size and
//!   length within a second (videos), but different bytes.
//! - **similar**: pictures that look alike (perceptual hash). Opt-in, because it is the only
//!   stage that decodes pixels.
//! - **name**: names that differ only by a copy suffix, with sizes within 5%.
//!
//! The non-exact tiers are guesses, so nothing in them is ever marked by a rule: a person marks
//! each group. Nothing here modifies a user file: results are data, and a [`Plan`] built from
//! them is only a proposal.
//!
//! [`Plan`]: crate::plan::Plan

mod exact;
mod keep;
mod name;
mod similar;
mod tiers;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use std::path::Path;

use crate::hash::ContentHash;
use crate::model::{FileId, MediaKind};
use crate::plan::path_serde;
use crate::{
    CancelToken, Config, CoreEvent, EventSink, FileIssue, Result, TaskKind, TaskOutcome,
    YojakaError,
};

pub use name::normalize as normalize_name;
pub use similar::{BkTree, Look, dhash_file, look_file};

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
    pub const ALL: [Tier; 4] = [Tier::Exact, Tier::Likely, Tier::Similar, Tier::Name];

    /// Identical bytes. Every other tier is a guess that a person has to confirm.
    pub fn is_exact(self) -> bool {
        self == Tier::Exact
    }

    pub(crate) fn is_exact_ref(&self) -> bool {
        self.is_exact()
    }

    /// Why the members of a group of this tier were put together, in plain words.
    pub fn reason(self) -> &'static str {
        match self {
            Tier::Exact => "identical files",
            Tier::Likely => "same date, camera, pixels",
            Tier::Name => "same name, similar size",
            Tier::Similar => "pictures look alike",
        }
    }

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
    pub camera: Option<String>,
    /// Hash of the whole file. Always known in the exact tier; in the other tiers only when an
    /// earlier search happened to read the file.
    pub hash: Option<ContentHash>,
}

impl DupFile {
    /// The recorded date and time (`2021-05-04 10:11:12`), as written by the camera.
    pub fn taken_text(&self) -> Option<String> {
        let t = time::OffsetDateTime::from_unix_timestamp(self.taken_at?).ok()?;
        Some(format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            t.year(),
            u8::from(t.month()),
            t.day(),
            t.hour(),
            t.minute(),
            t.second()
        ))
    }

    /// Width and height in pixels (`4032×3024`).
    pub fn pixels_text(&self) -> Option<String> {
        Some(format!("{}×{}", self.width?, self.height?))
    }
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

/// What a duplicate search looks for. The default is the exact tier only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DupOptions {
    /// Hash again even when a hash is saved (not used by default).
    pub rehash: bool,
    pub exact: bool,
    pub likely: bool,
    pub name: bool,
    /// Compare how pictures look. Decodes every supported picture once (then cached).
    pub similar: bool,
    /// Similar tier: how many of the 64 hash bits may differ. `None` uses the configured value.
    pub similar_distance: Option<u32>,
}

impl Default for DupOptions {
    fn default() -> Self {
        Self {
            rehash: false,
            exact: true,
            likely: false,
            name: false,
            similar: false,
            similar_distance: None,
        }
    }
}

impl DupOptions {
    /// Every tier that reads no pixels: exact, likely and name.
    pub fn all() -> Self {
        Self {
            likely: true,
            name: true,
            ..Self::default()
        }
    }

    /// Only `tier`.
    pub fn only(tier: Tier) -> Self {
        Self {
            exact: tier == Tier::Exact,
            likely: tier == Tier::Likely,
            name: tier == Tier::Name,
            similar: tier == Tier::Similar,
            ..Self::default()
        }
    }

    pub fn with_similar(mut self) -> Self {
        self.similar = true;
        self
    }
}

/// Items that are (exact tier) or may be (other tiers) the same thing. `delete[i]` marks `members[i]` for removal; at least one member
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

    /// Keeps exactly the member the rules pick and marks the others. Only identical files can
    /// be decided by a rule: a group of any other tier is left as it is (returns `false`), so
    /// that it is only ever marked by a person looking at it.
    pub fn apply_rules(&mut self, rules: &[KeepRule]) -> bool {
        if !self.tier.is_exact() {
            return false;
        }
        let keep = pick_keeper(&self.members, rules);
        self.set_keeper(keep);
        true
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
    /// Bytes of the user's files read. A raw photo in the similar tier counts in full, although
    /// only its preview (a few MB) is read from disk.
    pub bytes_read: u64,
    pub groups: u64,
    /// Total across the exact groups, with the default keep rules not applied yet.
    pub potential_reclaim: u64,
    pub exact_groups: u64,
    pub likely_groups: u64,
    pub name_groups: u64,
    pub similar_groups: u64,
    /// Similar tier: pictures decoded in this run.
    pub pictures_hashed: u64,
    /// Similar tier: size of those pictures (part of `bytes_read`).
    pub picture_bytes: u64,
    /// Similar tier: pictures whose look was already saved from an earlier run.
    pub pictures_cached: u64,
    /// Similar tier: pictures in a format that is not compared (none of the formats Yojaka knows).
    pub pictures_unsupported: u64,
    /// Similar tier: pictures that could not be decoded, or are too plain to compare.
    pub pictures_unusable: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DupReport {
    pub groups: Vec<DupGroup>,
    pub stats: DupStats,
    pub issues: Vec<FileIssue>,
}

/// Looks for duplicates in the index at `index_path`. The exact tier always runs (the other
/// tiers leave out files it already matched); `opts` decides which tiers are reported.
/// Cancelling returns `Err(Cancelled)`; hashes saved so far are kept for the next run.
pub fn find(
    config: &Config,
    opts: DupOptions,
    index_path: &Path,
    sink: &EventSink,
    cancel: &CancelToken,
) -> Result<DupReport> {
    sink.important(CoreEvent::TaskStarted {
        task: TaskKind::Hash,
        label: "Looking for duplicates".into(),
    });
    let result = tiers::find(config, opts, index_path, sink, cancel);
    let outcome = match &result {
        Ok(_) => TaskOutcome::Complete,
        Err(YojakaError::Cancelled) => TaskOutcome::Cancelled,
        Err(e) => TaskOutcome::Failed(e.to_string()),
    };
    sink.important(CoreEvent::TaskFinished {
        task: TaskKind::Hash,
        outcome,
    });
    result
}

impl DupReport {
    /// Applies `rules` to every exact group. Groups of the other tiers are never marked in bulk.
    pub fn apply_rules(&mut self, rules: &[KeepRule]) {
        for g in &mut self.groups {
            g.apply_rules(rules);
        }
    }

    pub fn reclaim(&self) -> u64 {
        self.groups.iter().map(DupGroup::reclaim).sum()
    }
}
