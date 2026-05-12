//! PA — Port/Adapter Ownership.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 6: Port/Adapter Ownership".
//!
//! Phase-2 scope:
//! - PA001: trait declared and immediately implemented in the same file
//!   (port and adapter co-located — physical separation never happened).
//! - PA002: application/domain file imports a concrete adapter framework.
//! - PA003: application function performs external IO without going
//!   through a declared port.
//! - PA004: adapter type constructed outside any composition root /
//!   bootstrap / composition module.
//!
//! `init` returns an empty section: there's nothing to infer up front. The
//! lockfile starts empty so PA001 fires on every co-located trait+impl pair;
//! the user reviews each one and either splits the port from its adapter
//! (the canonical fix) or accepts the trait as a non-port utility helper via
//! `accepted_colocated_traits`.

// locus: ot canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic, vacant_paradigm_diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
pub mod lockfile_schema;
pub mod rules;

pub const PA_PREFIX: &str = "PA";

pub struct PortAdapter;

impl Paradigm for PortAdapter {
    fn name(&self) -> &'static str {
        "Port/Adapter Ownership"
    }
    fn rule_prefix(&self) -> &'static str {
        PA_PREFIX
    }
    fn init(&self, _air: &AirWorkspace) -> serde_json::Value {
        // No automatic inference — port/adapter exemptions come from review.
        serde_json::Value::Null
    }
    fn check(&self, _air: &AirWorkspace, lockfile: &Lockfile, _mode: CheckMode) -> Vec<Diagnostic> {
        // All PA rules migrated to RuleDefinition (#71 P4); only the LOCUS002
        // vacancy nudge remains here so vacant-by-definition paradigms keep
        // surfacing onboarding guidance.
        let section: lockfile_schema::PaSection =
            lockfile.paradigm_section(PA_PREFIX).unwrap_or_default();
        if section.is_vacant() && !lockfile.is_acknowledged_empty(PA_PREFIX) {
            return vec![vacant_paradigm_diagnostic(
                PA_PREFIX,
                "Port/Adapter Ownership",
                &[
                    (
                        "application_paths",
                        "module patterns identifying application/domain code",
                    ),
                    (
                        "concrete_adapter_patterns",
                        "import paths application/domain code must not reach",
                    ),
                    (
                        "adapter_type_patterns",
                        "type patterns identifying concrete adapters",
                    ),
                ],
            )];
        }
        Vec::new()
    }
}
