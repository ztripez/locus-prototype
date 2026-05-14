//! Declared architectural concept (e.g. "user", "order").
//!
//! Concepts are the architectural domain vocabulary the rest of the
//! model hangs off.

// locus: ot canonical

use serde::{Deserialize, Serialize};

use super::source::SourceRef;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct ConceptFact {
    pub id: String,
    pub source_of_truth: Option<String>,
    pub registry: Option<String>,
    pub source: SourceRef,
}
