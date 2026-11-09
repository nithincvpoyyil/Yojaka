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

    /// At most `max_lines` lines. Each folder shows its `k` biggest children (by bytes); the rest
    /// are summed into one "… N more folders" line. Single-child chains share a line.
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

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
}
