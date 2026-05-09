//! AB — Abstraction Discipline.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 16: Abstraction Discipline".
//!
//! Detects speculative abstraction — traits / interfaces added "in case"
//! other implementations exist someday but in practice point at exactly one
//! concrete type with no boundary role. The `manager / processor /
//! DataHandler` pattern from the spec.
//!
//! Phase scope so far:
//! - AB001: trait declared in the workspace has exactly one impl.
//! - AB002: type named after a generic role (`*Manager`, `*Service`,
//!   `*Processor`, …) without an accepted abstraction record.
//!
//! `init` returns `Null`: there's no automatic inference for "this
//! single-impl trait is a real port" or "this `*Manager` is the right
//! domain term." Acceptance is a deliberate user action, mirroring
//! DG/MO/UT.

// locus: ot canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
pub mod lockfile_schema;
pub mod rules;

pub const AB_PREFIX: &str = "AB";

pub struct AbstractionDiscipline;

impl Paradigm for AbstractionDiscipline {
    fn name(&self) -> &'static str {
        "Abstraction Discipline"
    }
    fn rule_prefix(&self) -> &'static str {
        AB_PREFIX
    }
    fn init(&self, _air: &AirWorkspace) -> serde_json::Value {
        serde_json::Value::Null
    }
    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic> {
        let section: lockfile_schema::AbSection =
            lockfile.paradigm_section(AB_PREFIX).unwrap_or_default();
        let mut diags = rules::ab001(air, &section, mode);
        diags.extend(rules::ab002(air, &section, mode));
        diags
    }
}
