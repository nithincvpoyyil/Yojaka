//! Offline reverse geocoding (Iteration 2). Coordinates never leave the machine.

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeoPoint {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocSrc {
    Exif,
    Inferred,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Place {
    pub city: String,
    pub admin1: Option<String>,
    pub admin2: Option<String>,
    pub country_code: String,
}

pub trait Geocoder: Send + Sync {
    fn lookup(&self, point: GeoPoint) -> Option<Place>;
}
