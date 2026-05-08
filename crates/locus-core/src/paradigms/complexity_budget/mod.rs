//! CX — Complexity Budget Ownership.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 10: Complexity Budget Ownership".
//!
//! Reads `AirItem::Function` items from each file and compares each
//! function's `line_count` against a per-module budget held in the
//! lockfile's CX section. The first CX rule (`CX001`) flags functions
//! whose line count exceeds the configured budget. CX007 caps the public
//! API surface a single file may expose; CX008 caps the number of call
//! sites a single function may issue outside an accepted orchestration
//! module.
//!
//! `init` returns `Null`: there's no automatic inference for "this
//! function is allowed to be long" — the user has to declare the
//! override (or the default) deliberately, same as DG/MO. Without a CX
//! section, CX001 stays silent so un-onboarded code isn't bombarded
//! with line-budget warnings. CX007 ships with sensible defaults and
//! fires immediately; CX008 stays silent until `orchestration_paths` is
//! populated.

// ot: canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
pub mod lockfile_schema;
pub mod rules;

pub const CX_PREFIX: &str = "CX";

pub struct ComplexityBudget;

impl Paradigm for ComplexityBudget {
    fn name(&self) -> &'static str {
        "Complexity Budget Ownership"
    }
    fn rule_prefix(&self) -> &'static str {
        CX_PREFIX
    }
    fn init(&self, _air: &AirWorkspace) -> serde_json::Value {
        // No automatic inference — function budgets come from the user.
        serde_json::Value::Null
    }
    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic> {
        let section: lockfile_schema::CxSection =
            lockfile.paradigm_section(CX_PREFIX).unwrap_or_default();
        let mut diags = rules::cx001(air, &section, mode);
        diags.extend(rules::cx007(air, &section, mode));
        diags.extend(rules::cx008(air, &section, mode));
        diags
    }
}
