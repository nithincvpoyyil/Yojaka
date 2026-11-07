//! Live Photo pairing. A pair is one item and is never split.
//!
//! 1. Same Apple content identifier, one image and one video in the same folder.
//! 2. Fallback: a `.MOV` and an image in the same folder with the same stem and capture times
//!    within 2 seconds, when neither carries a conflicting identifier.
//!
//! Anything ambiguous is left unpaired: a missed pair is harmless, a false pair is not.

use std::collections::HashMap;

use rusqlite::{Connection, params};

use crate::Result;

/// Shared `content.identifier` of a HEIC/JPEG + MOV pair.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PairId(pub String);

/// Largest capture-time gap for the name-and-time fallback.
pub const FALLBACK_WINDOW_SECS: i64 = 2;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PairStats {
    pub by_content_id: u64,
    pub by_name_and_time: u64,
}

impl PairStats {
    pub fn total(&self) -> u64 {
        self.by_content_id + self.by_name_and_time
    }
}

#[derive(Debug, Clone)]
struct Cand {
    id: i64,
    folder: Vec<u8>,
    stem: String,
    content_id: Option<String>,
    taken_at: Option<i64>,
}

fn stem(name: &[u8]) -> String {
    let s = String::from_utf8_lossy(name).to_lowercase();
    match s.rfind('.') {
        Some(i) if i > 0 => s[..i].to_string(),
        _ => s,
    }
}

/// Recomputes every `meta.pair_id` from scratch. The pair id is the image's file id.
pub fn pair(conn: &mut Connection) -> Result<PairStats> {
    let mut images = Vec::new();
    let mut videos = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT f.id, f.parent_folder, f.file_name, f.format, m.content_id, m.taken_at \
             FROM meta m JOIN file f ON f.id = m.file_id \
             WHERE f.format IN ('heic','jpeg','mov')",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                Cand {
                    id: r.get(0)?,
                    folder: r.get(1)?,
                    stem: stem(&r.get::<_, Vec<u8>>(2)?),
                    content_id: r.get(4)?,
                    taken_at: r.get(5)?,
                },
                r.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (c, format) = row?;
            if format == "mov" {
                videos.push(c);
            } else {
                images.push(c);
            }
        }
    }

    let mut pairs: Vec<(i64, i64)> = Vec::new(); // (image id, video id)
    let mut stats = PairStats::default();
    let mut image_taken = vec![false; images.len()];
    let mut video_taken = vec![false; videos.len()];

    // Step 1: content identifier. Try (id, folder) first, then (id, folder, stem) for the
    // folders that hold several copies.
    for with_stem in [false, true] {
        type Key = (String, Vec<u8>, Option<String>);
        let key = |c: &Cand, cid: &str| -> Key {
            (
                cid.to_string(),
                c.folder.clone(),
                with_stem.then(|| c.stem.clone()),
            )
        };
        let mut groups: HashMap<Key, (Vec<usize>, Vec<usize>)> = HashMap::new();
        for (i, c) in images.iter().enumerate().filter(|(i, _)| !image_taken[*i]) {
            if let Some(cid) = &c.content_id {
                groups.entry(key(c, cid)).or_default().0.push(i);
            }
        }
        for (i, c) in videos.iter().enumerate().filter(|(i, _)| !video_taken[*i]) {
            if let Some(cid) = &c.content_id {
                groups.entry(key(c, cid)).or_default().1.push(i);
            }
        }
        for (ims, vids) in groups.values() {
            if let ([i], [v]) = (ims.as_slice(), vids.as_slice()) {
                pairs.push((images[*i].id, videos[*v].id));
                image_taken[*i] = true;
                video_taken[*v] = true;
                stats.by_content_id += 1;
            }
        }
    }

    // Step 2: same folder + stem + capture time within the window.
    let mut by_name: HashMap<(&[u8], &str), Vec<usize>> = HashMap::new();
    for (i, c) in images.iter().enumerate().filter(|(i, _)| !image_taken[*i]) {
        by_name.entry((&c.folder, &c.stem)).or_default().push(i);
    }
    let mut claims: HashMap<usize, Vec<usize>> = HashMap::new(); // image index -> video indexes
    for (v, vid) in videos.iter().enumerate().filter(|(v, _)| !video_taken[*v]) {
        let Some(vt) = vid.taken_at else { continue };
        let matches: Vec<usize> = by_name
            .get(&(vid.folder.as_slice(), vid.stem.as_str()))
            .into_iter()
            .flatten()
            .copied()
            .filter(|&i| {
                let img = &images[i];
                let conflicting = matches!((&img.content_id, &vid.content_id), (Some(_), Some(_)));
                !conflicting
                    && img
                        .taken_at
                        .is_some_and(|it| (it - vt).abs() <= FALLBACK_WINDOW_SECS)
            })
            .collect();
        if let [i] = matches.as_slice() {
            claims.entry(*i).or_default().push(v);
        }
    }
    for (i, vs) in claims {
        if let [v] = vs.as_slice() {
            pairs.push((images[i].id, videos[*v].id));
            stats.by_name_and_time += 1;
        }
    }

    let tx = conn.transaction()?;
    tx.execute("UPDATE meta SET pair_id=NULL WHERE pair_id IS NOT NULL", [])?;
    {
        let mut set = tx.prepare("UPDATE meta SET pair_id=?1 WHERE file_id IN (?1, ?2)")?;
        for (img, vid) in &pairs {
            set.execute(params![img, vid])?;
        }
    }
    tx.commit()?;
    Ok(stats)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::index::Index;

    struct Db(Index);

    impl Db {
        fn new() -> Self {
            let idx = Index::open_in_memory().unwrap();
            idx.connection()
                .execute(
                    "INSERT INTO scan(root_path,started_at,status) VALUES(x'2f',0,'complete')",
                    [],
                )
                .unwrap();
            Self(idx)
        }

        /// Adds a file + meta row and returns its file id.
        fn add(
            &self,
            folder: &str,
            name: &str,
            format: &str,
            cid: Option<&str>,
            taken: Option<i64>,
        ) -> i64 {
            let c = self.0.connection();
            let kind = if format == "mov" { "video" } else { "image" };
            let path = format!("{folder}/{name}");
            c.execute(
                "INSERT INTO file(full_path,parent_folder,file_name,size_bytes,modified_at_ns,device_id,inode,\
                 media_kind,format,last_seen_scan_id) VALUES(?1,?2,?3,1,0,1,abs(random()),?4,?5,1)",
                params![path.as_bytes(), folder.as_bytes(), name.as_bytes(), kind, format],
            )
            .unwrap();
            let id = c.last_insert_rowid();
            c.execute(
                "INSERT INTO meta(file_id,taken_at,taken_at_src,content_id) VALUES(?1,?2,'exif',?3)",
                params![id, taken, cid],
            )
            .unwrap();
            id
        }

        fn pairs(&mut self) -> (PairStats, Vec<Vec<i64>>) {
            let stats = pair(self.0.connection_mut_for_tests()).unwrap();
            let c = self.0.connection();
            let mut st = c
                .prepare("SELECT pair_id, GROUP_CONCAT(file_id) FROM meta WHERE pair_id IS NOT NULL GROUP BY pair_id")
                .unwrap();
            let mut groups: Vec<Vec<i64>> = st
                .query_map([], |r| {
                    let s: String = r.get(1)?;
                    let mut ids: Vec<i64> = s.split(',').map(|x| x.parse().unwrap()).collect();
                    ids.sort();
                    Ok(ids)
                })
                .unwrap()
                .map(Result::unwrap)
                .collect();
            groups.sort();
            (stats, groups)
        }
    }

    #[test]
    fn same_content_id_pairs() {
        let mut db = Db::new();
        let i = db.add("/p", "IMG_1.HEIC", "heic", Some("A"), Some(100));
        let v = db.add("/p", "IMG_1.MOV", "mov", Some("A"), Some(100));
        let (s, g) = db.pairs();
        assert_eq!(g, vec![vec![i, v]]);
        assert_eq!(s.by_content_id, 1);
    }

    #[test]
    fn copies_in_other_folders_pair_separately() {
        let mut db = Db::new();
        let (i1, v1) = (
            db.add("/a", "IMG_1.HEIC", "heic", Some("A"), Some(1)),
            db.add("/a", "IMG_1.MOV", "mov", Some("A"), Some(1)),
        );
        let (i2, v2) = (
            db.add("/b", "IMG_1.HEIC", "heic", Some("A"), Some(1)),
            db.add("/b", "IMG_1.MOV", "mov", Some("A"), Some(1)),
        );
        let (_, g) = db.pairs();
        assert_eq!(g, vec![vec![i1, v1], vec![i2, v2]]);
    }

    #[test]
    fn name_and_time_fallback_respects_the_window() {
        let mut db = Db::new();
        let i = db.add("/p", "IMG_1.JPG", "jpeg", None, Some(1000));
        let v = db.add("/p", "img_1.mov", "mov", None, Some(1002)); // 2 s: pairs, case-insensitive
        db.add("/p", "IMG_2.JPG", "jpeg", None, Some(1000));
        db.add("/p", "IMG_2.MOV", "mov", None, Some(1003)); // 3 s: too far
        db.add("/p", "IMG_3.JPG", "jpeg", None, Some(1000));
        db.add("/q", "IMG_3.MOV", "mov", None, Some(1000)); // other folder
        let (s, g) = db.pairs();
        assert_eq!(g, vec![vec![i, v]]);
        assert_eq!(s.by_name_and_time, 1);
    }

    #[test]
    fn conflicting_ids_and_missing_dates_never_pair() {
        let mut db = Db::new();
        db.add("/p", "IMG_1.HEIC", "heic", Some("A"), Some(5));
        db.add("/p", "IMG_1.MOV", "mov", Some("B"), Some(5));
        db.add("/p", "IMG_2.HEIC", "heic", None, None);
        db.add("/p", "IMG_2.MOV", "mov", None, None);
        let (s, g) = db.pairs();
        assert!(g.is_empty(), "{g:?}");
        assert_eq!(s.total(), 0);
    }

    #[test]
    fn ambiguity_leaves_everything_unpaired() {
        let mut db = Db::new();
        // two stills with the same stem and one video
        db.add("/p", "IMG_1.HEIC", "heic", None, Some(10));
        db.add("/p", "IMG_1.JPG", "jpeg", None, Some(10));
        db.add("/p", "IMG_1.MOV", "mov", None, Some(10));
        // one still, two videos claiming it
        db.add("/p", "IMG_2.HEIC", "heic", None, Some(10));
        db.add("/p", "IMG_2.MOV", "mov", None, Some(10));
        db.add("/p", "img_2.MOV", "mov", None, Some(11));
        let (_, g) = db.pairs();
        assert!(g.is_empty(), "{g:?}");
    }

    #[test]
    fn pairing_is_recomputed_from_scratch() {
        let mut db = Db::new();
        let i = db.add("/p", "IMG_1.HEIC", "heic", Some("A"), Some(1));
        let v = db.add("/p", "IMG_1.MOV", "mov", Some("A"), Some(1));
        assert_eq!(db.pairs().1, vec![vec![i, v]]);
        db.0.connection()
            .execute("DELETE FROM file WHERE id=?1", [v])
            .unwrap();
        assert!(
            db.pairs().1.is_empty(),
            "a pair whose half vanished must dissolve"
        );
    }
}
