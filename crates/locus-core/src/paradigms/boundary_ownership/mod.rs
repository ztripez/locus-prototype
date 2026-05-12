//! BO — Boundary Ownership.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 5: Boundary Ownership". Reads the
//! domain/application layer split from `paradigms.BO` in `locus.lock` and
//! flags imports of transport/persistence/serialization paths from inside
//! the domain layer.
//!
//! Phase scope so far:
//! - BO001: domain layer imports a transport/persistence dependency.
//! - BO002: domain function exposes a persistence-shaped type in its
//!   parameter or return signature.
//! - BO004: canonical type carries a forbidden derive
//!   (e.g. `Serialize`/`Deserialize`/`ToSchema`).
//! - BO005: domain function performs a persistence write
//!   (`std::fs::write`/`create_dir`/`remove_*` etc., via the std-rt
//!   loader's `PersistenceWrite` facts).

// locus: ot canonical

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
    fn check(
        &self,
        _air: &AirWorkspace,
        _lockfile: &Lockfile,
        _mode: CheckMode,
    ) -> Vec<Diagnostic> {
        // All BO rules migrated to RuleDefinition (#71 P4).
        // Detection runs through the governance pipeline; this legacy
        // path is now a no-op.
        Vec::new()
    }
}
