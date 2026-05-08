//! OB — Observability Ownership.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 18: Observability Ownership". Reads
//! the observer-module / forbidden-log-target split from `paradigms.OB` in
//! `locus.lock` and flags raw `println!` / `dbg!` (and equivalents) called
//! from outside the declared observer modules.
//!
//! Phase scope so far:
//! - OB001: raw print/dbg in non-test, non-observer code.
//! - OB002: metric-emission macro outside the accepted metric owner module.
//! - OB003: event-emission macro outside the accepted event owner module.
//! - OB004: boundary-entry function (carrying a `// ot: marks
//!   boundary_entry` source hint) with no `Logging` fact targeting it.
//!   Opt-in is the marker itself; no lockfile field gates the rule.

// ot: canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
pub mod lockfile_schema;
pub mod rules;

pub const OB_PREFIX: &str = "OB";

pub struct Observability;

impl Paradigm for Observability {
    fn name(&self) -> &'static str {
        "Observability Ownership"
    }
    fn rule_prefix(&self) -> &'static str {
        OB_PREFIX
    }
    fn init(&self, _air: &AirWorkspace) -> serde_json::Value {
        // Observer-module assertions and the forbidden-target list are user
        // policy, not inferences. `init` returns an empty section; the user
        // populates the lockfile fields directly (or via a future
        // `locus ob` mutator) and ObSection's serde defaults fill in the
        // print/dbg baseline on first deserialize.
        serde_json::Value::Null
    }
    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic> {
        let section: lockfile_schema::ObSection =
            lockfile.paradigm_section(OB_PREFIX).unwrap_or_default();
        let mut diags = rules::ob001(air, &section, mode);
        diags.extend(rules::ob002(air, &section, mode));
        diags.extend(rules::ob003(air, &section, mode));
        diags.extend(rules::ob004(air, &section, mode));
        diags
    }
}
