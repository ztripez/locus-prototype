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
use crate::diagnostics::{CheckMode, Diagnostic, vacant_paradigm_diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
pub mod lockfile_schema;
pub mod rules;

pub const BO_PREFIX: &str = "BO";

// locus: allow MO005 — paradigm host struct intentionally lives in mod.rs by convention
pub struct BoundaryOwnership;

// locus: allow MO005 — paradigm Paradigm impl intentionally lives in mod.rs by convention
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
        if section.is_vacant() && !lockfile.is_acknowledged_empty(BO_PREFIX) {
            return vec![vacant_paradigm_diagnostic(
                BO_PREFIX,
                "Boundary Ownership",
                &[
                    ("domain_paths", "module patterns identifying domain code"),
                    (
                        "forbidden_in_domain",
                        "import paths domain code must not reach",
                    ),
                    (
                        "persistence_type_patterns",
                        "persistence-shaped types forbidden in domain signatures",
                    ),
                    (
                        "canonical_paths",
                        "module patterns identifying canonical types (BO004)",
                    ),
                ],
            )];
        }
        let mut diags = rules::bo001(air, &section, mode);
        diags.extend(rules::bo002(air, &section, mode));
        diags.extend(rules::bo004(air, &section, mode));
        diags.extend(rules::bo005(air, &section, mode));
        diags
    }
}
