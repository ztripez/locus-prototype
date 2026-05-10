//! Shape of the OT section inside `locus.lock`.
//!
//! Each entry carries its `source` (hint / init / accepted) so reviewers
//! can tell why a symbol was promoted. `init` means "Locus inferred this
//! during `locus init` from a strong signal"; `hint` means "the symbol
//! carries a `// locus: ot …` annotation"; `accepted` is the future `locus accept`
//! path.

// locus: ot canonical

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct OtSection {
    /// concept-id → entry. Concept ids come from the inferred name stem
    /// (e.g. `user`, `email-address`).
    #[serde(default)]
    pub concepts: BTreeMap<String, ConceptEntry>,
    /// Module/function patterns with converter authority for OT004.
    /// Useful for adapter surfaces that intentionally construct canonicals
    /// across crate boundaries.
    #[serde(default)]
    pub converter_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConceptEntry {
    pub canonical: AcceptedCanonical,
    #[serde(default)]
    pub boundaries: Vec<AcceptedBoundary>,
    #[serde(default)]
    pub converters: Vec<AcceptedConverter>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcceptedCanonical {
    pub symbol: String,
    pub source: Source,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcceptedBoundary {
    pub symbol: String,
    pub boundary: Option<String>,
    pub source: Source,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcceptedConverter {
    pub from: String,
    pub to: String,
    pub symbol: String,
    pub source: Source,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// Recorded because a `// locus: ot …` hint exists on the type.
    Hint,
    /// Promoted by `locus init` from an inferred candidate (currently:
    /// converters whose endpoints are both accepted).
    Init,
    /// Recorded by `locus accept` (Phase 2.B).
    Accepted,
}

impl OtSection {
    /// Lookup by canonical-or-boundary symbol. Returns the role recorded for
    /// the symbol and the concept it belongs to, or `None` if not in the
    /// lockfile.
    pub fn role_of(&self, symbol: &str) -> Option<(LockedRole, &str)> {
        for (concept_id, entry) in &self.concepts {
            if entry.canonical.symbol == symbol {
                return Some((LockedRole::Canonical, concept_id.as_str()));
            }
            if entry.boundaries.iter().any(|b| b.symbol == symbol) {
                return Some((LockedRole::Boundary, concept_id.as_str()));
            }
        }
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockedRole {
    Canonical,
    Boundary,
}
