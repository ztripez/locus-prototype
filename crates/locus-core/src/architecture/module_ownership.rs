//! Declared module-to-owner/concept binding.

// locus: ot canonical

use serde::{Deserialize, Serialize};

use super::source::SourceRef;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct ModuleOwnershipFact {
    pub module: String,
    pub owner: Option<String>,
    pub concept: Option<String>,
    pub source: SourceRef,
}
