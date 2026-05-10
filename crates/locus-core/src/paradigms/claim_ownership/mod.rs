//! CL — Claim Ownership.
//!
//! Spec: `docs/superpowers/specs/2026-05-09-claim-ownership-paradigm.md`
//! (issue #16). Detects high-risk natural-language claims in comments
//! and docs without grading prose quality and without LLM-in-the-loop.
//!
//! Phase scope so far:
//! - CL001: doc comment cites an external reference (`#NN`, URL) but
//!   carries no local rationale. Heuristic: < 5 non-reference word
//!   tokens after stripping recognised reference shapes.
//!
//! `init` returns `Null`: the policy choice ("require local rationale
//! alongside external references") is a deliberate user decision,
//! mirroring DC001's `require_public_docs` opt-in. CL is silent until
//! the user sets `paradigms.CL.require_local_rationale = true`.
//!
//! Future CL rules (CL002 temporal, CL003 sync, CL004 generated, CL005
//! status, CL006 safety) need a richer text-claim AIR shape; the design
//! doc spells out the migration path.

// locus: ot canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod lockfile_schema;
pub mod rules;

pub const CL_PREFIX: &str = "CL";

pub struct ClaimOwnership;

impl Paradigm for ClaimOwnership {
    fn name(&self) -> &'static str {
        "Claim Ownership"
    }
    fn rule_prefix(&self) -> &'static str {
        CL_PREFIX
    }
    fn init(&self, _air: &AirWorkspace) -> serde_json::Value {
        // Policy choice. CL is silent until the user opts in via
        // `paradigms.CL.require_local_rationale = true`.
        serde_json::Value::Null
    }
    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic> {
        let section: lockfile_schema::ClSection =
            lockfile.paradigm_section(CL_PREFIX).unwrap_or_default();
        rules::cl001(air, &section, mode)
    }
}
