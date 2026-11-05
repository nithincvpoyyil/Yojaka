//! Tree generation and the manifest.

use std::collections::{BTreeMap, HashSet};
use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::{ExtendedColorType, ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

use crate::exif_writer::{ExifData, splice_into_jpeg};
use crate::rng::Rng;
use crate::{GenError, io};

pub const GENERATOR_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct Spec {
    pub files: usize,
    pub seed: u64,
    pub depth: usize,
    pub dupes: f64,
    pub corrupt: f64,
    pub gps: f64,
    pub heic: f64,
    pub hardlinks: usize,
    pub symlinks: usize,
    pub noise: usize,
}

impl Default for Spec {
    fn default() -> Self {
        Self {
            files: 1000,
            seed: 42,
            depth: 4,
            dupes: 0.10,
            corrupt: 0.01,
            gps: 0.6,
            heic: 0.0,
            hardlinks: 5,
            symlinks: 5,
            noise: 200,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Gps {
    pub lat: f64,
    pub lon: f64,
    pub expect_city: String,
    pub expect_cc: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// Relative to `tree/`
    pub path: String,
    /// `image` or `other`
    pub kind: String,
    pub format: Option<String>,
    pub size: u64,
    pub taken_at: Option<String>,
    pub camera: Option<String>,
    pub gps: Option<Gps>,
    pub dup_group: Option<u32>,
    pub name_group: Option<u32>,
    pub hardlink_of: Option<String>,
    pub symlink_to: Option<String>,
    pub corrupt: bool,
    pub should_index: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Expected {
    pub indexed_count: usize,
    pub exact_groups: Vec<Vec<String>>,
    pub by_format: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub seed: u64,
    pub generator_version: u32,
    pub files: Vec<Entry>,
    pub expected: Expected,
    #[serde(skip)]
    pub warnings: Vec<String>,
}

const PLACES: &[(&str, &str, f64, f64)] = &[
    ("Panaji", "IN", 15.4989, 73.8278),
    ("Paris", "FR", 48.8566, 2.3522),
    ("Tokyo", "JP", 35.6762, 139.6503),
    ("Sydney", "AU", -33.8688, 151.2093),
    ("New York City", "US", 40.7128, -74.0060),
    ("Cape Town", "ZA", -33.9249, 18.4241),
];
const CAMERAS: &[(&str, &str)] = &[
    ("Apple", "iPhone 15"),
    ("Google", "Pixel 8"),
    ("Canon", "EOS R6"),
];
const DIR_NAMES: &[&str] = &[
    "a", "b", "c", "d", "trips", "2023", "family", "misc", "raw", "old",
];

struct Gen<'a> {
    spec: &'a Spec,
    tree: PathBuf,
    rng: Rng,
    entries: Vec<Entry>,
    used: HashSet<String>,
    dirs: Vec<String>,
    warnings: Vec<String>,
}

pub fn generate(spec: &Spec, out: &Path) -> Result<Manifest, GenError> {
    prepare_out(out)?;
    let mut g = Gen {
        spec,
        tree: out.join("tree"),
        rng: Rng::new(spec.seed),
        entries: Vec::new(),
        used: HashSet::new(),
        dirs: Vec::new(),
        warnings: Vec::new(),
    };
    io(&g.tree, std::fs::create_dir_all(&g.tree))?;
    io(
        &out.join("outside"),
        std::fs::create_dir_all(out.join("outside")),
    )?;
    g.make_dirs();
    let uniques = g.make_uniques()?;
    g.make_dupes(&uniques)?;
    g.make_links(&uniques, out)?;
    g.make_noise_and_bait()?;
    let manifest = g.finish(out)?;
    Ok(manifest)
}

/// Only ever touch a directory that is empty or was produced by fixturegen.
fn prepare_out(out: &Path) -> Result<(), GenError> {
    if out.exists() {
        let empty = io(out, std::fs::read_dir(out))?.next().is_none();
        if !empty {
            if !out.join("manifest.json").is_file() {
                return Err(GenError::Other(format!(
                    "{} is not empty and not a fixturegen output; refusing to touch it",
                    out.display()
                )));
            }
            for sub in ["tree", "outside"] {
                let p = out.join(sub);
                if p.exists() {
                    io(&p, std::fs::remove_dir_all(&p))?;
                }
            }
            io(out, std::fs::remove_file(out.join("manifest.json")))?;
        }
    }
    io(out, std::fs::create_dir_all(out))
}

fn take_stem(path: &str) -> (&str, &str) {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').unwrap_or((name, ""))
}

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

impl Gen<'_> {
    fn make_dirs(&mut self) {
        let n = (self.spec.files / 20).max(8);
        self.dirs.push(String::new());
        for _ in 0..n {
            let depth = self.rng.below(self.spec.depth + 1);
            let parts: Vec<&str> = (0..depth)
                .map(|_| DIR_NAMES[self.rng.below(DIR_NAMES.len())])
                .collect();
            self.dirs.push(parts.join("/"));
        }
        self.dirs.sort();
        self.dirs.dedup();
    }

    fn write(&mut self, rel: &str, bytes: &[u8]) -> Result<(), GenError> {
        let full = self.tree.join(rel);
        if let Some(parent) = full.parent() {
            io(parent, std::fs::create_dir_all(parent))?;
        }
        io(&full, std::fs::write(&full, bytes))?;
        self.used.insert(rel.to_string());
        Ok(())
    }

    fn base_image(&mut self, idx: usize) -> RgbImage {
        let (r, g, b) = (
            self.rng.below(256) as u8,
            self.rng.below(256) as u8,
            self.rng.below(256) as u8,
        );
        let mut img = RgbImage::from_fn(64, 48, |x, y| {
            Rgb([
                r.wrapping_add((x * 3) as u8),
                g.wrapping_add((y * 4) as u8),
                b.wrapping_add((x + y) as u8),
            ])
        });
        // Encode the index into the first row so every image is distinct.
        for bit in 0..32u32 {
            let v = if (idx >> bit) & 1 == 1 { 255 } else { 0 };
            img.put_pixel(bit, 0, Rgb([v, v, v]));
        }
        img
    }

    fn encode_jpeg(&self, img: &RgbImage) -> Result<Vec<u8>, GenError> {
        let mut buf = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 85)
            .encode(
                img.as_raw(),
                img.width(),
                img.height(),
                ExtendedColorType::Rgb8,
            )
            .map_err(|e| GenError::Image(e.to_string()))?;
        Ok(buf)
    }

    fn encode_png(&self, img: &RgbImage) -> Result<Vec<u8>, GenError> {
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
            .map_err(|e| GenError::Image(e.to_string()))?;
        Ok(buf)
    }

    fn encode_gif(&mut self, img: &RgbImage, frames: u32) -> Result<Vec<u8>, GenError> {
        let mut buf = Vec::new();
        {
            let mut enc = image::codecs::gif::GifEncoder::new(&mut buf);
            for f in 0..frames {
                let rgba = RgbaImage::from_fn(img.width(), img.height(), |x, y| {
                    let p = img.get_pixel(x, y);
                    Rgba([p[0].wrapping_add((f * 40) as u8), p[1], p[2], 255])
                });
                enc.encode_frame(image::Frame::new(rgba))
                    .map_err(|e| GenError::Image(e.to_string()))?;
            }
        }
        Ok(buf)
    }

    fn heic_from_jpeg(&mut self, jpeg: &[u8]) -> Option<Vec<u8>> {
        let cache = directories::BaseDirs::new()?
            .home_dir()
            .join(".cache")
            .join("yojaka-fixtures");
        let key = blake3::hash(jpeg).to_hex();
        let cached = cache.join(format!("{}.heic", &key.as_str()[..32]));
        if let Ok(b) = std::fs::read(&cached) {
            return Some(b);
        }
        std::fs::create_dir_all(&cache).ok()?;
        let src = cache.join(format!("{}.jpg", &key.as_str()[..32]));
        std::fs::write(&src, jpeg).ok()?;
        let ok = std::process::Command::new("sips")
            .args(["-s", "format", "heic"])
            .arg(&src)
            .arg("--out")
            .arg(&cached)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        let _ = std::fs::remove_file(&src);
        if ok {
            std::fs::read(&cached).ok()
        } else {
            None
        }
    }

    fn make_uniques(&mut self) -> Result<Vec<usize>, GenError> {
        let mut uniques = Vec::new();
        let mut warned_heic = false;
        for i in 0..self.spec.files {
            let dir = self.dirs[self.rng.below(self.dirs.len())].clone();
            let roll = self.rng.below(100);
            let want_heic = self.spec.heic > 0.0 && self.rng.chance(self.spec.heic);
            let img = self.base_image(i);

            let year = self.rng.range(2015, 2024);
            let (mo, d) = (self.rng.range(1, 12), self.rng.range(1, 28));
            let (h, mi, s) = (
                self.rng.range(0, 23),
                self.rng.range(0, 59),
                self.rng.range(0, 59),
            );
            let taken_at = format!("{year:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}");
            let exif_dt = format!("{year:04}:{mo:02}:{d:02} {h:02}:{mi:02}:{s:02}");
            let (make, model) = CAMERAS[self.rng.below(CAMERAS.len())];
            let place = self
                .rng
                .chance(self.spec.gps)
                .then(|| PLACES[self.rng.below(PLACES.len())]);
            let corrupt = self.rng.chance(self.spec.corrupt);

            let (ext, format) = if roll < 70 {
                if want_heic {
                    ("heic", "heic")
                } else {
                    ("jpg", "jpeg")
                }
            } else if roll < 90 {
                ("png", "png")
            } else {
                ("gif", "gif")
            };
            let rel = join(&dir, &format!("IMG_{i:04}.{ext}"));

            let exif_capable = matches!(format, "jpeg" | "heic");
            let (bytes, has_meta) = if corrupt {
                (corrupt_bytes(format, &mut self.rng), false)
            } else if exif_capable {
                let jpeg = self.encode_jpeg(&img)?;
                let jpeg = splice_into_jpeg(
                    &jpeg,
                    &ExifData {
                        make,
                        model,
                        orientation: 1,
                        date_time_original: &exif_dt,
                        gps: place.map(|p| (p.2, p.3)),
                    },
                );
                if format == "heic" {
                    match self.heic_from_jpeg(&jpeg) {
                        Some(b) => (b, true),
                        None => {
                            if !warned_heic {
                                self.warnings.push("sips unavailable or failed; HEIC files written as JPEG content".into());
                                warned_heic = true;
                            }
                            (jpeg, true)
                        }
                    }
                } else {
                    (jpeg, true)
                }
            } else if format == "png" {
                (self.encode_png(&img)?, false)
            } else {
                let frames = if self.rng.chance(0.5) { 2 } else { 1 };
                (self.encode_gif(&img, frames)?, false)
            };

            self.write(&rel, &bytes)?;
            self.entries.push(Entry {
                path: rel,
                kind: "image".into(),
                format: Some(format.into()),
                size: bytes.len() as u64,
                taken_at: has_meta.then_some(taken_at),
                camera: has_meta.then(|| format!("{make} {model}")),
                gps: if has_meta {
                    place.map(|p| Gps {
                        lat: p.2,
                        lon: p.3,
                        expect_city: p.0.into(),
                        expect_cc: p.1.into(),
                    })
                } else {
                    None
                },
                dup_group: None,
                name_group: None,
                hardlink_of: None,
                symlink_to: None,
                corrupt,
                should_index: true,
            });
            uniques.push(self.entries.len() - 1);
        }
        Ok(uniques)
    }

    fn pick_other_dir(&mut self, avoid: &str) -> String {
        for _ in 0..20 {
            let d = &self.dirs[self.rng.below(self.dirs.len())];
            if d != avoid {
                return d.clone();
            }
        }
        format!("{avoid}/copies")
    }

    fn make_dupes(&mut self, uniques: &[usize]) -> Result<(), GenError> {
        let healthy: Vec<usize> = uniques
            .iter()
            .copied()
            .filter(|&i| !self.entries[i].corrupt)
            .collect();
        if healthy.is_empty() {
            return Ok(());
        }
        let n = (self.spec.files as f64 * self.spec.dupes).round() as usize;
        let mut name_groups = 0u32;
        let mut group_of: BTreeMap<usize, u32> = BTreeMap::new();

        for k in 0..n {
            let src = healthy[self.rng.below(healthy.len())];
            let src_path = self.entries[src].path.clone();
            let (stem, ext) = take_stem(&src_path);
            let (stem, ext) = (stem.to_string(), ext.to_string());
            let other = self.pick_other_dir(dir_of(&src_path));
            let name = match k % 3 {
                0 => format!("{stem} (1).{ext}"),
                1 => format!("{stem} copy.{ext}"),
                _ => format!("{stem}.{ext}"),
            };
            let rel = join(&other, &name);
            if self.used.contains(&rel) {
                continue;
            }
            let bytes = std::fs::read(self.tree.join(&src_path)).map_err(|e| GenError::Io {
                path: self.tree.join(&src_path),
                source: e,
            })?;
            self.write(&rel, &bytes)?;
            let mut e = self.entries[src].clone();
            e.path = rel;
            if k % 3 != 2 {
                let g = *group_of.entry(src).or_insert_with(|| {
                    name_groups += 1;
                    name_groups
                });
                self.entries[src].name_group = Some(g);
                e.name_group = Some(g);
            } else {
                e.name_group = None;
            }
            self.entries.push(e);
        }

        // Same-looking names, different bytes (for the name tier).
        let jpegs: Vec<usize> = healthy
            .iter()
            .copied()
            .filter(|&i| self.entries[i].format.as_deref() == Some("jpeg"))
            .collect();
        for k in 0..(n / 2) {
            if jpegs.is_empty() {
                break;
            }
            let src = jpegs[self.rng.below(jpegs.len())];
            let src_path = self.entries[src].path.clone();
            let (stem, _) = take_stem(&src_path);
            let rel = join(
                &self.pick_other_dir(dir_of(&src_path)),
                &format!("{stem} (2).jpg"),
            );
            if self.used.contains(&rel) {
                continue;
            }
            let img = self.base_image(self.spec.files + k);
            let bytes = self.encode_jpeg(&img)?;
            self.write(&rel, &bytes)?;
            let g = *group_of.entry(src).or_insert_with(|| {
                name_groups += 1;
                name_groups
            });
            self.entries[src].name_group = Some(g);
            self.entries.push(Entry {
                path: rel,
                kind: "image".into(),
                format: Some("jpeg".into()),
                size: bytes.len() as u64,
                taken_at: None,
                camera: None,
                gps: None,
                dup_group: None,
                name_group: Some(g),
                hardlink_of: None,
                symlink_to: None,
                corrupt: false,
                should_index: true,
            });
        }
        Ok(())
    }

    fn make_links(&mut self, uniques: &[usize], out: &Path) -> Result<(), GenError> {
        if uniques.is_empty() {
            return Ok(());
        }
        for k in 0..self.spec.hardlinks {
            let src = uniques[self.rng.below(uniques.len())];
            let src_path = self.entries[src].path.clone();
            let (stem, ext) = take_stem(&src_path);
            let rel = join(
                &self.pick_other_dir(dir_of(&src_path)),
                &format!("{stem} hardlink{k}.{ext}"),
            );
            if self.used.contains(&rel) {
                continue;
            }
            let full = self.tree.join(&rel);
            if let Some(p) = full.parent() {
                io(p, std::fs::create_dir_all(p))?;
            }
            io(&full, std::fs::hard_link(self.tree.join(&src_path), &full))?;
            self.used.insert(rel.clone());
            let mut e = self.entries[src].clone();
            e.path = rel;
            e.hardlink_of = Some(src_path);
            e.name_group = None;
            self.entries.push(e);
        }

        for k in 0..self.spec.symlinks {
            let inside = k % 2 == 0;
            let dir = self.dirs[self.rng.below(self.dirs.len())].clone();
            let rel = join(&dir, &format!("link{k}.jpg"));
            if self.used.contains(&rel) {
                continue;
            }
            let ups = if dir.is_empty() {
                0
            } else {
                dir.split('/').count()
            };
            let (target_rel, link_target) = if inside {
                let src = uniques[self.rng.below(uniques.len())];
                let tp = self.entries[src].path.clone();
                (tp.clone(), format!("{}{}", "../".repeat(ups), tp))
            } else {
                let name = format!("outside{k}.jpg");
                let img = self.base_image(1_000_000 + k);
                let bytes = self.encode_jpeg(&img)?;
                let p = out.join("outside").join(&name);
                io(&p, std::fs::write(&p, bytes))?;
                (
                    format!("../outside/{name}"),
                    format!("{}../outside/{name}", "../".repeat(ups)),
                )
            };
            let full = self.tree.join(&rel);
            if let Some(p) = full.parent() {
                io(p, std::fs::create_dir_all(p))?;
            }
            io(&full, std::os::unix::fs::symlink(&link_target, &full))?;
            self.used.insert(rel.clone());
            self.entries.push(Entry {
                path: rel,
                kind: "image".into(),
                format: Some("jpeg".into()),
                size: 0,
                taken_at: None,
                camera: None,
                gps: None,
                dup_group: None,
                name_group: None,
                hardlink_of: None,
                symlink_to: Some(target_rel),
                corrupt: false,
                should_index: false,
            });
        }
        Ok(())
    }

    fn make_noise_and_bait(&mut self) -> Result<(), GenError> {
        const EXTS: &[&str] = &["txt", "pdf", "json", "md"];
        for i in 0..self.spec.noise {
            let dir = self.dirs[self.rng.below(self.dirs.len())].clone();
            let ext = EXTS[i % EXTS.len()];
            let rel = join(&dir, &format!("note_{i:04}.{ext}"));
            let body = if ext == "pdf" {
                format!("%PDF-1.4\n% fixturegen noise {i}\n")
            } else {
                format!("fixturegen noise {i}\n")
            };
            self.write(&rel, body.as_bytes())?;
            self.entries.push(other_entry(rel, body.len() as u64));
        }
        // Skip-dir bait: images that must NOT be indexed.
        let img = self.base_image(2_000_000);
        let jpeg = self.encode_jpeg(&img)?;
        for rel in [
            "Test.photoslibrary/originals/IMG_9000.jpg",
            "Test.photoslibrary/IMG_9001.jpg",
            ".git/objects/IMG_9002.jpg",
        ] {
            self.write(rel, &jpeg)?;
            let mut e = other_entry(rel.to_string(), jpeg.len() as u64);
            e.kind = "image".into();
            e.format = Some("jpeg".into());
            self.entries.push(e);
        }
        Ok(())
    }

    fn finish(mut self, out: &Path) -> Result<Manifest, GenError> {
        self.entries.sort_by(|a, b| a.path.cmp(&b.path));

        // Ground truth: group by actual bytes among files that should be indexed.
        let mut by_hash: BTreeMap<[u8; 32], Vec<usize>> = BTreeMap::new();
        for (i, e) in self.entries.iter().enumerate() {
            if e.should_index && !e.corrupt {
                let full = self.tree.join(&e.path);
                let bytes = std::fs::read(&full).map_err(|s| GenError::Io {
                    path: full,
                    source: s,
                })?;
                by_hash
                    .entry(*blake3::hash(&bytes).as_bytes())
                    .or_default()
                    .push(i);
            }
        }
        let mut exact_groups: Vec<Vec<String>> = by_hash
            .into_values()
            .filter(|v| v.len() > 1)
            .map(|v| {
                v.into_iter()
                    .map(|i| self.entries[i].path.clone())
                    .collect::<Vec<_>>()
            })
            .collect();
        for g in &mut exact_groups {
            g.sort();
        }
        exact_groups.sort();
        for (gi, group) in exact_groups.iter().enumerate() {
            for p in group {
                if let Some(e) = self.entries.iter_mut().find(|e| &e.path == p) {
                    e.dup_group = Some(gi as u32 + 1);
                }
            }
        }

        let mut by_format: BTreeMap<String, usize> = BTreeMap::new();
        let mut indexed = 0;
        for e in self.entries.iter().filter(|e| e.should_index) {
            indexed += 1;
            if let Some(f) = &e.format {
                *by_format.entry(f.clone()).or_default() += 1;
            }
        }
        let manifest = Manifest {
            seed: self.spec.seed,
            generator_version: GENERATOR_VERSION,
            files: self.entries,
            expected: Expected {
                indexed_count: indexed,
                exact_groups,
                by_format,
            },
            warnings: self.warnings,
        };
        let path = out.join("manifest.json");
        let json =
            serde_json::to_string_pretty(&manifest).map_err(|e| GenError::Other(e.to_string()))?;
        io(&path, std::fs::write(&path, json))?;
        Ok(manifest)
    }
}

fn other_entry(path: String, size: u64) -> Entry {
    Entry {
        path,
        kind: "other".into(),
        format: None,
        size,
        taken_at: None,
        camera: None,
        gps: None,
        dup_group: None,
        name_group: None,
        hardlink_of: None,
        symlink_to: None,
        corrupt: false,
        should_index: false,
    }
}

/// Correct magic bytes followed by a truncated body.
fn corrupt_bytes(format: &str, rng: &mut Rng) -> Vec<u8> {
    let mut v: Vec<u8> = match format {
        "png" => vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
        "gif" => b"GIF89a".to_vec(),
        "heic" => vec![
            0, 0, 0, 0x18, b'f', b't', b'y', b'p', b'h', b'e', b'i', b'c',
        ],
        _ => vec![0xFF, 0xD8, 0xFF, 0xE0],
    };
    for _ in 0..12 {
        v.push(rng.below(256) as u8);
    }
    v
}
