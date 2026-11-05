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

fn fixture(home: &std::path::Path) -> std::path::PathBuf {
    let out = home.join("fx");
    let spec = fixturegen::Spec {
        files: 60,
        noise: 10,
        ..Default::default()
    };
    fixturegen::generate(&spec, &out).unwrap();
    out.join("tree")
}

#[test]
fn scan_then_stats_json() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = fixture(tmp.path());
    yojaka(tmp.path())
        .args(["scan"])
        .arg(&tree)
        .assert()
        .success()
        .stdout(predicate::str::contains("media files indexed"));

    let out = yojaka(tmp.path())
        .args(["stats", "--json"])
        .arg(&tree)
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["photos"]["count"].as_u64().unwrap() >= 60);
    assert_eq!(v["videos"]["count"], 0);
    assert_eq!(v["last_scan"]["status"], "complete");

    // Second scan is incremental.
    yojaka(tmp.path())
        .arg("scan")
        .arg(&tree)
        .assert()
        .success()
        .stdout(predicate::str::contains("0 new or changed"));
}

#[test]
fn stats_without_scan_hints_to_scan() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = fixture(tmp.path());
    yojaka(tmp.path())
        .arg("stats")
        .arg(&tree)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("yojaka scan"));
}

#[test]
fn scanning_a_protected_folder_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    yojaka(tmp.path())
        .args(["scan", "/System"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("protected"));
}

#[test]
fn scan_force_rebuilds_the_index() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = fixture(tmp.path());
    yojaka(tmp.path()).arg("scan").arg(&tree).assert().success();
    yojaka(tmp.path())
        .args(["scan", "--force"])
        .arg(&tree)
        .assert()
        .success()
        .stdout(
            predicate::str::contains("flushed and rebuilt")
                .and(predicate::str::contains("0 unchanged")),
        );
}
