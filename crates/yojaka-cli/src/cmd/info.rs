use std::path::Path;

use yojaka_core::Engine;

use super::fmt::{bytes, duration};
use crate::exit::Exit;

pub fn run(engine: &Engine, file: &Path, json: bool) -> anyhow::Result<Exit> {
    let r = engine.info(file)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&r)?);
        return Ok(Exit::Ok);
    }
    let mut rows: Vec<(&str, String)> = vec![
        ("File", r.path.display().to_string()),
        (
            "Format",
            format!("{} ({})", r.format.as_str(), bytes(r.size_bytes)),
        ),
    ];
    if let (Some(w), Some(h)) = (r.width, r.height) {
        let rot = r
            .rotation
            .filter(|d| *d != 0)
            .map(|d| format!(", rotated {d}°"))
            .unwrap_or_default();
        rows.push(("Size", format!("{w} × {h}{rot}")));
    }
    if let Some(t) = &r.taken_at {
        rows.push(("Taken", format!("{t}  (from {})", r.taken_at_src)));
    } else {
        rows.push(("Taken", "no embedded date".into()));
    }
    let camera = [r.camera_make.as_deref(), r.camera_model.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    if !camera.is_empty() {
        rows.push(("Camera", camera));
    }
    if let Some(l) = &r.lens {
        rows.push(("Lens", l.clone()));
    }
    if let Some(d) = r.duration_ms {
        rows.push(("Duration", duration(d)));
    }
    if let Some(f) = r.fps {
        rows.push(("Frame rate", format!("{f} fps")));
    }
    let codecs = [r.video_codec.as_deref(), r.audio_codec.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" + ");
    if !codecs.is_empty() {
        rows.push(("Codecs", codecs));
    }
    if let Some(n) = r.frames.filter(|n| *n > 1) {
        rows.push(("Frames", n.to_string()));
    }
    if let Some(id) = &r.content_id {
        rows.push(("Content id", id.clone()));
    }
    if let Some(g) = r.gps {
        rows.push(("GPS", format!("{:.5}, {:.5}", g.lat, g.lon)));
    }
    if let Some(p) = &r.place {
        let g = p.guarded(
            engine.config.location.near_km,
            engine.config.location.far_km,
        );
        let place = [g.city, p.admin1.clone(), g.country]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(", ");
        rows.push((
            "Place",
            format!(
                "{} ({:.1} km to {})",
                if place.is_empty() {
                    "unknown".into()
                } else {
                    place
                },
                p.dist_km,
                p.city
            ),
        ));
    }
    for (k, v) in rows {
        println!("{k:<11} {v}");
    }
    Ok(Exit::Ok)
}
