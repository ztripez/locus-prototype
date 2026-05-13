//! `ConceptSourceOfTruthPolicy` — architecture-intent enforcement.
//!
//! Reads concept declarations from `.locus/arch.json`. For each declared
//! concept, checks whether observed runtime state (findings + their
//! registry references) is consistent with the declared source of truth.
//! Emits one LOCUS005 advisory per bypass.
//!
//! Coexists with `RegistryIntegrityPolicy` (LOCUS003). LOCUS003 tracks
//! migration debt for legacy diagnostics; LOCUS005 tracks declared
//! architecture-intent violations. Legacy diagnostics are LOCUS003's
//! territory only — LOCUS005 explicitly skips them.
//!
//! ## Module layout
//!
//! - `policy` — `ConceptSourceOfTruthPolicy` struct, `PolicyDefinition`
//!   impl, and the four `check_*_concept` routing functions.
//! - `enforcement` — the `(advisory, enforced) × CheckMode` →
//!   `(Severity, DecisionStatus)` mapping.
//! - `emit` — emission helpers: `emit_bypass`, `push_unknown_concept`,
//!   plus the `LOCUS005` finding/decision builders.
//! - `tests` — unit tests (wired via `#[path = "tests/mod.rs"]`).

// locus: ot canonical

use crate::governance::ids::PolicyId;

mod emit;
mod enforcement;
mod policy;

pub use policy::ConceptSourceOfTruthPolicy;

pub const CONCEPT_SOT_ID: PolicyId = PolicyId::new("concept-source-of-truth");

pub(crate) const LOCUS005: &str = "LOCUS005";

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
