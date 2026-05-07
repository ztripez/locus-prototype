//! MO — Module / File Ownership.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 9: Module / File Ownership".
//!
//! Reads `AirItem::Type` items from each file and counts the `Public`
//! ones, then compares against a per-module budget held in the lockfile's
//! MO section. The first MO rule (`MO001`) flags files whose public-type
//! count exceeds the configured budget.
//!
//! `init` returns `Null`: there's no automatic inference for "this module
//! is allowed to be wide" — the user has to declare the override (or the
//! default) deliberately, same as DG. Without an MO section, MO001 stays
//! silent so un-onboarded code isn't bombarded with file-shape warnings.

// ot: canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
pub mod lockfile_schema;
pub mod rules;

pub const MO_PREFIX: &str = "MO";

pub struct ModuleOwnership;

impl Paradigm for ModuleOwnership {
    fn name(&self) -> &'static str {
        "Module / File Ownership"
    }
    fn rule_prefix(&self) -> &'static str {
        MO_PREFIX
    }
    fn init(&self, _air: &AirWorkspace) -> serde_json::Value {
        // No automatic inference — module budgets come from the user.
        serde_json::Value::Null
    }
    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic> {
        let section: lockfile_schema::MoSection =
            lockfile.paradigm_section(MO_PREFIX).unwrap_or_default();
        rules::mo001(air, &section, mode)
    }
}
