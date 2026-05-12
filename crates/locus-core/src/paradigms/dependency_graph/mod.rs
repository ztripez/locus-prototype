//! DG — Dependency Graph / Direction Ownership.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 4: Dependency Direction Ownership"
//! (numbering may shift as paradigms get added). Reads imports from AIR
//! (`AirItem::Import`) and matches them against `forbidden_edges` in the
//! lockfile's DG section.
//!
//! All DG rules (DG001–DG004) are now migrated to `RuleDefinition` in the
//! governance spine (#71). The legacy `check()` path returns empty.

// locus: ot canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic};
use crate::init::Suggestion;
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
pub mod init;
pub mod lockfile_schema;
pub mod rules;

pub const DG_PREFIX: &str = "DG";

pub struct DependencyGraph;

impl Paradigm for DependencyGraph {
    fn name(&self) -> &'static str {
        "Dependency Graph / Direction"
    }
    fn rule_prefix(&self) -> &'static str {
        DG_PREFIX
    }

    fn init(&self, _air: &AirWorkspace) -> serde_json::Value {
        // No automatic inference — direction declarations come from the user.
        serde_json::Value::Null
    }

    fn check(
        &self,
        _air: &AirWorkspace,
        _lockfile: &Lockfile,
        _mode: CheckMode,
    ) -> Vec<Diagnostic> {
        // All DG rules (DG001–DG004) are now driven through the governance
        // spine RuleDefinition pipeline. Nothing left to run here.
        Vec::new()
    }

    fn suggest(&self, air: &AirWorkspace, lockfile: &Lockfile) -> Vec<Suggestion> {
        init::suggest(air, lockfile)
    }
}
