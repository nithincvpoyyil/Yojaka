//! Offline reverse geocoding. Coordinates never leave the machine.
//!
//! The place database is GeoNames `cities1000` (CC-BY 4.0), embedded as a zstd blob built by
//! `tools/geodata`. It is decompressed lazily on the first lookup and indexed in a static 3-D
//! k-d tree over unit-sphere coordinates, so the poles and the antimeridian need no special
//! cases. The nearest place is then ranked by great-circle (haversine) distance.

use std::sync::OnceLock;

mod kdtree;

use kdtree::KdTree;

const BLOB: &[u8] = include_bytes!("../../data/geo.bin.zst");
const EARTH_KM: f64 = 6371.0088;
const NONE: u32 = u32::MAX;
/// Beyond this distance a point is not attributed to any country (open sea, poles).
pub const COUNTRY_MAX_KM: f64 = 300.0;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeoPoint {
    pub lat: f64,
    pub lon: f64,
}

impl GeoPoint {
    /// `None` for out-of-range values, NaN, and the (0, 0) "null island" placeholder.
    pub fn checked(lat: f64, lon: f64) -> Option<Self> {
        let ok = lat.is_finite()
            && lon.is_finite()
            && (-90.0..=90.0).contains(&lat)
            && (-180.0..=180.0).contains(&lon);
        (ok && !(lat == 0.0 && lon == 0.0)).then_some(Self { lat, lon })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocSrc {
    Exif,
    Inferred,
}

impl LocSrc {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exif => "gps",
            Self::Inferred => "inferred",
        }
    }
}

/// The nearest populated place to a point, with its administrative context.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Place {
    pub city: String,
    pub admin1: Option<String>,
    pub admin2: Option<String>,
    pub country_code: String,
    pub country: String,
    pub dist_km: f64,
}

pub trait Geocoder: Send + Sync {
    fn lookup(&self, point: GeoPoint) -> Option<Place>;
}

/// How a place is turned into folder-name parts (FR-45 precision guard).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Guarded {
    /// `Some("Panaji")`, `Some("Near Panaji")`, or `None` when too far.
    pub city: Option<String>,
    /// `None` when the point is not inside the nearest place's country.
    pub country: Option<String>,
}

impl Place {
    pub fn guarded(&self, near_km: f64, far_km: f64) -> Guarded {
        let city = if self.dist_km <= near_km {
            Some(self.city.clone())
        } else if self.dist_km <= far_km {
            Some(format!("Near {}", self.city))
        } else {
            None
        };
        let country = (self.dist_km <= COUNTRY_MAX_KM).then(|| self.country.clone());
        Guarded { city, country }
    }
}

struct Db {
    countries: Vec<(String, String)>,
    admin1: Vec<(String, String)>,
    admin2: Vec<(String, String)>,
    names: Vec<String>,
    /// Parallel to the tree payload: (name, country, admin1, admin2) indexes.
    places: Vec<(u32, u16, u32, u32)>,
    tree: KdTree,
}

struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, tail) = self.0.split_at_checked(n)?;
        self.0 = tail;
        Some(head)
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn str(&mut self) -> Option<String> {
        let n = usize::from(self.u16()?);
        Some(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
}

impl Db {
    fn load() -> Option<Self> {
        let raw = zstd::decode_all(BLOB).ok()?;
        let mut c = Cursor(&raw);
        if c.take(4)? != b"YGEO" || c.u32()? != 1 {
            return None;
        }
        let n = c.u32()?;
        let mut countries = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let cc = String::from_utf8_lossy(c.take(2)?).into_owned();
            countries.push((cc, c.str()?));
        }
        let mut tables: [Vec<(String, String)>; 2] = [Vec::new(), Vec::new()];
        for t in &mut tables {
            for _ in 0..c.u32()? {
                let k = c.str()?;
                t.push((k, c.str()?));
            }
        }
        let [admin1, admin2] = tables;
        let mut names = Vec::new();
        for _ in 0..c.u32()? {
            names.push(c.str()?);
        }
        let n = c.u32()? as usize;
        let mut points = Vec::with_capacity(n);
        let mut places = Vec::with_capacity(n);
        for _ in 0..n {
            let (lat, lon) = (f64::from(c.f32()?), f64::from(c.f32()?));
            points.push(unit_vector(lat, lon));
            places.push((c.u32()?, c.u16()?, c.u32()?, c.u32()?));
        }
        Some(Self {
            countries,
            admin1,
            admin2,
            names,
            places,
            tree: KdTree::build(points),
        })
    }

    fn lookup(&self, p: GeoPoint) -> Option<Place> {
        let q = unit_vector(p.lat, p.lon);
        let (idx, chord2) = self.tree.nearest(q)?;
        let (name, cc, a1, a2) = self.places[idx];
        let (code, country) = self.countries.get(usize::from(cc))?;
        let pick = |table: &[(String, String)], i: u32| {
            (i != NONE)
                .then(|| table.get(i as usize).map(|(_, n)| n.clone()))
                .flatten()
        };
        // chord length on the unit sphere -> central angle -> km
        let angle = 2.0 * (chord2.sqrt() / 2.0).clamp(0.0, 1.0).asin();
        Some(Place {
            city: self.names.get(name as usize)?.clone(),
            admin1: pick(&self.admin1, a1),
            admin2: pick(&self.admin2, a2),
            country_code: code.clone(),
            country: country.clone(),
            dist_km: angle * EARTH_KM,
        })
    }
}

fn unit_vector(lat: f64, lon: f64) -> [f64; 3] {
    let (la, lo) = (lat.to_radians(), lon.to_radians());
    [la.cos() * lo.cos(), la.cos() * lo.sin(), la.sin()]
}

static DB: OnceLock<Option<Db>> = OnceLock::new();

/// The embedded database (about 170k places). Loaded on first use, then shared.
#[derive(Debug, Clone, Copy, Default)]
pub struct EmbeddedGeocoder;

impl EmbeddedGeocoder {
    /// Identifies the data set; places are re-resolved when this changes.
    pub const DATA_VERSION: u32 = 1;

    pub fn is_available(&self) -> bool {
        DB.get_or_init(Db::load).is_some()
    }
}

impl Geocoder for EmbeddedGeocoder {
    fn lookup(&self, point: GeoPoint) -> Option<Place> {
        DB.get_or_init(Db::load).as_ref()?.lookup(point)
    }
}

/// Great-circle distance in kilometres (used by tests and the inference pass).
pub fn haversine_km(a: GeoPoint, b: GeoPoint) -> f64 {
    let (la, lb) = (a.lat.to_radians(), b.lat.to_radians());
    let dlat = lb - la;
    let dlon = (b.lon - a.lon).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + la.cos() * lb.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * EARTH_KM * h.sqrt().clamp(0.0, 1.0).asin()
}

#[cfg(test)]
mod tests;
