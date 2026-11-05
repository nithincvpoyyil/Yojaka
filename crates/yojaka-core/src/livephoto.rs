//! Live Photo pairing (Iteration 2). A pair is never split.

/// Shared `content.identifier` of a HEIC/JPEG + MOV pair.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PairId(pub String);
