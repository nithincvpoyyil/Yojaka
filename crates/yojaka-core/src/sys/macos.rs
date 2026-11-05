//! macOS filesystem helpers.

use std::os::macos::fs::MetadataExt;

const SF_DATALESS: u32 = 0x4000_0000;

/// True for iCloud placeholders. Opening one would start a download, so never do.
pub fn is_dataless(md: &std::fs::Metadata) -> bool {
    md.st_flags() & SF_DATALESS != 0
}

#[cfg(test)]
mod tests {
    #[test]
    fn regular_file_is_not_dataless() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("f");
        std::fs::write(&p, b"x").unwrap();
        assert!(!super::is_dataless(&std::fs::metadata(&p).unwrap()));
    }
}
