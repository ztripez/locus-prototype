//! CR — Composition Root Ownership.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 7: Composition Root Ownership".
//!
//! Phase scope:
//! - CR001: service-shaped construction outside any declared composition
//!   root.
//! - CR002: high-density wiring — a single function inside a composition
//!   root constructs more services than `wiring_density_threshold` (default
//!   12).
//!
//! `init` returns an empty section: composition-root locations are a user
//! declaration, not an inference. The rule stays silent until the user
//! populates `composition_root_paths`.

// locus: ot canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic, vacant_paradigm_diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
pub mod init;
pub mod lockfile_schema;
pub mod rules;

pub const CR_PREFIX: &str = "CR";

pub struct CompositionRoot;

impl Paradigm for CompositionRoot {
    fn name(&self) -> &'static str {
        "Composition Root Ownership"
    }
    fn rule_prefix(&self) -> &'static str {
        CR_PREFIX
    }
    fn init(&self, _air: &AirWorkspace) -> serde_json::Value {
        serde_json::Value::Null
    }
    fn check(&self, _air: &AirWorkspace, lockfile: &Lockfile, _mode: CheckMode) -> Vec<Diagnostic> {
        // All CR rules migrated to RuleDefinition (#71 P4); only the LOCUS002
        // vacancy nudge remains here so vacant-by-definition paradigms keep
        // surfacing onboarding guidance.
        let section: lockfile_schema::CrSection =
            lockfile.paradigm_section(CR_PREFIX).unwrap_or_default();
        if section.is_vacant() && !lockfile.is_acknowledged_empty(CR_PREFIX) {
            return vec![vacant_paradigm_diagnostic(
                CR_PREFIX,
                "Composition Root Ownership",
                &[(
                    "composition_root_paths",
                    "module patterns identifying composition roots / bootstrap modules",
                )],
            )];
        }
        Vec::new()
    }
    fn suggest(&self, air: &AirWorkspace, lockfile: &Lockfile) -> Vec<crate::init::Suggestion> {
        init::suggest(air, lockfile)
    }
}
