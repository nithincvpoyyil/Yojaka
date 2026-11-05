//! Organize templates (Iteration 6). Shapes only.

use crate::{Result, YojakaError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template(String);

impl Template {
    pub fn parse(_src: &str) -> Result<Self> {
        Err(YojakaError::NotYet("organize templates", "Iteration 6"))
    }
}
