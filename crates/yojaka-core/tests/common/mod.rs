//! Shared integration-test harness.
#![allow(dead_code, clippy::unwrap_used)]

use std::path::PathBuf;

use tempfile::TempDir;
use yojaka_core::AppPaths;

pub struct TestEnv {
    pub tmp: TempDir,
    pub paths: AppPaths,
    /// Set by [`TestEnv::with_fixture`]: the scan root (`<tmp>/fixture/tree`).
    pub tree: Option<PathBuf>,
    pub manifest: Option<fixturegen::Manifest>,
}

impl TestEnv {
    pub fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(&tmp.path().join("app"));
        paths.ensure().unwrap();
        Self {
            tmp,
            paths,
            tree: None,
            manifest: None,
        }
    }

    /// Generates a fixture tree with the fixturegen library API (not the binary).
    pub fn with_fixture(seed: u64, files: usize) -> Self {
        let mut env = Self::new();
        let out = env.tmp.path().join("fixture");
        let spec = fixturegen::Spec {
            seed,
            files,
            noise: 20,
            ..Default::default()
        };
        env.manifest = Some(fixturegen::generate(&spec, &out).unwrap());
        env.tree = Some(out.join("tree"));
        env
    }
}
