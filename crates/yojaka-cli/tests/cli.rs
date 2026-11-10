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

#[test]
fn about_credits_geonames_with_the_licence_and_creates_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("fresh-home");
    yojaka(&home).arg("--about").assert().success().stdout(
        predicate::str::contains("GeoNames")
            .and(predicate::str::contains("https://www.geonames.org"))
            .and(predicate::str::contains("Creative Commons Attribution 4.0"))
            .and(predicate::str::contains(
                "creativecommons.org/licenses/by/4.0",
            ))
            .and(predicate::str::contains("MIT License"))
            .and(predicate::str::contains("converted")),
    );
    assert!(!home.exists(), "--about must not create the data folder");
}

#[test]
fn dupes_previews_and_saves_a_plan_without_touching_files() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = fixture(tmp.path());
    yojaka(tmp.path()).arg("scan").arg(&tree).assert().success();
    let before = tree_listing(&tree);

    let out = yojaka(tmp.path())
        .args(["dupes", "--json"])
        .arg(&tree)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let groups = v["groups"].as_array().unwrap();
    assert!(!groups.is_empty(), "the fixture has duplicates");
    for g in groups {
        let marks: Vec<bool> = g["delete"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d.as_bool().unwrap())
            .collect();
        assert!(marks.iter().any(|d| !d), "one copy is always kept");
    }
    assert!(v["reclaim"].as_u64().unwrap() > 0);

    let plan = tmp.path().join("plan.json");
    yojaka(tmp.path())
        .args(["dupes", "--keep", "shortest-path", "--plan-out"])
        .arg(&plan)
        .arg(&tree)
        .assert()
        .success()
        .stdout(predicate::str::contains("Nothing has been changed"));
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&plan).unwrap()).unwrap();
    assert_eq!(saved["kind"], "dedupe");
    assert!(!saved["ops"].as_array().unwrap().is_empty());

    // Never overwrites.
    yojaka(tmp.path())
        .args(["dupes", "--plan-out"])
        .arg(&plan)
        .arg(&tree)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("never overwrites"));
    assert_eq!(
        tree_listing(&tree),
        before,
        "dupes must not change the tree"
    );
}

#[test]
fn dupes_rejects_a_bad_keep_rule_and_unbuilt_tiers() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = fixture(tmp.path());
    yojaka(tmp.path()).arg("scan").arg(&tree).assert().success();
    yojaka(tmp.path())
        .args(["dupes", "--keep", "bogus"])
        .arg(&tree)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unknown keep rule"));
    yojaka(tmp.path())
        .args(["dupes", "--tier", "likely"])
        .arg(&tree)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("not available yet"));
}

fn tree_listing(root: &std::path::Path) -> Vec<(String, u64)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<(String, u64)>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let md = std::fs::symlink_metadata(e.path()).unwrap();
            if md.is_dir() {
                walk(&e.path(), out);
            } else {
                out.push((e.path().display().to_string(), md.len()));
            }
        }
    }
    let mut v = Vec::new();
    walk(root, &mut v);
    v.sort();
    v
}

fn debug_logs(home: &std::path::Path) -> Vec<std::path::PathBuf> {
    let Ok(rd) = std::fs::read_dir(home.join("logs")) else {
        return vec![];
    };
    rd.flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "log"))
        .collect()
}

#[test]
fn no_debug_flag_means_no_log_file() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = fixture(tmp.path());
    yojaka(tmp.path()).arg("scan").arg(&tree).assert().success();
    assert!(debug_logs(tmp.path()).is_empty());
}

#[test]
fn debug_flag_writes_a_log_with_threads_and_steps() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = fixture(tmp.path());
    yojaka(tmp.path())
        .args(["--debug", "scan"])
        .arg(&tree)
        .assert()
        .success()
        .stderr(predicate::str::contains("debug log:"));
    let logs = debug_logs(tmp.path());
    assert_eq!(logs.len(), 1);
    let text = std::fs::read_to_string(&logs[0]).unwrap();
    for needle in [
        "yojaka starting (debug mode)",
        "scan: starting",
        "scan-worker-0",
        "scan-writer",
        "scan: finished",
        "metadata: finished",
    ] {
        assert!(text.contains(needle), "log lacks {needle:?}");
    }
}

#[test]
fn apply_is_a_dry_run_without_write_then_trashes_and_undo_restores() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = fixture(tmp.path());
    let trash = tmp.path().join("trash");
    yojaka(tmp.path()).arg("scan").arg(&tree).assert().success();
    let plan = tmp.path().join("plan.json");
    yojaka(tmp.path())
        .args(["dupes", "--keep", "shortest-path", "--plan-out"])
        .arg(&plan)
        .arg(&tree)
        .assert()
        .success();
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&plan).unwrap()).unwrap();
    let n = saved["ops"].as_array().unwrap().len();
    assert!(n > 0);
    let before = tree_listing(&tree);

    // Without --write: a summary, and nothing changes.
    yojaka(tmp.path())
        .arg("apply")
        .arg(&plan)
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry run: nothing was changed"));
    assert_eq!(tree_listing(&tree), before);

    // A wrong phrase changes nothing.
    let phrase = if n == 1 {
        "trash 1 file".to_string()
    } else {
        format!("trash {n} files")
    };
    let apply = |input: &str| {
        let mut c = yojaka(tmp.path());
        c.args(["--write", "--trash-dir"])
            .arg(&trash)
            .arg("apply")
            .arg(&plan)
            .write_stdin(format!("{input}\n"));
        c
    };
    if n >= 50 {
        apply("nope").assert().code(1);
        assert_eq!(tree_listing(&tree), before);
    }
    let answer = if n >= 50 { phrase } else { "y".to_string() };
    apply(&answer)
        .assert()
        .success()
        .stdout(predicate::str::contains("moved to the Trash"));
    assert_eq!(tree_listing(&tree).len(), before.len() - n);
    assert_eq!(std::fs::read_dir(&trash).unwrap().count(), n);

    // Applying the same plan again is refused.
    apply(&answer).assert().code(1);

    yojaka(tmp.path())
        .arg("history")
        .assert()
        .success()
        .stdout(predicate::str::contains("applied"));

    // Undo previews without --write, and restores with it.
    yojaka(tmp.path())
        .args(["undo", "--last"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Preview only"));
    assert_eq!(tree_listing(&tree).len(), before.len() - n);
    yojaka(tmp.path())
        .args(["--write", "undo", "--last"])
        .assert()
        .success();
    assert_eq!(tree_listing(&tree), before);
    yojaka(tmp.path())
        .arg("history")
        .assert()
        .stdout(predicate::str::contains("undone"));
}

#[test]
fn a_permanent_delete_needs_its_phrase_even_with_yes() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = fixture(tmp.path());
    yojaka(tmp.path()).arg("scan").arg(&tree).assert().success();
    let plan = tmp.path().join("plan.json");
    yojaka(tmp.path())
        .args(["dupes", "--plan-out"])
        .arg(&plan)
        .arg(&tree)
        .assert()
        .success();
    let before = tree_listing(&tree);
    yojaka(tmp.path())
        .args(["--write", "apply", "--yes", "--permanent"])
        .arg(&plan)
        .write_stdin("yes\n")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("Nothing was changed"));
    assert_eq!(tree_listing(&tree), before);
    // And without --permanent a permanent plan is not possible at all.
    yojaka(tmp.path())
        .args(["plan", "show"])
        .arg(&plan)
        .assert()
        .success()
        .stdout(predicate::str::contains("move to the Trash"));
}

#[test]
fn organize_plans_then_apply_copies_and_undo_removes_the_copies() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = fixture(tmp.path());
    yojaka(tmp.path()).arg("scan").arg(&tree).assert().success();
    let before = tree_listing(&tree);
    let dest = tmp.path().join("sorted");
    let plan = tmp.path().join("organize.json");

    // A bad template is a usage error and plans nothing.
    yojaka(tmp.path())
        .args(["organize", "--template", "{year}/{nope}/{name}"])
        .arg(&tree)
        .arg(&dest)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unknown placeholder"));

    yojaka(tmp.path())
        .args(["organize", "--preset", "date", "--plan-out"])
        .arg(&plan)
        .arg(&tree)
        .arg(&dest)
        .assert()
        .success()
        .stdout(predicate::str::contains("Nothing has been changed"))
        .stdout(predicate::str::contains("copy (originals stay)"));
    assert!(!dest.exists());
    assert_eq!(tree_listing(&tree), before);

    // Never overwrites the plan file.
    yojaka(tmp.path())
        .args(["organize", "--plan-out"])
        .arg(&plan)
        .arg(&tree)
        .arg(&dest)
        .assert()
        .code(2);

    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&plan).unwrap()).unwrap();
    assert_eq!(saved["kind"], "organize");
    let copies = saved["ops"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|o| o["op"] == "copy")
        .count();
    assert!(copies > 0);

    // Dry run first.
    yojaka(tmp.path())
        .arg("apply")
        .arg(&plan)
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry run"));
    assert!(!dest.exists());

    let phrase = if copies >= 1000 {
        format!("copy {copies} files")
    } else {
        "y".into()
    };
    yojaka(tmp.path())
        .args(["--write", "apply"])
        .arg(&plan)
        .write_stdin(format!("{phrase}\n"))
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("{copies} copied")));
    assert_eq!(tree_listing(&tree), before, "originals untouched");
    assert_eq!(tree_listing(&dest).len(), copies);

    yojaka(tmp.path())
        .args(["--write", "undo", "--last"])
        .assert()
        .success();
    assert!(!dest.exists() || tree_listing(&dest).is_empty());
    assert_eq!(tree_listing(&tree), before);
}

#[test]
fn organize_into_a_protected_folder_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = fixture(tmp.path());
    yojaka(tmp.path()).arg("scan").arg(&tree).assert().success();
    yojaka(tmp.path())
        .arg("organize")
        .arg(&tree)
        .arg("/System/yojaka")
        .assert()
        .failure()
        .stderr(predicate::str::contains("protected"));
}
