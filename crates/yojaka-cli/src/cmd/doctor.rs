use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};
use yojaka_core::Engine;

use crate::exit::Exit;

fn writable(dir: &Path) -> bool {
    let probe = dir.join(".yojaka-write-probe");
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

fn tool_version(name: &str, arg: &str) -> Value {
    match Command::new(name).arg(arg).output() {
        Ok(o) if o.status.success() => {
            let out = String::from_utf8_lossy(&o.stdout);
            let line = out.lines().next().unwrap_or("").trim().to_string();
            json!({ "found": true, "version": line })
        }
        _ => json!({ "found": false }),
    }
}

fn sysctl(key: &str) -> Option<u32> {
    let o = Command::new("sysctl").args(["-n", key]).output().ok()?;
    String::from_utf8_lossy(&o.stdout).trim().parse().ok()
}

fn full_disk_access() -> &'static str {
    let Some(home) = std::env::var_os("HOME") else {
        return "unknown";
    };
    match std::fs::read_dir(Path::new(&home).join("Library/Mail")) {
        Ok(_) => "granted",
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => "not granted",
        Err(_) => "unknown",
    }
}

pub fn run(engine: &Engine, json_out: bool) -> Exit {
    let p = &engine.paths;
    let config_file = p.config_file();
    let dir =
        |path: &Path| json!({ "path": path, "exists": path.is_dir(), "writable": writable(path) });
    let macos = Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let perf = sysctl("hw.perflevel0.logicalcpu");
    let eff = sysctl("hw.perflevel1.logicalcpu");
    let auto_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let threads = if engine.config.threads == 0 {
        auto_threads
    } else {
        engine.config.threads
    };

    let report = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "git_sha": env!("YOJAKA_GIT_SHA"),
        "target": env!("YOJAKA_TARGET"),
        "macos": macos,
        "paths": {
            "data": dir(&p.data),
            "index": dir(&p.index_dir()),
            "journal": dir(&p.journal_dir()),
            "logs": dir(&p.logs),
            "config_file": { "path": config_file, "exists": config_file.is_file() },
        },
        "tools": {
            "exiftool": tool_version("exiftool", "-ver"),
            "ffprobe": tool_version("ffprobe", "-version"),
            "ollama": tool_version("ollama", "--version"),
        },
        "full_disk_access": full_disk_access(),
        "cpu": { "performance_cores": perf, "efficiency_cores": eff, "default_threads": threads },
    });

    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
        return Exit::Ok;
    }

    println!(
        "Yojaka {} ({}) {}",
        report["version"].as_str().unwrap_or(""),
        report["git_sha"].as_str().unwrap_or(""),
        report["target"].as_str().unwrap_or("")
    );
    println!("macOS {macos}");
    println!("\nPaths");
    for (name, v) in report["paths"].as_object().into_iter().flatten() {
        let path = v["path"].as_str().unwrap_or("");
        let state = if name == "config_file" {
            if v["exists"] == true {
                "exists"
            } else {
                "not created (defaults in use)"
            }
        } else if v["writable"] == true {
            "writable"
        } else {
            "NOT writable"
        };
        println!("  {name:<12} {path}  [{state}]");
    }
    println!("\nTools");
    for (name, v) in report["tools"].as_object().into_iter().flatten() {
        match v["version"].as_str() {
            Some(ver) if v["found"] == true => println!("  {name:<10} {ver}"),
            _ => println!("  {name:<10} not found"),
        }
    }
    println!(
        "\nFull Disk Access: {}",
        report["full_disk_access"].as_str().unwrap_or("")
    );
    if report["full_disk_access"] == "not granted" {
        println!(
            "  To scan protected folders: System Settings → Privacy & Security → Full Disk Access → add your terminal."
        );
    }
    println!(
        "\nCPU: {} performance + {} efficiency cores; default threads: {threads}",
        perf.map_or("?".into(), |n| n.to_string()),
        eff.map_or("?".into(), |n| n.to_string())
    );
    Exit::Ok
}
