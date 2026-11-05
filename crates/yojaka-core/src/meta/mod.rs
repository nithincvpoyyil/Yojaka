//! Metadata extraction (Iteration 2). Shapes only.

use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Meta {
    pub taken_at: Option<String>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
    pub gps: Option<crate::geo::GeoPoint>,
}

#[derive(Debug, thiserror::Error)]
pub enum MetaError {
    #[error("unsupported format")]
    Unsupported,
    #[error("malformed metadata: {0}")]
    Malformed(String),
}

pub trait Extractor: Send + Sync {
    fn extract(&self, path: &Path) -> Result<Meta, MetaError>;
}
