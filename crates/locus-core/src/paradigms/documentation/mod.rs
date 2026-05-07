//! DC — Documentation / Comment Ownership.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 17: Documentation / Comment Ownership".
//!
//! Phase scope so far:
//! - DC001: public type or function has no doc comment. Opt-in via
//!   `paradigms.DC.require_public_docs`; silent by default.

// ot: canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
pub mod lockfile_schema;
pub mod rules;

pub const DC_PREFIX: &str = "DC";

pub struct Documentation;

impl Paradigm for Documentation {
    fn name(&self) -> &'static str {
        "Documentation / Comment Ownership"
    }
    fn rule_prefix(&self) -> &'static str {
        DC_PREFIX
    }
    fn init(&self, _air: &AirWorkspace) -> serde_json::Value {
        // No automatic inference: `require_public_docs` is a project policy
        // choice, and `exempt_paths` is hand-curated. `init` returns an
        // empty section; users opt in via the lockfile directly (or via a
        // future `locus dc` CLI mutator).
        serde_json::Value::Null
    }
    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic> {
        let section: lockfile_schema::DcSection =
            lockfile.paradigm_section(DC_PREFIX).unwrap_or_default();
        rules::dc001(air, &section, mode)
    }
}
