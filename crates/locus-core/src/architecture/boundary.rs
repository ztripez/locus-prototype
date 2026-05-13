//! Declared architectural boundary (HTTP edge, CLI surface, persistence
//! layer, etc.).

// locus: ot canonical

use serde::{Deserialize, Serialize};

use super::source::SourceRef;

/// Boundary classification: where the architecture meets the outside
/// world. Kept as a small closed enum; loaders that need a richer
/// taxonomy use [`BoundaryKind::Other`] and carry the detail in the
/// boundary id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BoundaryKind {
    Http,
    Cli,
    Persistence,
    Ffi,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct BoundaryFact {
    pub id: String,
    pub kind: BoundaryKind,
    pub adapters_allowed: bool,
    pub source: SourceRef,
}
