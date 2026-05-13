//! Provenance reference for facts loaded from structured sources.

// locus: ot canonical

use serde::{Deserialize, Serialize};

/// Provenance reference. Identifies a structured source artifact.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct SourceRef {
    /// Stable id, e.g. `"openapi:api.yaml"` or `"adr:0042"`. Format is
    /// loader-defined; policies should treat this opaquely.
    pub id: String,
    /// Loader kind tag, e.g. `"openapi"`, `"markdown"`, `"adr"`.
    pub kind: String,
    /// Optional filesystem path relative to the workspace root.
    pub path: Option<String>,
}
