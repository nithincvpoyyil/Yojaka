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

#[cfg(test)]
mod tests {
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
