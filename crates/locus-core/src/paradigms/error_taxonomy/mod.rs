//! ER — Error Taxonomy Ownership.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 13: Error Taxonomy Ownership".
//!
//! Phase scope:
//! - ER001: multiple public error types in the same module (heuristic
//!   warning that flags taxonomy forks like `UserError` + `CreateUserError`
//!   sitting next to each other in one file).
//!
//! ER001 is lockfile-free; the rule fires from AIR alone. Future rules will
//! consume the (currently empty) [`lockfile_schema::ErSection`].

// ot: canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod lockfile_schema;
pub mod rules;

pub const ER_PREFIX: &str = "ER";

pub struct ErrorTaxonomy;

impl Paradigm for ErrorTaxonomy {
    fn name(&self) -> &'static str {
        "Error Taxonomy Ownership"
    }
    fn rule_prefix(&self) -> &'static str {
        ER_PREFIX
    }
    fn init(&self, _air: &AirWorkspace) -> serde_json::Value {
        // No automatic inference yet — ER001 is heuristic and lockfile-free.
        // ER002+ may populate accepted-error entries here.
        serde_json::Value::Null
    }
    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic> {
        let section: lockfile_schema::ErSection =
            lockfile.paradigm_section(ER_PREFIX).unwrap_or_default();
        rules::er001(air, &section, mode)
    }
}
