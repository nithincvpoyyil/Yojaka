//! Protected macOS locations (FR-14a). Hard safety rule, no override.
//!
//! Callers must pass **canonical** paths (symlinks, `..` and firmlinks resolved).

use std::path::{Component, Path, PathBuf};

use crate::{Result, YojakaError};

const SYSTEM_PREFIXES: &[&str] = &[
    "/System",
    "/Library",
    "/Applications",
    "/usr",
    "/bin",
    "/sbin",
    "/etc",
    "/var",
    "/private",
    "/opt",
    "/cores",
    "/dev",
];

const PROTECTED_NAMES: &[&str] = &[
    ".Spotlight-V100",
    ".fseventsd",
    ".Trashes",
    ".DocumentRevisions-V100",
    ".TemporaryItems",
];

/// Per-user temp/cache folders live under `/private/var`, but are not system folders.
/// Exempting them keeps tests (which run in temp dirs) possible.
const EXEMPT: &[&str] = &["/private/var/folders"];

#[derive(Debug, Clone)]
pub struct Protected {
    prefixes: Vec<PathBuf>,
}

impl Protected {
    pub fn new(home: Option<&Path>) -> Self {
        let mut raw: Vec<PathBuf> = SYSTEM_PREFIXES.iter().map(PathBuf::from).collect();
        if let Some(h) = home {
            for sub in ["Library", "Applications", ".Trash"] {
                raw.push(h.join(sub));
            }
        }
        let mut prefixes = Vec::new();
        for p in raw {
            if let Ok(c) = p.canonicalize()
                && c != p
            {
                prefixes.push(c);
            }
            prefixes.push(p);
        }
        Self { prefixes }
    }

    pub fn system() -> Self {
        let base = directories::BaseDirs::new();
        Self::new(base.as_ref().map(|b| b.home_dir()))
    }

    /// Why `canonical` is protected, or `None` if it is fine to operate on.
    pub fn reason(&self, canonical: &Path) -> Option<String> {
        if canonical == Path::new("/") {
            return Some("the volume root".into());
        }
        let exempt = EXEMPT.iter().any(|e| canonical.starts_with(e));
        if !exempt && let Some(p) = self.prefixes.iter().find(|p| canonical.starts_with(p)) {
            return Some(format!("inside {}", p.display()));
        }
        for c in canonical.components() {
            let Component::Normal(name) = c else { continue };
            let name = name.to_string_lossy();
            let lower = name.to_ascii_lowercase();
            if PROTECTED_NAMES.iter().any(|n| name == *n) {
                return Some(format!("inside {name}"));
            }
            if lower.ends_with(".app") || lower.ends_with(".photoslibrary") {
                return Some(format!("inside the bundle {name}"));
            }
        }
        None
    }

    /// Resolves `root` and refuses it when it is protected. Returns the canonical path.
    pub fn check_root(&self, root: &Path) -> Result<PathBuf> {
        let canon = root.canonicalize().map_err(|e| YojakaError::io(root, e))?;
        match self.reason(&canon) {
            Some(reason) => Err(YojakaError::Protected {
                path: canon,
                reason,
            }),
            None => Ok(canon),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p() -> Protected {
        Protected::system()
    }

    #[test]
    fn system_folders_are_refused() {
        for path in [
            "/",
            "/System",
            "/Applications",
            "/usr",
            "/etc",
            "/private/tmp",
        ] {
            assert!(
                p().check_root(Path::new(path)).is_err(),
                "{path} should be refused"
            );
        }
    }

    #[test]
    fn symlinks_into_protected_paths_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let link = tmp.path().join("sneaky");
        std::os::unix::fs::symlink("/System", &link).unwrap();
        assert!(matches!(
            p().check_root(&link),
            Err(YojakaError::Protected { .. })
        ));
    }

    #[test]
    fn user_library_is_refused_but_home_is_allowed() {
        let home = directories::BaseDirs::new()
            .unwrap()
            .home_dir()
            .to_path_buf();
        let lib = home.join("Library");
        if lib.exists() {
            assert!(p().check_root(&lib).is_err());
        }
        assert!(p().reason(&home).is_none());
        assert!(p().reason(Path::new("/Volumes/Backup")).is_none());
    }

    #[test]
    fn bundles_and_special_folders_are_protected_by_name() {
        let pr = p();
        assert!(
            pr.reason(Path::new(
                "/Users/x/Pictures/Photos.photoslibrary/originals"
            ))
            .is_some()
        );
        assert!(pr.reason(Path::new("/Applications/Foo.app")).is_some());
        assert!(pr.reason(Path::new("/Volumes/V/.Spotlight-V100")).is_some());
        assert!(pr.reason(Path::new("/Volumes/V/Photos")).is_none());
    }

    #[test]
    fn temp_dirs_are_allowed() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(p().check_root(tmp.path()).is_ok());
    }
}
