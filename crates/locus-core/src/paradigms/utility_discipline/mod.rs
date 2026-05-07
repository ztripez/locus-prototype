//! UT — Utility / Shared Module Discipline.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 11: Utility / Shared Module
//! Discipline". Reads declared utility module patterns from
//! `paradigms.UT.utility_paths` in `locus.lock` and flags public types
//! defined inside any matching module — utility modules are by definition
//! domain-free, and a public type carries semantics that should live in a
//! domain/feature module instead.
//!
//! Phase scope so far:
//! - UT001: utility module defines a public type.
//! - UT002: utility module imports a forbidden feature/domain path.

// ot: canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
pub mod lockfile_schema;
pub mod rules;

pub const UT_PREFIX: &str = "UT";

pub struct UtilityDiscipline;

impl Paradigm for UtilityDiscipline {
    fn name(&self) -> &'static str {
        "Utility / Shared Module Discipline"
    }
    fn rule_prefix(&self) -> &'static str {
        UT_PREFIX
    }
    fn init(&self, _air: &AirWorkspace) -> serde_json::Value {
        // Utility status is a user assertion, not an inference. `init` returns
        // an empty section; the user adds patterns via `locus ut add-utility-path`.
        serde_json::Value::Null
    }
    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic> {
        let section: lockfile_schema::UtSection =
            lockfile.paradigm_section(UT_PREFIX).unwrap_or_default();
        let mut diags = rules::ut001(air, &section, mode);
        diags.extend(rules::ut002(air, &section, mode));
        diags
    }
}
