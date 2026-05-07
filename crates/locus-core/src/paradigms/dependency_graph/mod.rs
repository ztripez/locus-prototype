//! DG — Dependency Graph / Direction Ownership.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 4: Dependency Direction Ownership"
//! (numbering may shift as paradigms get added). Reads imports from AIR
//! (`AirItem::Import`) and matches them against `forbidden_edges` in the
//! lockfile's DG section.
//!
//! Phase-2 scope:
//! - DG001: forbidden import.
//!
//! `init` returns an empty section: there's no inference that can decide
//! "domain shouldn't reach api" for a project — the user has to declare that
//! intent. A future `locus dg suggest` (or similar) could enumerate the
//! current import graph as a starting point, but that's report territory,
//! not lockfile-mutation territory.

// ot: canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
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

    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic> {
        let section: lockfile_schema::DgSection =
            lockfile.paradigm_section(DG_PREFIX).unwrap_or_default();
        let mut out = rules::dg001(air, &section, mode);
        out.extend(rules::dg002(air, mode));
        out
    }
}
