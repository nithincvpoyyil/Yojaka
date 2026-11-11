//! Read-only queries behind the TUI file browser: folder tree, filtered/sorted pages, one file's details.
//!
//! Everything here only reads the index. Paged queries use `LIMIT/OFFSET` so a UI can fetch just
//! the rows it shows (the index is never loaded into memory, except the folder tree, which has
//! one node per folder, not per file).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, params_from_iter, types::Value};

use crate::Result;
use crate::index::Index;
use crate::model::MediaKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KindFilter {
    #[default]
    All,
    Photos,
    Videos,
}

impl KindFilter {
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Photos,
            Self::Photos => Self::Videos,
            Self::Videos => Self::All,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Photos => "photos",
            Self::Videos => "videos",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    /// Full path, so files stay grouped by folder.
    #[default]
    Name,
    Kind,
    Format,
    Duration,
    Size,
    Taken,
    Camera,
    Place,
}

impl SortKey {
    pub const ALL: [SortKey; 8] = [
        Self::Name,
        Self::Kind,
        Self::Format,
        Self::Duration,
        Self::Size,
        Self::Taken,
        Self::Camera,
        Self::Place,
    ];
    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|k| *k == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Kind => "kind",
            Self::Format => "format",
            Self::Duration => "duration",
            Self::Size => "size",
            Self::Taken => "taken",
            Self::Camera => "camera",
            Self::Place => "place",
        }
    }
    fn sql(self) -> &'static str {
        match self {
            Self::Name => "f.full_path",
            Self::Kind => "f.media_kind",
            Self::Format => "f.format",
            Self::Duration => "m.duration_ms",
            Self::Size => "f.size_bytes",
            Self::Taken => "m.taken_at",
            Self::Camera => "COALESCE(m.camera_model, m.camera_make)",
            Self::Place => "COALESCE(m.place_city, m.place_country)",
        }
    }
}

/// What the file table shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BrowseQuery {
    /// Only files at or below this folder.
    pub folder: Option<PathBuf>,
    pub kind: KindFilter,
    /// Case-insensitive "contains" on file name, camera and place. Empty means no filter.
    pub text: String,
    pub sort: SortKey,
    pub descending: bool,
}

/// One row of the file table.
#[derive(Debug, Clone, PartialEq)]
pub struct FileRow {
    pub id: i64,
    pub path: PathBuf,
    pub name: String,
    pub kind: MediaKind,
    pub format: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
    pub size_bytes: u64,
    /// Unix seconds of the recorded wall-clock time.
    pub taken_at: Option<i64>,
    pub camera: Option<String>,
    /// "City, Country". GPS coordinates are never part of a row.
    pub place: Option<String>,
    /// The place was inferred from neighbours, not read from the file's own GPS (shown with `≈`).
    pub place_inferred: bool,
    /// Half of a Live Photo pair.
    pub live: bool,
    pub cloud_only: bool,
}

/// Everything known about one file (the detail pane).
#[derive(Debug, Clone, PartialEq)]
pub struct FileDetail {
    pub row: FileRow,
    pub modified_at_ns: i64,
    pub taken_at_src: Option<String>,
    pub lens: Option<String>,
    pub fps: Option<f64>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub bitrate: Option<u64>,
    pub container: Option<String>,
    pub frames: Option<u64>,
    pub has_gps: bool,
    pub loc_src: Option<String>,
    pub metadata_status: String,
    /// Path of the other half of a Live Photo.
    pub live_partner: Option<PathBuf>,
}

fn path_of(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(OsStr::from_bytes(&bytes))
}

/// "Make Model" without repeating the make when the model already starts with it.
fn camera_label(make: Option<String>, model: Option<String>) -> Option<String> {
    match (make, model) {
        (Some(a), Some(b)) if b.to_lowercase().starts_with(&a.to_lowercase()) => Some(b),
        (Some(a), Some(b)) => Some(format!("{a} {b}")),
        (Some(a), None) => Some(a),
        (None, b) => b,
    }
}

fn place_label(city: Option<String>, country: Option<String>) -> Option<String> {
    match (city, country) {
        (Some(a), Some(b)) => Some(format!("{a}, {b}")),
        (a, b) => a.or(b),
    }
}

fn like_pattern(text: &str) -> String {
    let mut s = String::from("%");
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            s.push('\\');
        }
        s.push(c);
    }
    s.push('%');
    s
}

const COLUMNS: &str = "SELECT f.id, f.full_path, f.file_name, f.media_kind, f.format, \
     m.width, m.height, m.duration_ms, f.size_bytes, m.taken_at, m.camera_make, m.camera_model, \
     m.place_city, m.place_country, m.loc_src, m.pair_id, f.is_cloud_only";
const SELECT_ROW: &str = "SELECT f.id, f.full_path, f.file_name, f.media_kind, f.format, \
     m.width, m.height, m.duration_ms, f.size_bytes, m.taken_at, m.camera_make, m.camera_model, \
     m.place_city, m.place_country, m.loc_src, m.pair_id, f.is_cloud_only \
     FROM file f LEFT JOIN meta m ON m.file_id=f.id";

/// Sorts that follow an index, so shallow pages need no sorting at all.
fn index_ordered(sort: SortKey) -> bool {
    matches!(sort, SortKey::Name | SortKey::Size | SortKey::Kind)
}

/// Pages starting deeper than this (or sorted by a column without an index) are served from a
/// temporary table holding the whole result in order, which is built once per query.
const DIRECT_MAX_OFFSET: u64 = 2_000;

fn row_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<FileRow> {
    let kind: String = r.get(3)?;
    Ok(FileRow {
        id: r.get(0)?,
        path: path_of(r.get(1)?),
        name: String::from_utf8_lossy(&r.get::<_, Vec<u8>>(2)?).into_owned(),
        kind: if kind == "video" {
            MediaKind::Video
        } else {
            MediaKind::Image
        },
        format: r.get(4)?,
        width: r.get::<_, Option<i64>>(5)?.map(|v| v as u32),
        height: r.get::<_, Option<i64>>(6)?.map(|v| v as u32),
        duration_ms: r.get::<_, Option<i64>>(7)?.map(|v| v as u64),
        size_bytes: r.get::<_, i64>(8)? as u64,
        taken_at: r.get(9)?,
        camera: camera_label(r.get(10)?, r.get(11)?),
        place: place_label(r.get(12)?, r.get(13)?),
        place_inferred: r.get::<_, Option<String>>(14)?.as_deref() == Some("inferred"),
        live: r.get::<_, Option<i64>>(15)?.is_some(),
        cloud_only: r.get::<_, i64>(16)? != 0,
    })
}

/// A node of the folder tree.
#[derive(Debug, Clone)]
pub struct FolderNode {
    pub path: PathBuf,
    /// Last path component (the root of `/` keeps its full path).
    pub name: String,
    pub parent: Option<usize>,
    /// Indexes into [`FolderTree::nodes`], sorted by name.
    pub children: Vec<usize>,
    /// Files directly in this folder.
    pub files_direct: u64,
    /// Files in this folder and everything below it.
    pub files_total: u64,
}

/// Every folder that holds (or leads to) indexed files. `nodes[0]` is the scan root.
#[derive(Debug, Clone, Default)]
pub struct FolderTree {
    pub nodes: Vec<FolderNode>,
}

impl FolderTree {
    /// Builds the tree from `(folder, direct file count)` pairs. Folders outside `root` are ignored.
    pub fn build(root: &Path, folders: impl IntoIterator<Item = (PathBuf, u64)>) -> Self {
        let mut nodes = vec![FolderNode {
            path: root.to_path_buf(),
            name: root.file_name().map_or_else(
                || root.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            ),
            parent: None,
            children: vec![],
            files_direct: 0,
            files_total: 0,
        }];
        let mut by_path: BTreeMap<PathBuf, usize> = BTreeMap::new();
        by_path.insert(root.to_path_buf(), 0);
        for (folder, n) in folders {
            let Ok(rel) = folder.strip_prefix(root) else {
                continue;
            };
            let mut cur = 0usize;
            let mut path = root.to_path_buf();
            for comp in rel.components() {
                path.push(comp);
                cur = match by_path.get(&path) {
                    Some(&i) => i,
                    None => {
                        let i = nodes.len();
                        nodes.push(FolderNode {
                            path: path.clone(),
                            name: comp.as_os_str().to_string_lossy().into_owned(),
                            parent: Some(cur),
                            children: vec![],
                            files_direct: 0,
                            files_total: 0,
                        });
                        nodes[cur].children.push(i);
                        by_path.insert(path.clone(), i);
                        i
                    }
                };
            }
            nodes[cur].files_direct += n;
        }
        // Children were created in an arbitrary order; sort them, then add up the totals
        // bottom-up (a child always has a larger index than its parent).
        for i in 0..nodes.len() {
            let mut kids = std::mem::take(&mut nodes[i].children);
            kids.sort_by_key(|&k| nodes[k].name.to_lowercase());
            nodes[i].children = kids;
        }
        for i in (0..nodes.len()).rev() {
            nodes[i].files_total += nodes[i].files_direct;
            if let Some(p) = nodes[i].parent {
                let t = nodes[i].files_total;
                nodes[p].files_total += t;
            }
        }
        Self { nodes }
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// A read-only handle on one index.
#[derive(Debug)]
pub struct Browser {
    index: Index,
    root: PathBuf,
    /// The query whose sorted ids are in the temp table `browse_order`, if any.
    ordered: RefCell<Option<BrowseQuery>>,
    direct_max_offset: u64,
}

impl Browser {
    pub fn open(index_path: &Path, root: PathBuf) -> Result<Self> {
        Ok(Self {
            index: Index::open(index_path)?,
            root,
            ordered: RefCell::new(None),
            direct_max_offset: DIRECT_MAX_OFFSET,
        })
    }

    /// Serve every page from the sorted temp table (tests compare both paths).
    #[doc(hidden)]
    pub fn always_materialize(mut self) -> Self {
        self.direct_max_offset = 0;
        self
    }

    /// Sorts the whole result of `q` once into the temp table `browse_order`
    /// (`pos` = 1-based rank), so any page afterwards is a primary-key range read.
    fn materialize(&self, q: &BrowseQuery) -> Result<()> {
        if self.ordered.borrow().as_ref() == Some(q) {
            return Ok(());
        }
        *self.ordered.borrow_mut() = None;
        let (where_sql, params) = Self::filter(q);
        let dir = if q.descending { "DESC" } else { "ASC" };
        let conn = self.conn();
        conn.execute_batch(
            "DROP TABLE IF EXISTS temp.browse_order; \
             CREATE TEMP TABLE browse_order(pos INTEGER PRIMARY KEY, id INTEGER NOT NULL)",
        )?;
        // Rows get increasing `pos` in the order the SELECT produces them.
        conn.execute(
            &format!(
                "INSERT INTO browse_order(id) SELECT f.id FROM file f \
                 LEFT JOIN meta m ON m.file_id=f.id{where_sql} ORDER BY {} {dir}, f.id {dir}",
                q.sort.sql()
            ),
            params_from_iter(params.iter()),
        )?;
        *self.ordered.borrow_mut() = Some(q.clone());
        Ok(())
    }

    fn conn(&self) -> &Connection {
        self.index.connection()
    }

    /// `WHERE` clause (with a leading space) plus its parameters, numbered as pushed.
    pub(crate) fn filter(q: &BrowseQuery) -> (String, Vec<Value>) {
        let mut clauses: Vec<String> = vec![];
        let mut params: Vec<Value> = vec![];
        if let Some(folder) = &q.folder {
            // Paths are raw bytes; "dir/" .. "dir0" ('0' is '/' + 1) covers the whole subtree on the
            // UNIQUE index of `full_path`.
            let mut lo = folder.as_os_str().as_bytes().to_vec();
            if lo.last() != Some(&b'/') {
                lo.push(b'/');
            }
            let mut hi = lo.clone();
            if let Some(last) = hi.last_mut() {
                *last = b'0';
            }
            params.push(Value::Blob(lo));
            params.push(Value::Blob(hi));
            clauses.push("f.full_path >= ?1 AND f.full_path < ?2".into());
        }
        match q.kind {
            KindFilter::All => {}
            KindFilter::Photos => clauses.push("f.media_kind='image'".into()),
            KindFilter::Videos => clauses.push("f.media_kind='video'".into()),
        }
        let text = q.text.trim();
        if !text.is_empty() {
            params.push(Value::Text(like_pattern(text)));
            let n = params.len();
            clauses.push(format!(
                "(CAST(f.file_name AS TEXT) LIKE ?{n} ESCAPE '\\' OR m.camera_make LIKE ?{n} ESCAPE '\\' \
                 OR m.camera_model LIKE ?{n} ESCAPE '\\' OR m.place_city LIKE ?{n} ESCAPE '\\' \
                 OR m.place_country LIKE ?{n} ESCAPE '\\')"
            ));
        }
        let sql = if clauses.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", clauses.join(" AND "))
        };
        (sql, params)
    }

    /// Number of rows `q` matches.
    pub fn count(&self, q: &BrowseQuery) -> Result<u64> {
        let (where_sql, params) = Self::filter(q);
        let join = if q.text.trim().is_empty() {
            ""
        } else {
            " LEFT JOIN meta m ON m.file_id=f.id"
        };
        let sql = format!("SELECT COUNT(*) FROM file f{join}{where_sql}");
        let n: i64 = self
            .conn()
            .query_row(&sql, params_from_iter(params.iter()), |r| r.get(0))?;
        Ok(n as u64)
    }

    /// `limit` rows of `q` starting at row `offset`.
    pub fn page(&self, q: &BrowseQuery, offset: u64, limit: u64) -> Result<Vec<FileRow>> {
        let ready = self.ordered.borrow().as_ref() == Some(q);
        let direct = !ready && index_ordered(q.sort) && offset <= self.direct_max_offset;
        if direct {
            let (where_sql, params) = Self::filter(q);
            let dir = if q.descending { "DESC" } else { "ASC" };
            // `id` as the tie-breaker keeps pages stable between requests.
            let sql = format!(
                "{SELECT_ROW}{where_sql} ORDER BY {} {dir}, f.id {dir} LIMIT {limit} OFFSET {offset}",
                q.sort.sql()
            );
            let mut st = self.conn().prepare_cached(&sql)?;
            let rows = st.query_map(params_from_iter(params.iter()), row_from)?;
            return Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?);
        }
        self.materialize(q)?;
        let sql = format!(
            "{COLUMNS} FROM browse_order o JOIN file f ON f.id=o.id \
             LEFT JOIN meta m ON m.file_id=f.id WHERE o.pos > ?1 AND o.pos <= ?2 ORDER BY o.pos"
        );
        let mut st = self.conn().prepare_cached(&sql)?;
        let rows = st.query_map(
            [offset as i64, offset.saturating_add(limit) as i64],
            row_from,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The folder tree of the whole index.
    pub fn folders(&self) -> Result<FolderTree> {
        let mut st = self
            .conn()
            .prepare("SELECT parent_folder, COUNT(*) FROM file GROUP BY parent_folder")?;
        let rows = st.query_map([], |r| Ok((path_of(r.get(0)?), r.get::<_, i64>(1)? as u64)))?;
        let folders = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(FolderTree::build(&self.root, folders))
    }

    /// Full details of one file, or `None` when it is no longer in the index.
    pub fn detail(&self, id: i64) -> Result<Option<FileDetail>> {
        let sql = format!("{SELECT_ROW} WHERE f.id=?1");
        let row = match self.conn().query_row(&sql, [id], row_from) {
            Ok(r) => r,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let extra = self.conn().query_row(
            "SELECT f.modified_at_ns, m.taken_at_src, m.lens, m.fps, m.video_codec, m.audio_codec, \
             m.bitrate, m.container, m.frames, COALESCE(m.has_gps,0), m.loc_src, f.metadata_status, m.pair_id \
             FROM file f LEFT JOIN meta m ON m.file_id=f.id WHERE f.id=?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<f64>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                    r.get::<_, Option<String>>(7)?,
                    r.get::<_, Option<i64>>(8)?,
                    r.get::<_, i64>(9)?,
                    r.get::<_, Option<String>>(10)?,
                    r.get::<_, String>(11)?,
                    r.get::<_, Option<i64>>(12)?,
                ))
            },
        )?;
        let live_partner = match extra.12 {
            Some(pair) => self
                .conn()
                .query_row(
                    "SELECT f.full_path FROM meta m JOIN file f ON f.id=m.file_id \
                     WHERE m.pair_id=?1 AND f.id<>?2 LIMIT 1",
                    [pair, id],
                    |r| r.get::<_, Vec<u8>>(0),
                )
                .ok()
                .map(path_of),
            None => None,
        };
        Ok(Some(FileDetail {
            row,
            modified_at_ns: extra.0,
            taken_at_src: extra.1.filter(|s| s != "none"),
            lens: extra.2,
            fps: extra.3,
            video_codec: extra.4,
            audio_codec: extra.5,
            bitrate: extra.6.map(|v| v as u64),
            container: extra.7,
            frames: extra.8.map(|v| v as u64),
            has_gps: extra.9 != 0,
            loc_src: extra.10,
            metadata_status: extra.11,
            live_partner,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(paths: &[(&str, u64)]) -> FolderTree {
        FolderTree::build(
            Path::new("/r"),
            paths.iter().map(|(p, n)| (PathBuf::from(p), *n)),
        )
    }

    #[test]
    fn folder_tree_adds_up_children() {
        let t = tree(&[("/r", 1), ("/r/a", 2), ("/r/a/x", 3), ("/r/b", 4)]);
        assert_eq!(t.nodes[0].files_total, 10);
        assert_eq!(t.nodes[0].files_direct, 1);
        let a = t.nodes.iter().find(|n| n.name == "a").unwrap();
        assert_eq!(a.files_total, 5);
        assert_eq!(a.children.len(), 1);
    }

    #[test]
    fn folder_tree_sorts_children_ignoring_case() {
        let t = tree(&[("/r/b", 1), ("/r/A", 1), ("/r/c", 1)]);
        let names: Vec<_> = t.nodes[0]
            .children
            .iter()
            .map(|&i| t.nodes[i].name.as_str())
            .collect();
        assert_eq!(names, ["A", "b", "c"]);
    }

    #[test]
    fn folder_tree_ignores_folders_outside_the_root() {
        let t = tree(&[("/other/x", 5), ("/r/a", 1)]);
        assert_eq!(t.nodes[0].files_total, 1);
    }

    #[test]
    fn like_pattern_escapes_wildcards() {
        assert_eq!(like_pattern("a%b_c"), "%a\\%b\\_c%");
    }

    #[test]
    fn camera_label_does_not_repeat_the_make() {
        assert_eq!(
            camera_label(Some("Sony".into()), Some("SONY ILCE-7M3".into())).as_deref(),
            Some("SONY ILCE-7M3")
        );
        assert_eq!(
            camera_label(Some("Apple".into()), Some("iPhone 15".into())).as_deref(),
            Some("Apple iPhone 15")
        );
        assert_eq!(camera_label(None, None), None);
    }

    #[test]
    fn sort_and_kind_cycle_through_everything() {
        let mut s = SortKey::Name;
        for _ in 0..SortKey::ALL.len() {
            s = s.next();
        }
        assert_eq!(s, SortKey::Name);
        assert_eq!(KindFilter::All.next().next().next(), KindFilter::All);
    }
}
