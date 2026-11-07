//! Builds `crates/yojaka-core/data/geo.bin.zst` from the GeoNames dumps.
//!
//! Usage: `cargo run -p geodata -- <dump-dir> <out-file>`
//! where `<dump-dir>` holds `cities1000.txt`, `admin1CodesASCII.txt`, `admin2Codes.txt`
//! and `countryInfo.txt` (see README.md).
//!
//! Blob layout (little-endian, zstd-compressed as a whole):
//! ```text
//! "YGEO" u32 version
//! u32 n_countries  { cc[2]  str name }            str = u16 len + utf-8 bytes
//! u32 n_admin1     { str "CC.A1" , str name }
//! u32 n_admin2     { str "CC.A1.A2" , str name }
//! u32 n_names      { str }                         city names
//! u32 n_places     { f32 lat, f32 lon, u32 name, u16 country, u32 admin1, u32 admin2 }
//!                                                  admin index u32::MAX = none
//! ```

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::collections::HashMap;
use std::fs;
use std::path::Path;

const NONE: u32 = u32::MAX;

fn put_str(out: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    let len = u16::try_from(b.len()).unwrap_or(u16::MAX);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&b[..usize::from(len)]);
}

fn read(dir: &Path, name: &str) -> String {
    fs::read_to_string(dir.join(name)).unwrap_or_else(|e| {
        eprintln!("cannot read {name}: {e}");
        std::process::exit(1);
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: geodata <dump-dir> <out-file>");
        std::process::exit(2);
    }
    let (dir, out_path) = (Path::new(&args[1]), Path::new(&args[2]));

    // countries: ISO code -> name
    let mut countries: Vec<(String, String)> = Vec::new();
    let mut country_idx: HashMap<String, u16> = HashMap::new();
    for line in read(dir, "countryInfo.txt").lines() {
        if line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() > 4 && f[0].len() == 2 {
            country_idx.insert(f[0].to_string(), countries.len() as u16);
            countries.push((f[0].to_string(), f[4].to_string()));
        }
    }
    let admin1_raw: HashMap<String, String> = read(dir, "admin1CodesASCII.txt")
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() > 1).then(|| (f[0].to_string(), f[1].to_string()))
        })
        .collect();
    let admin2_raw: HashMap<String, String> = read(dir, "admin2Codes.txt")
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() > 1).then(|| (f[0].to_string(), f[1].to_string()))
        })
        .collect();

    let mut admin1: Vec<(String, String)> = Vec::new();
    let mut admin1_idx: HashMap<String, u32> = HashMap::new();
    let mut admin2: Vec<(String, String)> = Vec::new();
    let mut admin2_idx: HashMap<String, u32> = HashMap::new();
    let mut names: Vec<String> = Vec::new();
    let mut name_idx: HashMap<String, u32> = HashMap::new();
    let mut places: Vec<u8> = Vec::new();
    let mut n_places = 0u32;

    for line in read(dir, "cities1000.txt").lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 13 {
            continue;
        }
        let (Ok(lat), Ok(lon)) = (f[4].parse::<f32>(), f[5].parse::<f32>()) else {
            continue;
        };
        let Some(&cc) = country_idx.get(f[8]) else {
            continue;
        };
        let name = f[1].to_string();
        let ni = *name_idx.entry(name.clone()).or_insert_with(|| {
            names.push(name);
            (names.len() - 1) as u32
        });
        let mut a1 = NONE;
        if !f[10].is_empty() {
            let key = format!("{}.{}", f[8], f[10]);
            if let Some(n) = admin1_raw.get(&key) {
                a1 = *admin1_idx.entry(key.clone()).or_insert_with(|| {
                    admin1.push((key, n.clone()));
                    (admin1.len() - 1) as u32
                });
            }
        }
        let mut a2 = NONE;
        if !f[10].is_empty() && !f[11].is_empty() {
            let key = format!("{}.{}.{}", f[8], f[10], f[11]);
            if let Some(n) = admin2_raw.get(&key) {
                a2 = *admin2_idx.entry(key.clone()).or_insert_with(|| {
                    admin2.push((key, n.clone()));
                    (admin2.len() - 1) as u32
                });
            }
        }
        places.extend_from_slice(&lat.to_le_bytes());
        places.extend_from_slice(&lon.to_le_bytes());
        places.extend_from_slice(&ni.to_le_bytes());
        places.extend_from_slice(&cc.to_le_bytes());
        places.extend_from_slice(&a1.to_le_bytes());
        places.extend_from_slice(&a2.to_le_bytes());
        n_places += 1;
    }

    let mut out = Vec::new();
    out.extend_from_slice(b"YGEO");
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&(countries.len() as u32).to_le_bytes());
    for (cc, name) in &countries {
        out.extend_from_slice(cc.as_bytes());
        put_str(&mut out, name);
    }
    for table in [&admin1, &admin2] {
        out.extend_from_slice(&(table.len() as u32).to_le_bytes());
        for (k, v) in table {
            put_str(&mut out, k);
            put_str(&mut out, v);
        }
    }
    out.extend_from_slice(&(names.len() as u32).to_le_bytes());
    for n in &names {
        put_str(&mut out, n);
    }
    out.extend_from_slice(&n_places.to_le_bytes());
    out.extend_from_slice(&places);

    let packed = zstd::encode_all(&out[..], 19).unwrap_or_else(|e| {
        eprintln!("zstd failed: {e}");
        std::process::exit(1);
    });
    if let Some(p) = out_path.parent() {
        let _ = fs::create_dir_all(p);
    }
    fs::write(out_path, &packed).unwrap_or_else(|e| {
        eprintln!("cannot write {}: {e}", out_path.display());
        std::process::exit(1);
    });
    println!(
        "{n_places} places, {} countries, {} admin1, {} admin2: {} KiB raw -> {} KiB",
        countries.len(),
        admin1.len(),
        admin2.len(),
        out.len() / 1024,
        packed.len() / 1024
    );
}
