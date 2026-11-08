//! Keep rules: which copy of a duplicate group stays. Rules chain: each one narrows the
//! candidates, and a final deterministic tie-break (shortest path bytes, then file id) always
//! leaves exactly one.

use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::str::FromStr;

use super::DupItem;

/// What `--keep` uses when none is given.
pub const DEFAULT_KEEP: &str = "oldest-modified,shortest-path";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeepRule {
    /// Earliest date taken (files without one lose to files with one).
    OldestTaken,
    /// Earliest modification time.
    OldestModified,
    NewestModified,
    ShortestPath,
    /// Files under this folder win, when any are.
    PreferPath(PathBuf),
    HighestResolution,
    LongestDuration,
    HighestBitrate,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "unknown keep rule `{0}` (try oldest-taken, oldest-modified, newest-modified, shortest-path, prefer-path:DIR, highest-resolution, longest-duration, highest-bitrate)"
)]
pub struct ParseKeepRuleError(pub String);

impl FromStr for KeepRule {
    type Err = ParseKeepRuleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if let Some(dir) = s.strip_prefix("prefer-path:") {
            if dir.is_empty() {
                return Err(ParseKeepRuleError(s.to_owned()));
            }
            return Ok(Self::PreferPath(PathBuf::from(dir)));
        }
        Ok(match s {
            "oldest-taken" => Self::OldestTaken,
            "oldest-modified" | "oldest" => Self::OldestModified,
            "newest-modified" | "newest" => Self::NewestModified,
            "shortest-path" => Self::ShortestPath,
            "highest-resolution" => Self::HighestResolution,
            "longest-duration" => Self::LongestDuration,
            "highest-bitrate" => Self::HighestBitrate,
            _ => return Err(ParseKeepRuleError(s.to_owned())),
        })
    }
}

/// Parses a chain such as `["prefer-path:/a", "oldest-modified,shortest-path"]` (commas split
/// too). An empty list gives the default chain.
pub fn parse_rules<S: AsRef<str>>(parts: &[S]) -> Result<Vec<KeepRule>, ParseKeepRuleError> {
    let mut rules = Vec::new();
    for part in parts {
        for piece in part.as_ref().split(',').filter(|p| !p.trim().is_empty()) {
            rules.push(piece.parse()?);
        }
    }
    if rules.is_empty() {
        return parse_rules(&[DEFAULT_KEEP]);
    }
    Ok(rules)
}

/// Narrows `cands` to the members with the smallest key, ignoring members without one.
/// If nobody has a key the candidates are left as they are.
fn narrow_by<K: Ord>(
    items: &[DupItem],
    cands: &mut Vec<usize>,
    key: impl Fn(&DupItem) -> Option<K>,
) {
    let best = cands.iter().filter_map(|&i| key(&items[i])).min();
    if let Some(best) = best {
        cands.retain(|&i| key(&items[i]).as_ref() == Some(&best));
    }
}

/// Index of the member to keep. Deterministic: the same group and rules always give the same answer.
pub fn pick_keeper(items: &[DupItem], rules: &[KeepRule]) -> usize {
    assert!(!items.is_empty());
    let mut cands: Vec<usize> = (0..items.len()).collect();
    for rule in rules {
        if cands.len() == 1 {
            break;
        }
        match rule {
            KeepRule::OldestTaken => narrow_by(items, &mut cands, |m| m.primary().taken_at),
            KeepRule::OldestModified => {
                narrow_by(items, &mut cands, |m| Some(m.primary().mtime_ns));
            }
            KeepRule::NewestModified => {
                narrow_by(items, &mut cands, |m| {
                    Some(std::cmp::Reverse(m.primary().mtime_ns))
                });
            }
            KeepRule::ShortestPath => narrow_by(items, &mut cands, |m| {
                Some(m.primary().path.as_os_str().len())
            }),
            KeepRule::PreferPath(dir) => {
                if cands
                    .iter()
                    .any(|&i| items[i].primary().path.starts_with(dir))
                {
                    cands.retain(|&i| items[i].primary().path.starts_with(dir));
                }
            }
            KeepRule::HighestResolution => narrow_by(items, &mut cands, |m| {
                let f = m.primary();
                Some(std::cmp::Reverse(
                    u64::from(f.width?) * u64::from(f.height?),
                ))
            }),
            KeepRule::LongestDuration => narrow_by(items, &mut cands, |m| {
                m.primary().duration_ms.map(std::cmp::Reverse)
            }),
            KeepRule::HighestBitrate => {
                narrow_by(items, &mut cands, |m| {
                    m.primary().bitrate.map(std::cmp::Reverse)
                });
            }
        }
    }
    cands
        .into_iter()
        .min_by(|&a, &b| {
            let (fa, fb) = (items[a].primary(), items[b].primary());
            fa.path
                .as_os_str()
                .as_bytes()
                .cmp(fb.path.as_os_str().as_bytes())
                .then(fa.id.cmp(&fb.id))
        })
        .unwrap_or(0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::dupes::DupFile;
    use crate::hash::ContentHash;
    use crate::model::{FileId, MediaKind};

    fn item(id: i64, path: &str, mtime: i64) -> DupItem {
        DupItem {
            files: vec![DupFile {
                id: FileId(id),
                path: PathBuf::from(path),
                size: 10,
                mtime_ns: mtime,
                kind: MediaKind::Image,
                taken_at: None,
                width: None,
                height: None,
                duration_ms: None,
                bitrate: None,
                hash: ContentHash([0; 32]),
            }],
        }
    }

    #[test]
    fn parses_chains_and_rejects_nonsense() {
        let r = parse_rules(&["prefer-path:/a/b,oldest", "shortest-path"]).unwrap();
        assert_eq!(
            r,
            vec![
                KeepRule::PreferPath("/a/b".into()),
                KeepRule::OldestModified,
                KeepRule::ShortestPath
            ]
        );
        assert!(parse_rules(&["bogus"]).is_err());
        assert!(parse_rules(&["prefer-path:"]).is_err());
        assert_eq!(parse_rules::<&str>(&[]).unwrap().len(), 2);
    }

    #[test]
    fn rules_chain_in_order() {
        let items = [
            item(1, "/p/a/IMG (1).jpg", 5),
            item(2, "/p/a/IMG.jpg", 5),
            item(3, "/p/b/IMG.jpg", 9),
        ];
        // Oldest narrows to 1 and 2, then the shorter path wins.
        let rules = parse_rules(&["oldest-modified", "shortest-path"]).unwrap();
        assert_eq!(pick_keeper(&items, &rules), 1);
        // Preferred folder first.
        let rules = parse_rules(&["prefer-path:/p/b", "oldest-modified"]).unwrap();
        assert_eq!(pick_keeper(&items, &rules), 2);
        // A preferred folder that matches nothing changes nothing.
        let rules = parse_rules(&["prefer-path:/zzz", "newest-modified"]).unwrap();
        assert_eq!(pick_keeper(&items, &rules), 2);
    }

    #[test]
    fn full_ties_break_by_path_then_id_regardless_of_order() {
        let mut items = vec![item(7, "/b.jpg", 1), item(3, "/a.jpg", 1)];
        let rules = parse_rules(&["oldest-modified"]).unwrap();
        let k = pick_keeper(&items, &rules);
        assert_eq!(items[k].primary().id, FileId(3));
        items.reverse();
        let k = pick_keeper(&items, &rules);
        assert_eq!(items[k].primary().id, FileId(3));
    }
}
