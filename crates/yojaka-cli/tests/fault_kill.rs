//! Fault injection with the real program: `kill -9` in the middle of `apply` and of `undo`.
//!
//! Each run is slowed down with `YOJAKA_TEST_OP_DELAY_MS` (a debug-build switch in the journal) so
//! the kill lands between journal lines. Afterwards `apply --resume` must finish the plan, and
//! `undo` must give back a tree whose content is exactly the original.
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_yojaka");
const DELAY_MS: &str = "30";

fn yojaka(home: &Path) -> assert_cmd::Command {
    let mut c = assert_cmd::Command::new(BIN);
    c.env("YOJAKA_HOME", home);
    c
}

fn content_hash(bytes: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// Every regular file under `root` (relative path) with a hash of its content. Temp files that a
/// copy in progress leaves (`.name.yojaka-N.tmp`) are not counted: they are never at a final name.
fn files(root: &Path) -> BTreeMap<PathBuf, u64> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let md = std::fs::symlink_metadata(&p).unwrap();
            if md.is_dir() {
                stack.push(p);
            } else if md.is_file() {
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                if name.starts_with('.') && name.ends_with(".tmp") && name.contains(".yojaka-") {
                    continue;
                }
                out.insert(
                    p.strip_prefix(root).unwrap().to_path_buf(),
                    content_hash(&std::fs::read(&p).unwrap()),
                );
            }
        }
    }
    out
}

fn stray_temp_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p
                .file_name()
                .is_some_and(|n| n.to_string_lossy().contains(".yojaka-"))
            {
                out.push(p);
            }
        }
    }
    out
}

struct Lab {
    home: tempfile::TempDir,
    tree: PathBuf,
    dest: PathBuf,
    trash: PathBuf,
    plan: PathBuf,
}

impl Lab {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let out = home.path().join("fx");
        let spec = fixturegen::Spec {
            files: 120,
            dupes: 0.4,
            noise: 0,
            live_photos: 0,
            ..Default::default()
        };
        fixturegen::generate(&spec, &out).unwrap();
        let tree = out.join("tree").canonicalize().unwrap();
        let lab = Self {
            dest: home.path().join("sorted"),
            trash: home.path().join("trash"),
            plan: home.path().join("plan.json"),
            tree,
            home,
        };
        yojaka(lab.home.path())
            .arg("scan")
            .arg(&lab.tree)
            .assert()
            .success();
        lab
    }

    fn make_plan(&self, kind: Kind) {
        let mut c = yojaka(self.home.path());
        match kind {
            Kind::Trash => {
                c.args(["dupes", "--keep", "shortest-path", "--plan-out"])
                    .arg(&self.plan)
                    .arg(&self.tree);
            }
            Kind::Copy | Kind::Move => {
                c.args(["organize", "--template", "{year}/{name}", "--plan-out"])
                    .arg(&self.plan);
                if kind == Kind::Move {
                    c.arg("--move");
                }
                c.arg(&self.tree).arg(&self.dest);
            }
        }
        c.assert().success();
    }

    /// File operations in the saved plan (what the typed phrase counts).
    fn file_ops(&self) -> usize {
        let v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&self.plan).unwrap()).unwrap();
        v["ops"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|o| matches!(o["op"].as_str(), Some("trash" | "move" | "copy")))
            .count()
    }

    fn plan_id(&self) -> String {
        let v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&self.plan).unwrap()).unwrap();
        v["id"].as_str().unwrap().to_string()
    }

    fn journal(&self) -> PathBuf {
        self.home
            .path()
            .join("data")
            .join("journal")
            .join(format!("{}.jsonl", self.plan_id()))
    }

    /// Ids of the plan's file operations (folder creations are left out).
    fn file_op_ids(&self) -> BTreeSet<u64> {
        let v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&self.plan).unwrap()).unwrap();
        v["ops"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|o| matches!(o["op"].as_str(), Some("trash" | "move" | "copy")))
            .map(|o| o["id"].as_u64().unwrap())
            .collect()
    }

    /// Journal lines that record a finished *file* operation of `run` (`apply` or `undo`).
    fn done_lines(&self, run: &str) -> usize {
        let ids = self.file_op_ids();
        std::fs::read_to_string(self.journal()).map_or(0, |t| {
            t.lines()
                .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
                .filter(|l| l["t"] == "entry" && l["run"] == run && l["state"] == "done")
                .filter(|l| l["op"].as_u64().is_some_and(|id| ids.contains(&id)))
                .count()
        })
    }

    /// True when the journal's last line is the "begin" of a file operation: a kill now leaves a
    /// step half done.
    fn inside_a_step(&self, run: &str) -> bool {
        let ids = self.file_op_ids();
        std::fs::read_to_string(self.journal()).is_ok_and(|t| {
            t.lines()
                .rev()
                .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
                .find(|l| l["t"] == "entry")
                .is_some_and(|l| {
                    l["run"] == run
                        && l["state"] == "begin"
                        && l["op"].as_u64().is_some_and(|id| ids.contains(&id))
                })
        })
    }

    /// Like [`Self::kill_after`], but waits for a moment inside a step, so the journal is left
    /// with a step that was begun and never finished.
    fn kill_inside_a_step(&self, mut child: Child, run: &str, at_least: usize) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            assert!(
                Instant::now() < deadline,
                "never caught the run inside a step"
            );
            if let Some(status) = child.try_wait().unwrap() {
                panic!("the run ended ({status}) before it could be killed");
            }
            if self.done_lines(run) >= at_least && self.inside_a_step(run) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(
            self.inside_a_step(run),
            "the kill must leave a half-done step"
        );
    }

    fn phrase(&self, kind: Kind) -> String {
        let word = match kind {
            Kind::Trash => "trash",
            Kind::Copy => "copy",
            Kind::Move => "move",
        };
        format!("{word} {} files", self.file_ops())
    }

    /// Starts `yojaka --write ...` slowed down, with the confirmation on its stdin.
    fn spawn_slow(&self, kind: Kind, args: &[&OsStr]) -> Child {
        let log = std::fs::File::create(self.home.path().join("child.log")).unwrap();
        let mut child = Command::new(BIN)
            .env("YOJAKA_HOME", self.home.path())
            .env("YOJAKA_TEST_OP_DELAY_MS", DELAY_MS)
            .arg("--write")
            .arg("--trash-dir")
            .arg(&self.trash)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let _ = writeln!(stdin, "{}", self.phrase(kind));
        drop(stdin);
        child
    }

    /// Waits until `run` has finished `at_least` operations, then `kill -9`s the child. Fails
    /// (instead of passing vacuously) when the run ended before the kill.
    fn kill_after(&self, mut child: Child, run: &str, at_least: usize, total: usize) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            assert!(Instant::now() < deadline, "the run never got going");
            if let Some(status) = child.try_wait().unwrap() {
                panic!("the run ended ({status}) before it could be killed");
            }
            if self.done_lines(run) >= at_least {
                break;
            }
            std::thread::sleep(Duration::from_millis(3));
        }
        child.kill().unwrap(); // SIGKILL: no clean-up code runs
        child.wait().unwrap();
        let done = self.done_lines(run);
        assert!(
            done < total,
            "the kill came too late: {done} of {total} operations were already done"
        );
    }

    /// What the slowed-down child printed (for failure messages).
    fn child_log(&self) -> String {
        std::fs::read_to_string(self.home.path().join("child.log")).unwrap_or_default()
    }

    fn apply_args(&self, resume: bool) -> Vec<&OsStr> {
        let mut v: Vec<&OsStr> = vec![OsStr::new("apply"), self.plan.as_os_str()];
        v.push(OsStr::new("--yes"));
        if resume {
            v.push(OsStr::new("--resume"));
        }
        v
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Trash,
    Copy,
    Move,
}

/// Kill `apply` part-way, resume it, then undo it; the source tree must be what it was.
fn kill_apply_then_resume_then_undo(kind: Kind) {
    let lab = Lab::new();
    lab.make_plan(kind);
    let n = lab.file_ops();
    assert!(
        n >= 20,
        "the fixture should give a plan of some size, got {n}"
    );
    let before = files(&lab.tree);
    let source_hashes: BTreeSet<u64> = before.values().copied().collect();

    let args = lab.apply_args(false);
    let child = lab.spawn_slow(kind, &args);
    lab.kill_after(child, "apply", 3, n);

    // After the kill: nothing was lost, and nothing sits at a final name half written.
    let mid_source = files(&lab.tree);
    let mid_dest = files(&lab.dest);
    match kind {
        Kind::Copy => assert_eq!(mid_source, before, "a copy never touches the originals"),
        Kind::Trash | Kind::Move => assert!(mid_source.len() < before.len()),
    }
    for (path, hash) in &mid_dest {
        assert!(
            source_hashes.contains(hash),
            "{} is at a final name but is not a complete copy of any original",
            path.display()
        );
    }

    // A second plain run is refused; --resume finishes the job.
    yojaka(lab.home.path())
        .arg("--write")
        .arg("--trash-dir")
        .arg(&lab.trash)
        .args(["apply"])
        .arg(&lab.plan)
        .arg("--yes")
        .write_stdin(format!("{}\n", lab.phrase(kind)))
        .assert()
        .code(1);
    let resume = lab.apply_args(true);
    yojaka(lab.home.path())
        .arg("--write")
        .arg("--trash-dir")
        .arg(&lab.trash)
        .args(&resume)
        .write_stdin(format!("{}\n", lab.phrase(kind)))
        .assert()
        .success();
    match kind {
        Kind::Trash => assert_eq!(files(&lab.tree).len(), before.len() - n),
        Kind::Copy => {
            assert_eq!(files(&lab.tree), before);
            assert_eq!(files(&lab.dest).len(), n);
        }
        Kind::Move => {
            assert_eq!(files(&lab.tree).len(), before.len() - n);
            assert_eq!(files(&lab.dest).len(), n);
        }
    }
    assert!(
        stray_temp_files(&lab.dest).is_empty(),
        "no temporary file may be left after the resume"
    );
    yojaka(lab.home.path())
        .arg("history")
        .assert()
        .stdout(predicates::str::contains("applied"));

    yojaka(lab.home.path())
        .arg("--write")
        .arg("--trash-dir")
        .arg(&lab.trash)
        .args(["undo", "--last"])
        .assert()
        .success();
    assert_eq!(
        files(&lab.tree),
        before,
        "undo gives back the original tree"
    );
    assert!(files(&lab.dest).is_empty(), "undo leaves no copies behind");
    assert!(stray_temp_files(&lab.dest).is_empty());
}

#[test]
fn a_killed_trash_run_resumes_and_undo_restores_the_tree() {
    kill_apply_then_resume_then_undo(Kind::Trash);
}

#[test]
fn a_killed_copy_run_resumes_and_undo_removes_exactly_the_copies() {
    kill_apply_then_resume_then_undo(Kind::Copy);
}

#[test]
fn a_killed_move_run_resumes_and_undo_moves_everything_back() {
    kill_apply_then_resume_then_undo(Kind::Move);
}

/// After a killed `apply`, `undo` refuses until the run is finished, says how, and changes
/// nothing; the command it prints works.
#[test]
fn undo_refuses_an_interrupted_run_and_the_printed_resume_command_works() {
    let lab = Lab::new();
    lab.make_plan(Kind::Move);
    let n = lab.file_ops();
    let before = files(&lab.tree);
    let args = lab.apply_args(false);
    let child = lab.spawn_slow(Kind::Move, &args);
    lab.kill_inside_a_step(child, "apply", 3);
    assert!(lab.done_lines("apply") < n);
    let mid_tree = files(&lab.tree);
    let mid_dest = files(&lab.dest);

    let out = yojaka(lab.home.path())
        .args(["--write", "undo", "--last"])
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stderr).to_string();
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(
        said.contains("interrupted") && said.contains("--resume"),
        "{said}"
    );
    assert_eq!(files(&lab.tree), mid_tree, "the refusal changed nothing");
    assert_eq!(files(&lab.dest), mid_dest);

    // The printed command: `yojaka --write apply <plan copy> --resume`.
    let plan_copy = said
        .split("`yojaka --write apply ")
        .nth(1)
        .and_then(|rest| rest.split(" --resume`").next())
        .map(PathBuf::from)
        .unwrap();
    assert!(plan_copy.is_file(), "{}", plan_copy.display());
    yojaka(lab.home.path())
        .arg("--write")
        .arg("apply")
        .arg(&plan_copy)
        .args(["--resume", "--yes"])
        .write_stdin(format!("{}\n", lab.phrase(Kind::Move)))
        .assert()
        .success();
    yojaka(lab.home.path())
        .args(["--write", "undo", "--last"])
        .assert()
        .success();
    assert_eq!(files(&lab.tree), before);
    assert!(files(&lab.dest).is_empty());
}

/// Kill `undo` part-way; running `undo` again must finish the job.
fn kill_undo_then_undo_again(kind: Kind) {
    let lab = Lab::new();
    lab.make_plan(kind);
    let n = lab.file_ops();
    let before = files(&lab.tree);

    let apply = lab.apply_args(false);
    yojaka(lab.home.path())
        .arg("--write")
        .arg("--trash-dir")
        .arg(&lab.trash)
        .args(&apply)
        .write_stdin(format!("{}\n", lab.phrase(kind)))
        .assert()
        .success();
    let applied = files(&lab.tree);
    assert_ne!(applied, before);

    let undo_args = [OsStr::new("undo"), OsStr::new("--last")];
    let child = lab.spawn_slow(kind, &undo_args);
    lab.kill_after(child, "undo", 3, n);
    // Part-way: some files are back, some are not, none is lost.
    let mid = files(&lab.tree);
    assert_ne!(mid, applied);
    assert_ne!(mid, before);

    yojaka(lab.home.path())
        .arg("--write")
        .arg("--trash-dir")
        .arg(&lab.trash)
        .args(["undo", "--last"])
        .assert()
        .success();
    assert_eq!(files(&lab.tree), before, "the second undo finishes the job");
    assert!(files(&lab.dest).is_empty());
    assert!(stray_temp_files(&lab.dest).is_empty());
}

#[test]
fn a_killed_undo_of_a_trash_run_is_finished_by_the_next_undo() {
    kill_undo_then_undo_again(Kind::Trash);
}

#[test]
fn a_killed_undo_of_a_move_run_is_finished_by_the_next_undo() {
    kill_undo_then_undo_again(Kind::Move);
}

/// Organizing into a read-only folder: the command fails, says so, and changes nothing.
#[test]
fn a_read_only_destination_is_reported_and_changes_nothing() {
    use std::os::unix::fs::PermissionsExt;
    let lab = Lab::new();
    lab.make_plan(Kind::Copy);
    let before = files(&lab.tree);
    std::fs::create_dir_all(&lab.dest).unwrap();
    std::fs::set_permissions(&lab.dest, std::fs::Permissions::from_mode(0o555)).unwrap();

    let out = yojaka(lab.home.path())
        .arg("--write")
        .arg("--trash-dir")
        .arg(&lab.trash)
        .args(["apply"])
        .arg(&lab.plan)
        .arg("--yes")
        .write_stdin(format!("{}\n", lab.phrase(Kind::Copy)))
        .output()
        .unwrap();

    std::fs::set_permissions(&lab.dest, std::fs::Permissions::from_mode(0o755)).unwrap();
    let code = out.status.code().unwrap();
    assert_eq!(
        code, 3,
        "partial failure: every file failed and was reported"
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("Permission denied"));
    assert_eq!(files(&lab.tree), before, "the originals are untouched");
    assert!(files(&lab.dest).is_empty(), "nothing was written");
    assert!(stray_temp_files(&lab.dest).is_empty());
}

// ---- Disk full (needs `hdiutil`; run with --run-ignored) ----

/// A small APFS disk image, mounted for the test and detached when dropped.
struct DiskImage {
    device: String,
    mount: PathBuf,
    _dir: tempfile::TempDir,
}

impl DiskImage {
    fn new(megabytes: u32) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let volname = format!("YojakaFull{}", std::process::id());
        let out = Command::new("hdiutil")
            .args(["create", "-size"])
            .arg(format!("{megabytes}m"))
            .args(["-fs", "APFS", "-volname", &volname, "-attach"])
            .arg(dir.path().join("full.dmg"))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let device = text.split_whitespace().next().unwrap().to_string();
        Self {
            device,
            mount: PathBuf::from("/Volumes").join(volname),
            _dir: dir,
        }
    }

    fn free(&self) -> u64 {
        let out = Command::new("df")
            .arg("-k")
            .arg(&self.mount)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let cols: Vec<&str> = text.lines().nth(1).unwrap().split_whitespace().collect();
        cols[3].parse::<u64>().unwrap() * 1024
    }

    /// Writes a filler file until at most `leave` bytes are free (`0`: until the disk says no).
    fn fill(&self, leave: u64) -> PathBuf {
        let path = self.mount.join("filler.bin");
        let mut f = std::fs::File::create(&path).unwrap();
        let chunk = vec![7u8; 4 * 1024 * 1024];
        let mut left = if leave == 0 {
            u64::MAX
        } else {
            self.free().saturating_sub(leave)
        };
        while left > 0 {
            let n = left.min(chunk.len() as u64) as usize;
            if f.write_all(&chunk[..n]).is_err() {
                break;
            }
            left -= n as u64;
        }
        let _ = f.sync_all();
        path
    }
}

impl Drop for DiskImage {
    fn drop(&mut self) {
        let _ = Command::new("hdiutil")
            .args(["detach", &self.device, "-force"])
            .output();
    }
}

fn plan_bytes(lab: &Lab) -> u64 {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&lab.plan).unwrap()).unwrap();
    v["summary"]["bytes"].as_u64().unwrap()
}

/// Not enough room at the destination: refused before anything runs.
#[test]
#[ignore = "mounts a disk image"]
fn disk_image_without_room_refuses_before_anything_runs() {
    let disk = DiskImage::new(32);
    let mut lab = Lab::new();
    lab.dest = disk.mount.join("sorted");
    lab.make_plan(Kind::Copy);
    let needed = plan_bytes(&lab);
    assert!(
        needed > 0 && needed * 3 < 24 * 1024 * 1024,
        "needed {needed}"
    );
    let before = files(&lab.tree);
    disk.fill(needed / 2);

    let out = yojaka(lab.home.path())
        .arg("--write")
        .args(["apply"])
        .arg(&lab.plan)
        .arg("--yes")
        .write_stdin(format!("{}\n", lab.phrase(Kind::Copy)))
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1), "refused as a whole");
    let said =
        String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout);
    assert!(said.contains("not enough free space"), "{said}");
    assert_eq!(files(&lab.tree), before);
    assert!(files(&lab.dest).is_empty(), "nothing was copied");
    assert_eq!(lab.done_lines("apply"), 0);
}

/// The disk fills up while the copy runs: each file that did not fit leaves no partial file, the
/// run reports a partial failure, the originals are untouched, and once there is room again
/// `--resume` finishes the plan and `undo` removes exactly the copies.
#[test]
#[ignore = "mounts a disk image"]
fn disk_image_that_fills_during_the_run_leaves_no_partial_file() {
    // Plenty of room at the start (the check passes); someone fills the disk during the run.
    let disk = DiskImage::new(256);
    let mut lab = Lab::new();
    lab.dest = disk.mount.join("sorted");
    lab.make_plan(Kind::Copy);
    let n = lab.file_ops();
    assert!(disk.free() > plan_bytes(&lab) + 70 * 1024 * 1024);
    let before = files(&lab.tree);
    let source_hashes: BTreeSet<u64> = before.values().copied().collect();

    let args = lab.apply_args(false);
    let mut child = lab.spawn_slow(Kind::Copy, &args);
    let deadline = Instant::now() + Duration::from_secs(60);
    while lab.done_lines("apply") < 3 {
        assert!(
            Instant::now() < deadline && child.try_wait().unwrap().is_none(),
            "the run ended early: {}",
            lab.child_log()
        );
        std::thread::sleep(Duration::from_millis(3));
    }
    let filler = disk.fill(0); // someone else uses up the rest of the disk
    let status = child.wait().unwrap();

    assert_eq!(status.code(), Some(3), "a partial failure");
    assert_eq!(files(&lab.tree), before, "the originals are untouched");
    assert!(
        stray_temp_files(&lab.dest).is_empty(),
        "no temporary file is left behind"
    );
    let copied = files(&lab.dest);
    assert!(
        !copied.is_empty() && copied.len() < n,
        "{} of {n}",
        copied.len()
    );
    for (path, hash) in &copied {
        assert!(
            source_hashes.contains(hash),
            "{} is not a complete copy",
            path.display()
        );
    }

    // Room again: resume finishes, undo removes exactly the copies.
    std::fs::remove_file(&filler).unwrap();
    let resume = lab.apply_args(true);
    yojaka(lab.home.path())
        .arg("--write")
        .args(&resume)
        .write_stdin(format!("{}\n", lab.phrase(Kind::Copy)))
        .assert()
        .success();
    assert_eq!(files(&lab.dest).len(), n);
    assert_eq!(files(&lab.tree), before);
    yojaka(lab.home.path())
        .args(["--write", "undo", "--last"])
        .assert()
        .success();
    assert!(files(&lab.dest).is_empty());
    assert_eq!(files(&lab.tree), before);
}
