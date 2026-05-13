//! Declared contract reference (e.g. an OpenAPI operation, a JSON Schema
//! path).

// locus: ot canonical

use serde::{Deserialize, Serialize};

use super::source::SourceRef;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct ContractFact {
    /// e.g. `"openapi"`. Kept as String (not enum) so loaders can extend.
    pub source_kind: String,
    pub operation: Option<String>,
    pub path: Option<String>,
    pub schema: Option<String>,
    pub source: SourceRef,
}
