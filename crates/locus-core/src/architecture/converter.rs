//! Declared converter from one concept/shape to another.

// locus: ot canonical

use serde::{Deserialize, Serialize};

use super::source::SourceRef;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct ConverterFact {
    pub from: String,
    pub to: String,
    pub converter_path: Option<String>,
    pub source: SourceRef,
}
