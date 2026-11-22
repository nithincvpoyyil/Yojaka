//! macOS filesystem helpers.

use std::os::macos::fs::MetadataExt;

const SF_DATALESS: u32 = 0x4000_0000;

/// True for iCloud placeholders. Opening one would start a download, so never do.
pub fn is_dataless(md: &std::fs::Metadata) -> bool {
    md.st_flags() & SF_DATALESS != 0
}

/// Opens `path` read-only, refusing a symlink (`O_NOFOLLOW`): a file swapped for a link after
/// the scan must not lead outside the folder.
pub fn open_nofollow(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::File::options()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

/// Bytes free for this user on the disk that holds `path`, or would hold it: a folder that does
/// not exist yet is measured at its nearest existing parent. `None` when that cannot be read.
pub fn free_space(path: &std::path::Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let existing = path.ancestors().find(|p| p.exists())?;
    let c = std::ffi::CString::new(existing.as_os_str().as_bytes()).ok()?;
    let mut st = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    // SAFETY: `c` is a valid NUL-terminated path and `st` points to writable memory of the right
    // size; statvfs only writes into it.
    #[allow(unsafe_code)]
    let rc = unsafe { libc::statvfs(c.as_ptr(), st.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }
    // SAFETY: statvfs returned 0, so it filled `st`.
    #[allow(unsafe_code)]
    let st = unsafe { st.assume_init() };
    Some(u64::from(st.f_bavail).saturating_mul(st.f_frsize))
}

#[cfg(test)]
mod tests {
    #[test]
    fn free_space_is_read_for_a_folder_that_does_not_exist_yet() {
        let tmp = tempfile::tempdir().unwrap();
        let free = super::free_space(&tmp.path().join("not/yet/made"));
        assert!(free.is_some_and(|b| b > 0), "{free:?}");
    }

    #[test]
    fn a_symlink_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("f");
        std::fs::write(&target, b"x").unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(super::open_nofollow(&target).is_ok());
        assert!(super::open_nofollow(&link).is_err());
    }

    #[test]
    fn regular_file_is_not_dataless() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("f");
        std::fs::write(&p, b"x").unwrap();
        assert!(!super::is_dataless(&std::fs::metadata(&p).unwrap()));
    }
}
