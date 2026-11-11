//! Moving and copying one file without ever replacing anything.
//!
//! Every step that puts a file at its final name uses `hard_link`, which fails when the name is
//! taken, so a race with another program cannot lose a file. Copies go to a temporary name next to
//! the destination, are checked against the source (BLAKE3), and only then linked into place.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use crate::hash::{ContentHash, full_hash};
use crate::{Result, YojakaError};

const EXDEV: i32 = 18;

fn tmp_name(dst: &Path, op: u64) -> PathBuf {
    let name = dst.file_name().unwrap_or_default().to_string_lossy();
    dst.with_file_name(format!(".{name}.yojaka-{op}.tmp"))
}

/// Gives `dst` the content of `src`. Never replaces an existing `dst`. Returns the content hash
/// (of the source, equal to the copy's).
pub fn copy_verified(
    src: &Path,
    dst: &Path,
    op: u64,
    size: u64,
    mtime_ns: i64,
) -> Result<ContentHash> {
    let tmp = tmp_name(dst, op);
    // `reflink_or_copy` would replace a leftover, so refuse one.
    if tmp.symlink_metadata().is_ok() {
        std::fs::remove_file(&tmp).map_err(|e| YojakaError::io(&tmp, e))?;
    }
    let result = (|| -> Result<ContentHash> {
        // Same volume on APFS: an instant clone. Elsewhere: a normal copy.
        reflink_copy::reflink_or_copy(src, &tmp).map_err(|e| YojakaError::io(src, e))?;
        let f = OpenOptions::new()
            .write(true)
            .open(&tmp)
            .map_err(|e| YojakaError::io(&tmp, e))?;
        f.sync_all().map_err(|e| YojakaError::io(&tmp, e))?;
        if mtime_ns > 0 {
            let when = UNIX_EPOCH + Duration::from_nanos(mtime_ns as u64);
            f.set_modified(when).map_err(|e| YojakaError::io(&tmp, e))?;
        }
        drop(f);
        let want = full_hash(src, size)
            .map_err(|e| YojakaError::io(src, e))?
            .hash;
        let got = full_hash(&tmp, size)
            .map_err(|e| YojakaError::io(&tmp, e))?
            .hash;
        if want != got {
            return Err(YojakaError::Internal(format!(
                "the copy of {} did not match the original, so it was discarded",
                src.display()
            )));
        }
        link_new(&tmp, dst)?;
        Ok(want)
    })();
    // The temporary name goes in every case; on success the final name holds the same inode.
    let _ = std::fs::remove_file(&tmp);
    result
}

/// `hard_link` that says what went wrong in words.
pub fn link_new(from: &Path, to: &Path) -> Result<()> {
    std::fs::hard_link(from, to).map_err(|e| {
        if e.kind() == io::ErrorKind::AlreadyExists {
            YojakaError::Internal(format!("{} already exists", to.display()))
        } else {
            YojakaError::io(to, e)
        }
    })
}

fn is_cross_device(e: &YojakaError) -> bool {
    matches!(e, YojakaError::Io { source, .. } if source.raw_os_error() == Some(EXDEV))
}

/// How a move was done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moved {
    /// Same volume: the file got a new name, nothing was copied.
    Renamed,
    /// Another volume: copied, checked, then the source removed. Carries the content hash.
    Copied(ContentHash),
}

/// Moves `src` to `dst` without replacing anything.
pub fn move_file(src: &Path, dst: &Path, op: u64, size: u64, mtime_ns: i64) -> Result<Moved> {
    match link_new(src, dst) {
        Ok(()) => {
            std::fs::remove_file(src).map_err(|e| YojakaError::io(src, e))?;
            Ok(Moved::Renamed)
        }
        Err(e) if is_cross_device(&e) => {
            let hash = copy_verified(src, dst, op, size, mtime_ns)?;
            // The source must be what was copied (not edited meanwhile) before it goes.
            let md = std::fs::symlink_metadata(src).map_err(|e| YojakaError::io(src, e))?;
            if md.len() != size {
                let _ = std::fs::remove_file(dst);
                return Err(YojakaError::Internal(format!(
                    "{} changed while it was being copied; nothing was removed",
                    src.display()
                )));
            }
            std::fs::remove_file(src).map_err(|e| YojakaError::io(src, e))?;
            Ok(Moved::Copied(hash))
        }
        Err(e) => Err(e),
    }
}

/// Puts a moved file back (when its Live Photo partner failed): puts `dst` back at `src` (never over an existing file).
pub fn move_back(dst: &Path, src: &Path, op: u64, size: u64, mtime_ns: i64) -> Result<()> {
    if let Some(parent) = src.parent() {
        std::fs::create_dir_all(parent).map_err(|e| YojakaError::io(parent, e))?;
    }
    move_file(dst, src, op, size, mtime_ns).map(|_| ())
}

/// Is `path` a regular file of `size` bytes (and, when known, with this content)?
pub fn is_same_file(path: &Path, size: u64, hash: Option<ContentHash>) -> bool {
    let Ok(md) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if !md.is_file() || md.len() != size {
        return false;
    }
    match hash {
        Some(h) => full_hash(path, size).is_ok_and(|x| x.hash == h),
        None => true,
    }
}

/// Flushes a directory entry change to disk where the OS allows it.
pub fn sync_dir(dir: &Path) {
    if let Ok(f) = File::open(dir) {
        let _ = f.sync_all();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, data: &[u8]) -> (PathBuf, i64) {
        use std::os::unix::fs::MetadataExt;
        let p = dir.join(name);
        std::fs::write(&p, data).unwrap();
        let md = std::fs::metadata(&p).unwrap();
        (p, md.mtime() * 1_000_000_000 + md.mtime_nsec())
    }

    #[test]
    fn copy_makes_an_identical_file_and_leaves_no_temp_behind() {
        let t = tempfile::tempdir().unwrap();
        let (src, mt) = write(t.path(), "a.jpg", b"hello world");
        let dst = t.path().join("b.jpg");
        let h = copy_verified(&src, &dst, 1, 11, mt).unwrap();
        assert_eq!(std::fs::read(&dst).unwrap(), b"hello world");
        assert!(src.exists());
        assert_eq!(h, full_hash(&dst, 11).unwrap().hash);
        let names: Vec<_> = std::fs::read_dir(t.path()).unwrap().flatten().collect();
        assert_eq!(names.len(), 2, "no .tmp file left");
    }

    #[test]
    fn nothing_is_ever_replaced() {
        let t = tempfile::tempdir().unwrap();
        let (src, mt) = write(t.path(), "a.jpg", b"new");
        let (dst, _) = write(t.path(), "b.jpg", b"precious");
        assert!(copy_verified(&src, &dst, 1, 3, mt).is_err());
        assert!(move_file(&src, &dst, 2, 3, mt).is_err());
        assert_eq!(std::fs::read(&dst).unwrap(), b"precious");
        assert!(src.exists());
    }

    #[test]
    fn a_same_volume_move_renames() {
        let t = tempfile::tempdir().unwrap();
        let (src, mt) = write(t.path(), "a.jpg", b"abc");
        let dst = t.path().join("moved.jpg");
        assert_eq!(move_file(&src, &dst, 1, 3, mt).unwrap(), Moved::Renamed);
        assert!(!src.exists() && dst.exists());
        move_back(&dst, &src, 1, 3, mt).unwrap();
        assert!(src.exists() && !dst.exists());
    }

    #[test]
    fn the_copy_keeps_the_modification_time() {
        use std::os::unix::fs::MetadataExt;
        let t = tempfile::tempdir().unwrap();
        let (src, mt) = write(t.path(), "a.jpg", b"abc");
        let dst = t.path().join("b.jpg");
        copy_verified(&src, &dst, 1, 3, mt).unwrap();
        let md = std::fs::metadata(&dst).unwrap();
        assert_eq!(md.mtime() * 1_000_000_000 + md.mtime_nsec(), mt);
    }
}
