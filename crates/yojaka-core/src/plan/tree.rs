//! A folder tree built from a plan, and a short "glimpse" of it for the dry-run summary.
//! Shared by the CLI and the TUI review screen.

use std::collections::BTreeMap;
use std::path::{Component, Path};

#[derive(Debug, Default, Clone)]
struct Node {
    /// Everything at or below this folder.
    files: u64,
    bytes: u64,
    children: BTreeMap<String, Node>,
}

#[derive(Debug, Default, Clone)]
pub struct DirTree {
    root: Node,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlimpseLine {
    pub depth: usize,
    /// A folder path (single-child chains are joined: `2023/08`), or a "… N more" note.
    pub label: String,
    pub files: u64,
    pub bytes: u64,
    pub is_more: bool,
}

impl DirTree {
    pub fn new() -> Self {
        Self::default()
    }

    /// Counts one file of `bytes` at `path` (relative to the plan root) in all its folders.
    pub fn add(&mut self, path: &Path, bytes: u64) {
        let dirs: Vec<String> = path
            .parent()
            .into_iter()
            .flat_map(|p| p.components())
            .filter_map(|c| match c {
                Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect();
        let mut node = &mut self.root;
        node.files += 1;
        node.bytes += bytes;
        for d in dirs {
            node = node.children.entry(d).or_default();
            node.files += 1;
            node.bytes += bytes;
        }
    }

    pub fn files(&self) -> u64 {
        self.root.files
    }

    pub fn bytes(&self) -> u64 {
        self.root.bytes
    }

    /// At most `max_lines` lines. Each folder shows its `k` biggest children (by bytes), in
    /// calendar order (see [`calendar_key`]); the rest are summed into one "… N more folders"
    /// line. Single-child chains share a line.
    pub fn glimpse(&self, max_lines: usize, k: usize) -> Vec<GlimpseLine> {
        let mut out = Vec::new();
        render(&self.root, 0, max_lines, k, &mut out);
        out
    }
}

fn render(node: &Node, depth: usize, max: usize, k: usize, out: &mut Vec<GlimpseLine>) {
    let mut kids: Vec<(&String, &Node)> = node.children.iter().collect();
    kids.sort_by(|a, b| b.1.bytes.cmp(&a.1.bytes).then(a.0.cmp(b.0)));
    let hidden = kids.split_off(k.min(kids.len()));
    // The biggest are shown, in the order a person reads dates: 2022 before 2023, January
    // before April, then named folders A-Z.
    kids.sort_by_cached_key(|(name, _)| calendar_key(name));
    for (name, child) in kids {
        if out.len() >= max {
            return;
        }
        // Join a chain of folders that hold nothing directly and have one child.
        let (mut label, mut cur) = (name.clone(), child);
        while cur.children.len() == 1 {
            let (n, c) = cur.children.iter().next().unwrap_or_else(|| unreachable!());
            if cur.files != c.files {
                break; // files live directly in `cur`
            }
            label.push('/');
            label.push_str(n);
            cur = c;
        }
        out.push(GlimpseLine {
            depth,
            label,
            files: cur.files,
            bytes: cur.bytes,
            is_more: false,
        });
        render(cur, depth + 1, max, k, out);
    }
    if !hidden.is_empty() && out.len() < max {
        out.push(GlimpseLine {
            depth,
            label: format!("… {} more folders", hidden.len()),
            files: hidden.iter().map(|h| h.1.files).sum(),
            bytes: hidden.iter().map(|h| h.1.bytes).sum(),
            is_more: true,
        });
    }
}

/// Sort key that puts date folders in calendar order: numbers first (`2022`, `2023-April`,
/// `04`), by value and then month; then month names (`January` … `December`); then every other
/// name A-Z, ignoring case; `_undated` and other names starting with `_` last.
pub fn calendar_key(name: &str) -> (u8, u64, String) {
    let digits: String = name.chars().take_while(char::is_ascii_digit).collect();
    let rest = name[digits.len()..].trim_start_matches(['-', ' ', '_']);
    let month = |s: &str| crate::organize::month_number(s).map_or(0, u64::from);
    if let Ok(n) = digits.parse::<u64>() {
        return (0, n.saturating_mul(100) + month(rest), rest.to_lowercase());
    }
    if let Some(m) = crate::organize::month_number(name) {
        return (1, u64::from(m), String::new());
    }
    (
        if name.starts_with('_') { 3 } else { 2 },
        0,
        name.to_lowercase(),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn shown_folders_are_in_calendar_order() {
        let mut t = DirTree::new();
        // Bigger months first by size, so a sort by size would give September, July, April.
        for (m, n) in [("September", 5), ("July", 4), ("April", 3)] {
            for i in 0..n {
                t.add(Path::new(&format!("2025/{m}/{i}.jpg")), 10);
            }
        }
        t.add(Path::new("2026/January/a.jpg"), 10);
        t.add(Path::new("No Location/b.jpg"), 1000);
        let labels: Vec<String> = t.glimpse(40, 8).into_iter().map(|l| l.label).collect();
        assert_eq!(
            labels,
            [
                "2025",
                "April",
                "July",
                "September",
                "2026/January",
                "No Location"
            ]
        );
        let mut years = vec![
            "2023-April",
            "2023-January",
            "2022-May",
            "_undated",
            "Paris",
        ];
        years.sort_by_key(|n| calendar_key(n));
        assert_eq!(
            years,
            [
                "2022-May",
                "2023-January",
                "2023-April",
                "Paris",
                "_undated"
            ]
        );
    }

    #[test]
    fn chains_collapse_and_the_rest_is_summed() {
        let mut t = DirTree::new();
        t.add(Path::new("2023/08/a.jpg"), 10);
        t.add(Path::new("2023/08/b.jpg"), 10);
        t.add(Path::new("trips/x/y.jpg"), 5);
        for i in 0..4 {
            t.add(Path::new(&format!("misc{i}/f.jpg")), 1);
        }
        let g = t.glimpse(40, 2);
        assert_eq!(g[0].label, "2023/08");
        assert_eq!((g[0].files, g[0].bytes), (2, 20));
        assert_eq!(g[1].label, "trips/x");
        let more = g.last().unwrap();
        assert!(more.is_more && more.label == "… 4 more folders" && more.files == 4);
        assert_eq!(t.files(), 7);
    }

    #[test]
    fn output_never_exceeds_the_line_budget() {
        let mut t = DirTree::new();
        for i in 0..100 {
            t.add(Path::new(&format!("d{i}/e/f.jpg")), 1);
        }
        assert!(t.glimpse(5, 100).len() <= 5);
    }

    #[test]
    fn a_hundred_thousand_files_fit_in_forty_lines() {
        let mut t = DirTree::new();
        for i in 0..100_000u32 {
            t.add(
                Path::new(&format!(
                    "{}/{:02}/{}/f{i}.jpg",
                    2000 + i % 25,
                    i % 12 + 1,
                    i % 40
                )),
                1000,
            );
        }
        let g = t.glimpse(40, 8);
        assert!(g.len() <= 40, "{} lines", g.len());
        assert_eq!(t.files(), 100_000);
        assert!(
            g.iter().any(|l| l.is_more),
            "the rest is summarised, not dropped"
        );
    }
}
