#![allow(clippy::unwrap_used)]
mod common;

use common::TestEnv;
use yojaka_core::Engine;

#[test]
fn fixture_env_is_isolated_and_populated() {
    let env = TestEnv::with_fixture(1, 50);
    assert!(env.paths.data.starts_with(env.tmp.path()));
    let manifest = env.manifest.as_ref().unwrap();
    assert!(manifest.expected.indexed_count >= 50);
    assert!(env.tree.as_ref().unwrap().is_dir());
    let engine = Engine::new(env.paths.clone(), Default::default());
    assert_eq!(engine.config.threads, 0);
}
