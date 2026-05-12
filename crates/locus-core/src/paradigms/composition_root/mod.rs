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
use crate::diagnostics::{CheckMode, Diagnostic};
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
    fn check(
        &self,
        _air: &AirWorkspace,
        _lockfile: &Lockfile,
        _mode: CheckMode,
    ) -> Vec<Diagnostic> {
        // All CR rules migrated to RuleDefinition (#71 P4).
        // Detection runs through the governance pipeline; this legacy
        // path is now a no-op.
        Vec::new()
    }
    fn suggest(&self, air: &AirWorkspace, lockfile: &Lockfile) -> Vec<crate::init::Suggestion> {
        init::suggest(air, lockfile)
    }
}
