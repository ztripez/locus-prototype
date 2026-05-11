//! CX — Complexity Budget Ownership.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 10: Complexity Budget Ownership".
//!
//! Reads `AirItem::Function` items from each file and compares each
//! function's `line_count` against a per-module budget held in the
//! lockfile's CX section. CX001 flags long functions (default 50);
//! CX002 flags long files (default 400); CX007 caps the per-file
//! public-API surface (default 30); CX008 caps the per-function call-site
//! fan-out outside an accepted orchestration module.
//!
//! `init` returns `Null`: there's no automatic inference for "this
//! function is allowed to be long" — the user has to declare the
//! override deliberately. CX is **noisy by default**: built-in fallback
//! budgets fire on un-onboarded code (CX001/CX002/CX007 fire immediately
//! with their defaults); CX008 stays vacant-on-empty because deciding
//! where high fan-out is legitimate is a deliberate user act. Users
//! widen budgets via `paradigms.CX.overrides` / `module_overrides`,
//! `default_max_function_lines`, `default_max_module_lines`,
//! `max_public_items`, `exempt_paths`, or silence the paradigm wholesale
//! by adding `"CX"` to `Lockfile.acknowledged_empty`.

// locus: ot canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
pub mod init;
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
        // CX001 migrated to RuleDefinition (#71 P2). The governance
        // pipeline runs it via Cx001Rule::observe; the legacy adapter's
        // per-rule-code filter drops any CX001 diagnostic that would be
        // emitted here, but we don't even compute it.
        let mut diags = rules::cx002(air, &section, mode);
        diags.extend(rules::cx007(air, &section, mode));
        diags.extend(rules::cx008(air, &section, mode));
        diags
    }
    fn suggest(&self, air: &AirWorkspace, lockfile: &Lockfile) -> Vec<crate::init::Suggestion> {
        init::suggest(air, lockfile)
    }
}
