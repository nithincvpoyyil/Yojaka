use std::path::Path;

use yojaka_core::Engine;

use crate::exit::Exit;

pub fn run(
    engine: &Engine,
    path: Option<&Path>,
    json: bool,
    local_index: bool,
) -> anyhow::Result<Exit> {
    let root = path.unwrap_or_else(|| Path::new("."));
    let loc = engine.places(root, local_index)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&loc)?);
        return Ok(Exit::Ok);
    }
    let total = loc.with_place + loc.without_place;
    let pct = |n: u64| {
        if total == 0 {
            0.0
        } else {
            n as f64 * 100.0 / total as f64
        }
    };
    println!(
        "{} of {} files have a place ({:.0}%); {} have GPS",
        loc.with_place,
        total,
        pct(loc.with_place),
        loc.with_gps
    );
    println!("\n  Countries");
    for c in &loc.countries {
        println!(
            "    {:<28} {:>7}",
            format!("{} ({})", c.country, c.country_code),
            c.count
        );
    }
    println!("\n  Cities");
    for c in &loc.cities {
        let state = c
            .state
            .as_deref()
            .map(|s| format!(", {s}"))
            .unwrap_or_default();
        println!(
            "    {:<40} {:>7}",
            format!("{}{state}, {}", c.city, c.country),
            c.count
        );
    }
    println!("\n  (place names come from an offline database; nothing leaves this machine)");
    Ok(Exit::Ok)
}
