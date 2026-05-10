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

// locus: ot canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic, vacant_paradigm_diagnostic};
use crate::init::Suggestion;
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
pub mod init;
pub mod lockfile_schema;
pub mod rules;

pub const DG_PREFIX: &str = "DG";

// locus: allow MO005 — paradigm host struct intentionally lives in mod.rs by convention
pub struct DependencyGraph;

// locus: allow MO005 — paradigm Paradigm impl intentionally lives in mod.rs by convention
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
        // DG002 (cycle detection) is structural — keep it on regardless
        // of vacancy.
        let mut out = rules::dg002(air, mode);
        if section.is_vacant() && !lockfile.is_acknowledged_empty(DG_PREFIX) {
            out.push(vacant_paradigm_diagnostic(
                DG_PREFIX,
                "Dependency Graph / Direction",
                &[
                    ("forbidden_edges", "edges the workspace forbids (DG001)"),
                    (
                        "features",
                        "named feature regions with `public_api` patterns (DG003)",
                    ),
                    (
                        "shared_paths",
                        "module patterns for shared infrastructure (DG004)",
                    ),
                ],
            ));
            return out;
        }
        out.extend(rules::dg001(air, &section, mode));
        out.extend(rules::dg003(air, &section, mode));
        out.extend(rules::dg004(air, &section, mode));
        out
    }

    fn suggest(&self, air: &AirWorkspace, lockfile: &Lockfile) -> Vec<Suggestion> {
        init::suggest(air, lockfile)
    }
}
