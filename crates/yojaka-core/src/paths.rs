//! All on-disk locations in one place. Injected, never global.

use std::path::{Path, PathBuf};

use crate::{Result, YojakaError};

#[derive(Debug, Clone)]
pub struct AppPaths {
    /// `~/Library/Application Support/yojaka`
    pub data: PathBuf,
    /// `~/Library/Logs/yojaka`
    pub logs: PathBuf,
}

impl AppPaths {
    /// Real locations. `home_override` (from `YOJAKA_HOME`, read once in `main`) puts
    /// everything under that folder instead.
    pub fn from_env_or(home_override: Option<&Path>) -> Result<Self> {
        if let Some(root) = home_override {
            return Ok(Self::under(root));
        }
        let base = directories::BaseDirs::new().ok_or_else(|| YojakaError::Config {
            path: PathBuf::new(),
            message: "cannot determine the home directory".into(),
        })?;
        Ok(Self {
            data: base.data_dir().join("yojaka"),
            logs: base.home_dir().join("Library").join("Logs").join("yojaka"),
        })
    }

    pub fn from_env() -> Result<Self> {
        Self::from_env_or(None)
    }

    /// Tests: everything under a temp dir.
    pub fn under(root: &Path) -> Self {
        Self {
            data: root.join("data"),
            logs: root.join("logs"),
        }
    }

    pub fn config_file(&self) -> PathBuf {
        self.data.join("config.toml")
    }
    pub fn index_dir(&self) -> PathBuf {
        self.data.join("indexes")
    }
    /// `indexes/<hash of canonical scan root>.db`. I1 swaps the placeholder hash for blake3.
    pub fn index_for(&self, scan_root: &Path) -> PathBuf {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let canon = scan_root
            .canonicalize()
            .unwrap_or_else(|_| scan_root.to_path_buf());
        let mut h = DefaultHasher::new();
        canon.hash(&mut h);
        self.index_dir().join(format!("{:016x}.db", h.finish()))
    }
    pub fn journal_dir(&self) -> PathBuf {
        self.data.join("journal")
    }
    /// Plans saved from the app (`P`). Inside Yojaka's own data folder, so read-only mode may write here.
    pub fn plans_dir(&self) -> PathBuf {
        self.data.join("plans")
    }
    /// Exports made from the app (`E`). Same reason as [`Self::plans_dir`].
    pub fn exports_dir(&self) -> PathBuf {
        self.data.join("exports")
    }
    pub fn log_file(&self) -> PathBuf {
        self.logs.join("yojaka.log")
    }

    /// `mkdir -p` for every directory.
    pub fn ensure(&self) -> Result<()> {
        for dir in [
            &self.data,
            &self.logs,
            &self.index_dir(),
            &self.journal_dir(),
        ] {
            std::fs::create_dir_all(dir).map_err(|e| YojakaError::io(dir, e))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_keeps_everything_in_root() {
        let tmp = tempfile::tempdir().unwrap();
        let p = AppPaths::under(tmp.path());
        p.ensure().unwrap();
        for d in [p.index_dir(), p.journal_dir(), p.logs.clone()] {
            assert!(d.starts_with(tmp.path()) && d.is_dir());
        }
        assert!(p.config_file().starts_with(tmp.path()));
    }

    #[test]
    fn index_for_is_stable_and_distinct() {
        let tmp = tempfile::tempdir().unwrap();
        let p = AppPaths::under(tmp.path());
        let a = p.index_for(Path::new("/a"));
        assert_eq!(a, p.index_for(Path::new("/a")));
        assert_ne!(a, p.index_for(Path::new("/b")));
    }

    #[test]
    fn real_paths_use_library_folders() {
        let p = AppPaths::from_env().unwrap();
        assert!(p.data.ends_with("Library/Application Support/yojaka"));
        assert!(p.logs.ends_with("Library/Logs/yojaka"));
    }
}
