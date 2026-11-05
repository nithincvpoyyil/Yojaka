//! User configuration (`config.toml`). Missing keys take defaults; unknown keys are ignored.

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Result, YojakaError};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    /// 0 = auto
    pub threads: usize,
    pub scan: ScanConfig,
    pub dupes: DupesConfig,
    pub organize: OrganizeConfig,
    pub location: LocationConfig,
    pub llm: LlmConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ScanConfig {
    pub sniff_all: bool,
    pub kinds: Vec<String>,
    pub skip_dirs: Vec<String>,
    pub skip_hidden: bool,
    pub exiftool_fallback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DupesConfig {
    pub keep_rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct OrganizeConfig {
    pub template: String,
    pub large_plan_threshold: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct LocationConfig {
    pub near_km: f64,
    pub far_km: f64,
    pub infer: bool,
    pub infer_window_minutes: u32,
    pub no_location_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct LlmConfig {
    pub provider: String,
    pub model: String,
    pub allow_cloud: bool,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            sniff_all: false,
            kinds: vec!["image".into(), "video".into()],
            skip_dirs: vec![
                ".git".into(),
                "node_modules".into(),
                ".Trash".into(),
                "Library".into(),
            ],
            skip_hidden: true,
            exiftool_fallback: false,
        }
    }
}

impl Default for DupesConfig {
    fn default() -> Self {
        Self {
            keep_rules: vec!["oldest".into(), "shortest-path".into()],
        }
    }
}

impl Default for OrganizeConfig {
    fn default() -> Self {
        Self {
            template: "{year}/{month:02}/{name}".into(),
            large_plan_threshold: 1000,
        }
    }
}

impl Default for LocationConfig {
    fn default() -> Self {
        Self {
            near_km: 25.0,
            far_km: 100.0,
            infer: false,
            infer_window_minutes: 120,
            no_location_dir: "No Location".into(),
        }
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: "none".into(),
            model: String::new(),
            allow_cloud: false,
        }
    }
}

impl Config {
    /// A missing file yields the defaults.
    pub fn load(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(YojakaError::io(path, e)),
        };
        toml::from_str(&text).map_err(|e| YojakaError::Config {
            path: path.to_path_buf(),
            message: e.to_string(),
        })
    }

    /// Atomic: write `<path>.tmp`, fsync, rename.
    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).map_err(|e| YojakaError::Config {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        let tmp = path.with_extension("toml.tmp");
        let write = || -> std::io::Result<()> {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(text.as_bytes())?;
            f.sync_all()?;
            std::fs::rename(&tmp, path)
        };
        write().map_err(|e| YojakaError::io(path, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_toml_snapshot() {
        insta::assert_snapshot!(toml::to_string_pretty(&Config::default()).unwrap());
    }

    #[test]
    fn missing_file_gives_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            Config::load(&tmp.path().join("nope.toml")).unwrap(),
            Config::default()
        );
    }

    #[test]
    fn partial_file_fills_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("c.toml");
        std::fs::write(&p, "threads = 3\n[location]\nnear_km = 5.0\n").unwrap();
        let c = Config::load(&p).unwrap();
        assert_eq!(c.threads, 3);
        assert_eq!(c.location.near_km, 5.0);
        assert_eq!(c.location.far_km, 100.0);
        assert_eq!(c.scan, ScanConfig::default());
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("c.toml");
        std::fs::write(&p, "future_key = 1\n[scan]\nnew_thing = true\n").unwrap();
        assert_eq!(Config::load(&p).unwrap(), Config::default());
    }

    #[test]
    fn invalid_toml_is_a_config_error() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("c.toml");
        std::fs::write(&p, "threads = [").unwrap();
        assert!(matches!(Config::load(&p), Err(YojakaError::Config { .. })));
    }

    #[test]
    fn save_then_load_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("c.toml");
        let c = Config {
            threads: 7,
            ..Config::default()
        };
        c.save(&p).unwrap();
        assert_eq!(Config::load(&p).unwrap(), c);
        assert!(!p.with_extension("toml.tmp").exists());
    }
}
