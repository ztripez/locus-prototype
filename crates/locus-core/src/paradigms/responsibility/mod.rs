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

// ot: canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic, vacant_paradigm_diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
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
    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic> {
        let section: lockfile_schema::RmSection =
            lockfile.paradigm_section(RM_PREFIX).unwrap_or_default();
        if section.is_vacant() && !lockfile.is_acknowledged_empty(RM_PREFIX) {
            return vec![vacant_paradigm_diagnostic(
                RM_PREFIX,
                "Responsibility Mixing",
                &[
                    (
                        "default_max_action_kinds",
                        "per-function cap on distinct action kinds (RM001)",
                    ),
                    (
                        "converter_paths",
                        "module patterns for converter modules (RM002)",
                    ),
                    (
                        "handler_paths",
                        "module patterns for orchestration handlers (RM003)",
                    ),
                    (
                        "repository_paths",
                        "module patterns for repository modules (RM004)",
                    ),
                    (
                        "validator_paths",
                        "module patterns for validator modules (RM005)",
                    ),
                    (
                        "domain_paths_rm",
                        "module patterns for domain types (RM006)",
                    ),
                ],
            )];
        }
        let mut diags = rules::rm001(air, &section, mode);
        diags.extend(rules::rm002(air, &section, mode));
        diags.extend(rules::rm003(air, &section, mode));
        diags.extend(rules::rm004(air, &section, mode));
        diags.extend(rules::rm005(air, &section, mode));
        diags.extend(rules::rm006(air, &section, mode));
        diags
    }
}
