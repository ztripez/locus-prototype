//! FO — Feature Ownership.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 15: Feature Ownership".
//!
//! Phase scope:
//! - FO001: same concept defined in two different features.
//! - FO004: a type defined inside a `shared_paths` module has a field whose
//!   `type_text` references a declared feature's module — i.e. shared
//!   structures secretly knowing about feature internals.
//!
//! FO is conceptually adjacent to DG003 — DG003 forbids feature A *reaching
//! into* feature B's internals through imports; FO001 forbids feature A and
//! feature B both *defining* the same public type name. The two rules use
//! similar feature-definition shapes (`name` + `module` pattern) but each
//! paradigm owns its own copy of that shape so paradigms don't depend on
//! each other.

// ot: canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic, vacant_paradigm_diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
pub mod lockfile_schema;
pub mod rules;

pub const FO_PREFIX: &str = "FO";

pub struct FeatureOwnership;

impl Paradigm for FeatureOwnership {
    fn name(&self) -> &'static str {
        "Feature Ownership"
    }
    fn rule_prefix(&self) -> &'static str {
        FO_PREFIX
    }
    fn init(&self, _air: &AirWorkspace) -> serde_json::Value {
        // No automatic inference — feature regions are user-declared.
        serde_json::Value::Null
    }
    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic> {
        let section: lockfile_schema::FoSection =
            lockfile.paradigm_section(FO_PREFIX).unwrap_or_default();
        if section.is_vacant() && !lockfile.is_acknowledged_empty(FO_PREFIX) {
            return vec![vacant_paradigm_diagnostic(
                FO_PREFIX,
                "Feature Ownership",
                &[
                    (
                        "features",
                        "named feature regions (`name` + `module` pattern)",
                    ),
                    (
                        "shared_paths",
                        "module patterns identifying shared cross-feature regions (FO004)",
                    ),
                ],
            )];
        }
        let mut diags = rules::fo001(air, &section, mode);
        diags.extend(rules::fo004(air, &section, mode));
        diags
    }
}
