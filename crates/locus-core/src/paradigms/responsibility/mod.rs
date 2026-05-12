//! RM — Responsibility Mixing.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 8: Responsibility Ownership".
//!
//! Reads the per-function distinct-`ActionKind` cap from
//! `paradigms.RM.default_max_action_kinds` in `locus.lock` and flags any
//! function whose `AirTruthAction` body mixes more than that many kinds of
//! work — the "kitchen-sink handler" anti-pattern.
//!
//! Phase scope so far:
//! - RM001: function performs too many distinct kinds of work.
//! - RM002: converter performs a side-effect fact.
//! - RM003: handler module containing branch-rich domain policy.
//! - RM004: repository module containing branch-rich domain logic.
//! - RM005: validator function performing IO (external or persistence).
//! - RM006: domain type method performing persistence-write.

// locus: ot canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
pub mod init;
pub mod lockfile_schema;
pub mod rules;

pub const RM_PREFIX: &str = "RM";

pub struct Responsibility;

impl Paradigm for Responsibility {
    fn name(&self) -> &'static str {
        "Responsibility Mixing"
    }
    fn rule_prefix(&self) -> &'static str {
        RM_PREFIX
    }
    fn init(&self, _air: &AirWorkspace) -> serde_json::Value {
        // Cap is a user assertion, not an inference: the right number depends
        // on architectural style. `init` returns an empty section; the user
        // opts in by setting `default_max_action_kinds` in the lockfile.
        serde_json::Value::Null
    }
    fn check(
        &self,
        _air: &AirWorkspace,
        _lockfile: &Lockfile,
        _mode: CheckMode,
    ) -> Vec<Diagnostic> {
        // All RM rules migrated to RuleDefinition (#71 P4).
        // Detection runs through the governance pipeline; this legacy
        // path is now a no-op.
        Vec::new()
    }
    fn suggest(&self, air: &AirWorkspace, lockfile: &Lockfile) -> Vec<crate::init::Suggestion> {
        init::suggest(air, lockfile)
    }
}
