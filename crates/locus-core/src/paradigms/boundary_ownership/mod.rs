//! BO — Boundary Ownership.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 5: Boundary Ownership". Reads the
//! domain/application layer split from `paradigms.BO` in `locus.lock` and
//! flags imports of transport/persistence/serialization paths from inside
//! the domain layer.
//!
//! Phase scope so far:
//! - BO001: domain layer imports a transport/persistence dependency.

// ot: canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
pub mod lockfile_schema;
pub mod rules;

pub const BO_PREFIX: &str = "BO";

pub struct BoundaryOwnership;

impl Paradigm for BoundaryOwnership {
    fn name(&self) -> &'static str {
        "Boundary Ownership"
    }
    fn rule_prefix(&self) -> &'static str {
        BO_PREFIX
    }
    fn init(&self, _air: &AirWorkspace) -> serde_json::Value {
        // Domain/boundary split is a user assertion, not an inference. `init`
        // returns an empty section; the user populates the lockfile fields
        // directly (or via a future `locus bo` mutator).
        serde_json::Value::Null
    }
    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic> {
        let section: lockfile_schema::BoSection =
            lockfile.paradigm_section(BO_PREFIX).unwrap_or_default();
        rules::bo001(air, &section, mode)
    }
}
