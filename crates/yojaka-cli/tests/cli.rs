#![allow(clippy::unwrap_used)]
use assert_cmd::Command;
use predicates::prelude::*;

fn yojaka(home: &std::path::Path) -> Command {
    let mut c = Command::cargo_bin("yojaka").unwrap();
    c.env("YOJAKA_HOME", home);
    c
}

#[test]
fn version_has_number_sha_and_target() {
    let tmp = tempfile::tempdir().unwrap();
    yojaka(tmp.path())
        .arg("--version")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("0.0.1").and(predicate::str::contains("aarch64-apple-darwin")),
        );
}

#[test]
fn help_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    let out = yojaka(tmp.path()).arg("--help").output().unwrap();
    insta::assert_snapshot!(String::from_utf8(out.stdout).unwrap());
}

#[test]
fn doctor_json_uses_home_override() {
    let tmp = tempfile::tempdir().unwrap();
    let out = yojaka(tmp.path())
        .args(["doctor", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let root = tmp.path().canonicalize().unwrap();
    for key in ["data", "index", "journal", "logs", "config_file"] {
        let p = std::path::PathBuf::from(v["paths"][key]["path"].as_str().unwrap());
        let p = p.canonicalize().unwrap_or(p);
        assert!(
            p.starts_with(&root) || p.starts_with(tmp.path()),
            "{key}: {p:?}"
        );
    }
}

#[test]
fn stub_command_exits_1_and_names_iteration() {
    let tmp = tempfile::tempdir().unwrap();
    yojaka(tmp.path())
        .args(["dupes", "."])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("Iteration 4"));
}

#[test]
fn bad_usage_exits_2() {
    let tmp = tempfile::tempdir().unwrap();
    yojaka(tmp.path()).arg("--nope").assert().code(2);
}
