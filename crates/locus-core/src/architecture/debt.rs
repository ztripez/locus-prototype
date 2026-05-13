//! Declared piece of accepted architectural debt.

// locus: ot canonical

use serde::{Deserialize, Serialize};

use super::source::SourceRef;

/// What a [`DebtFact`] targets. Tagged with `kind`/`value` so the JSON
/// shape is `{"kind": "concept", "value": "user"}` regardless of which
/// variant is in play. This keeps round-trip determinism predictable
/// for both human-readable diagnostics and downstream tooling.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "kebab-case")]
pub enum DebtTarget {
    Concept(String),
    Boundary(String),
    Policy(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct DebtFact {
    pub target: DebtTarget,
    pub reason: String,
    pub issue: Option<String>,
    pub expires: Option<String>,
    pub source: SourceRef,
}
