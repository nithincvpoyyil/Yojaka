use super::*;

fn at(lat: f64, lon: f64) -> Place {
    EmbeddedGeocoder
        .lookup(GeoPoint { lat, lon })
        .expect("database loads")
}

/// (label, lat, lon, expected country code, expected city substring or None to skip)
const KNOWN: &[(&str, f64, f64, &str, Option<&str>)] = &[
    ("Panaji, India", 15.4989, 73.8278, "IN", Some("Panjim")),
    ("Paris", 48.8584, 2.2945, "FR", Some("Paris")),
    ("New York", 40.7484, -73.9857, "US", Some("New York")),
    ("Sydney", -33.8568, 151.2153, "AU", Some("Sydney")),
    ("Tokyo", 35.6586, 139.7454, "JP", Some("Tokyo")),
    ("Cape Town", -33.9249, 18.4241, "ZA", Some("Cape Town")),
    (
        "Buenos Aires",
        -34.6037,
        -58.3816,
        "AR",
        Some("Buenos Aires"),
    ),
    ("Reykjavik", 64.1466, -21.9426, "IS", Some("Reykjav")),
    (
        "Quito, near the equator",
        -0.1807,
        -78.4678,
        "EC",
        Some("Quito"),
    ),
    ("Greenwich meridian", 51.4769, 0.0, "GB", None),
    // antimeridian: both sides of 180 degrees
    ("Taveuni, Fiji (east of 180)", -16.85, 179.98, "FJ", None),
    ("Fiji (west of 180)", -16.80, -179.95, "FJ", None),
    ("Chukotka, Russia", 64.73, 177.5, "RU", Some("Anadyr")),
    // Border towns. Nearest-place lookup can pick the wrong side within a few km of a border
    // (accepted approximation, FR-44), so these sit a little inside each country.
    ("Tijuana", 32.5149, -117.0382, "MX", Some("Tijuana")),
    ("Chula Vista, US side", 32.6401, -117.0842, "US", None),
    (
        "Frankfurt (Oder)",
        52.3471,
        14.5506,
        "DE",
        Some("Frankfurt"),
    ),
    ("Slubice, Poland", 52.3480, 14.5640, "PL", None),
    ("Kigali", -1.9441, 30.0619, "RW", Some("Kigali")),
    (
        "Longyearbyen, Svalbard",
        78.2232,
        15.6267,
        "SJ",
        Some("Longyearbyen"),
    ),
    ("Ushuaia", -54.8019, -68.3030, "AR", Some("Ushuaia")),
    ("Honolulu", 21.3069, -157.8583, "US", Some("Honolulu")),
    ("Tromso", 69.6492, 18.9553, "NO", Some("Troms")),
];

#[test]
fn known_coordinates_resolve() {
    assert!(KNOWN.len() >= 20);
    for (label, lat, lon, cc, city) in KNOWN {
        let p = at(*lat, *lon);
        assert_eq!(&p.country_code, cc, "{label}: got {p:?}");
        if let Some(c) = city {
            // Neighbourhoods are places too ("The Rocks"), so accept the city or its district.
            let ctx = format!(
                "{} {} {}",
                p.city,
                p.admin1.clone().unwrap_or_default(),
                p.admin2.clone().unwrap_or_default()
            );
            assert!(ctx.contains(c), "{label}: got {p:?}");
        }
    }
}

#[test]
fn admin_names_are_resolved() {
    let p = at(15.4989, 73.8278);
    assert_eq!(p.country, "India");
    assert_eq!(p.admin1.as_deref(), Some("Goa"));
    assert!(p.dist_km < 10.0, "{p:?}");
}

#[test]
fn open_sea_and_poles_have_no_country_but_never_panic() {
    // Point Nemo, middle of the Pacific
    let nemo = at(-48.8767, -123.3933);
    assert!(nemo.dist_km > 1000.0, "{nemo:?}");
    let g = nemo.guarded(25.0, 100.0);
    assert_eq!(
        g,
        Guarded {
            city: None,
            country: None
        }
    );

    for (lat, lon) in [(90.0, 0.0), (-90.0, 0.0), (-90.0, 123.0), (89.99, 179.99)] {
        let p = at(lat, lon);
        assert!(p.dist_km.is_finite());
    }
    // Antarctica: nothing within 300 km of the pole
    assert!(at(-90.0, 0.0).guarded(25.0, 100.0).country.is_none());
}

#[test]
fn antimeridian_neighbours_are_close() {
    // The two Fiji points are ~12 km apart across 180 degrees.
    let a = at(-16.85, 179.98);
    let b = at(-16.85, -179.98);
    assert_eq!(a.country_code, "FJ");
    assert_eq!(b.country_code, "FJ");
    let d = haversine_km(
        GeoPoint {
            lat: -16.85,
            lon: 179.98,
        },
        GeoPoint {
            lat: -16.85,
            lon: -179.98,
        },
    );
    assert!(d < 10.0, "{d}");
}

#[test]
fn precision_guard() {
    let mk = |d: f64| Place {
        city: "Panaji".into(),
        admin1: None,
        admin2: None,
        country_code: "IN".into(),
        country: "India".into(),
        dist_km: d,
    };
    assert_eq!(mk(3.0).guarded(25.0, 100.0).city.as_deref(), Some("Panaji"));
    assert_eq!(
        mk(40.0).guarded(25.0, 100.0).city.as_deref(),
        Some("Near Panaji")
    );
    assert_eq!(mk(150.0).guarded(25.0, 100.0).city, None);
    assert_eq!(
        mk(150.0).guarded(25.0, 100.0).country.as_deref(),
        Some("India")
    );
    assert_eq!(mk(400.0).guarded(25.0, 100.0).country, None);
}

#[test]
fn bad_coordinates_are_rejected() {
    assert!(GeoPoint::checked(0.0, 0.0).is_none());
    assert!(GeoPoint::checked(91.0, 10.0).is_none());
    assert!(GeoPoint::checked(10.0, 181.0).is_none());
    assert!(GeoPoint::checked(f64::NAN, 10.0).is_none());
    assert!(GeoPoint::checked(-90.0, 0.0).is_some());
}

#[test]
fn kdtree_matches_brute_force() {
    use crate::geo::kdtree::KdTree;
    let mut state = 12345u64;
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    let pts: Vec<[f64; 3]> = (0..2000)
        .map(|_| unit_vector(next() * 180.0 - 90.0, next() * 360.0 - 180.0))
        .collect();
    let tree = KdTree::build(pts.clone());
    for _ in 0..300 {
        let q = unit_vector(next() * 180.0 - 90.0, next() * 360.0 - 180.0);
        let (i, d) = tree.nearest(q).unwrap();
        let (bi, bd) = pts
            .iter()
            .enumerate()
            .map(|(i, p)| (i, (0..3).map(|k| (p[k] - q[k]).powi(2)).sum::<f64>()))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        assert!((d - bd).abs() < 1e-12, "{i} vs {bi}");
    }
}

#[test]
fn hundred_thousand_lookups_are_fast() {
    assert!(EmbeddedGeocoder.is_available());
    let mut state = 99u64;
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    let started = std::time::Instant::now();
    let mut found = 0u32;
    for _ in 0..100_000 {
        let p = GeoPoint {
            lat: next() * 180.0 - 90.0,
            lon: next() * 360.0 - 180.0,
        };
        found += u32::from(EmbeddedGeocoder.lookup(p).is_some());
    }
    let took = started.elapsed();
    assert_eq!(found, 100_000);
    // Debug builds are far slower; the 1 s budget applies to optimised builds.
    if !cfg!(debug_assertions) {
        assert!(took.as_secs_f64() < 1.0, "{took:?}");
    }
}
