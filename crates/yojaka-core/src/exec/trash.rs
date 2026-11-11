//! Moving files to the Trash, and putting them back.
//!
//! macOS gives no "restore" call, so the journal records where each file ended up and a failed
//! pair is put back by moving it itself. [`SystemTrash`] is the real Trash; [`DirTrash`] is a
//! folder standing in for it (tests, so they never fill the user's Trash).

use std::path::{Path, PathBuf};

use objc2_foundation::{NSFileManager, NSString, NSURL};

use crate::model::Fingerprint;
use crate::{Result, YojakaError};

pub trait Trasher: Send + Sync + std::fmt::Debug {
    /// Moves `path` to the Trash. Returns where it landed.
    fn trash(&self, path: &Path) -> Result<PathBuf>;

    /// Looks for a trashed copy of `original` (same name family, size and modification time).
    /// Used to recover after a crash between the move and the journal entry. Best effort.
    fn find(&self, original: &Path, expect: &Fingerprint) -> Option<PathBuf>;
}

/// The user's Trash, through `NSFileManager` (the same call Finder uses).
#[derive(Debug, Default)]
pub struct SystemTrash;

impl Trasher for SystemTrash {
    fn trash(&self, path: &Path) -> Result<PathBuf> {
        let text = path
            .to_str()
            .ok_or_else(|| trash_err(path, "the path is not valid UTF-8"))?;
        let url = NSURL::fileURLWithPath(&NSString::from_str(text));
        let mut result = None;
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, Some(&mut result))
            .map_err(|e| trash_err(path, &e.localizedDescription().to_string()))?;
        result
            .and_then(|u| u.path())
            .map(|p| PathBuf::from(p.to_string()))
            .ok_or_else(|| trash_err(path, "macOS did not say where the file went"))
    }

    fn find(&self, original: &Path, expect: &Fingerprint) -> Option<PathBuf> {
        let home = directories::BaseDirs::new()?.home_dir().to_path_buf();
        find_in(&home.join(".Trash"), original, expect)
    }
}

/// A folder used as the Trash.
#[derive(Debug)]
pub struct DirTrash(pub PathBuf);

impl Trasher for DirTrash {
    fn trash(&self, path: &Path) -> Result<PathBuf> {
        std::fs::create_dir_all(&self.0).map_err(|e| YojakaError::io(&self.0, e))?;
        let name = path.file_name().unwrap_or_default().to_owned();
        let name_text = name.to_string_lossy().into_owned();
        let (stem, ext) = split_name(&name_text);
        let mut n = 0u32;
        loop {
            let candidate = if n == 0 {
                self.0.join(&name)
            } else {
                self.0.join(format!("{stem} {n}{ext}"))
            };
            // `hard_link` refuses to replace anything, so nothing in the Trash is overwritten.
            match std::fs::hard_link(path, &candidate) {
                Ok(()) => {
                    std::fs::remove_file(path).map_err(|e| YojakaError::io(path, e))?;
                    return Ok(candidate);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
                Err(e) => return Err(YojakaError::io(path, e)),
            }
        }
    }

    fn find(&self, original: &Path, expect: &Fingerprint) -> Option<PathBuf> {
        find_in(&self.0, original, expect)
    }
}

/// Moves a trashed file back to `original`. Never replaces an existing file.
pub fn restore(trashed: &Path, original: &Path) -> Result<()> {
    if let Some(parent) = original.parent() {
        std::fs::create_dir_all(parent).map_err(|e| YojakaError::io(parent, e))?;
    }
    std::fs::hard_link(trashed, original).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            YojakaError::Internal(format!(
                "{} already exists, so the file was not put back",
                original.display()
            ))
        } else {
            YojakaError::io(original, e)
        }
    })?;
    std::fs::remove_file(trashed).map_err(|e| YojakaError::io(trashed, e))
}

fn trash_err(path: &Path, why: &str) -> YojakaError {
    YojakaError::Internal(format!(
        "could not move {} to the Trash: {why}",
        path.display()
    ))
}

/// `("photo", ".jpg")` from `"photo.jpg"`; `("photo", "")` when there is no extension.
fn split_name(name: &str) -> (&str, String) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], name[i..].to_owned()),
        _ => (name, String::new()),
    }
}

fn find_in(dir: &Path, original: &Path, expect: &Fingerprint) -> Option<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let name = original.file_name()?.to_string_lossy().into_owned();
    let (stem, ext) = split_name(&name);
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let n = entry.file_name().to_string_lossy().into_owned();
        if !n.starts_with(stem) || !n.ends_with(&ext) {
            continue;
        }
        let Ok(md) = entry.metadata() else { continue };
        let mtime = md.mtime() * 1_000_000_000 + md.mtime_nsec();
        if md.is_file() && md.len() == expect.size && mtime == expect.mtime_ns {
            return Some(entry.path());
        }
    }
    None
}
