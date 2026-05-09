//! DA — Demand-Driven Architecture.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 3: Demand-Driven Architecture".
//!
//! Phase scope:
//! - DA001: trait with exactly one implementation and no accepted port role.
//! - DA002: factory function (`create_*`/`make_*`/`build_*`/`*_factory`)
//!   that only ever constructs a single type.
//! - DA007: strategy enum (`*Strategy`/`*Mode`/`*Policy`) with exactly
//!   one variant.
//!
//! `init` returns `Null`: there's no automatic inference for "this trait is
//! a real port" — the user has to declare that intent by toggling
//! `enabled = true` and (optionally) listing accepted single-impl traits in
//! the section. Until then DA001 stays silent, same lockfile-driven UX as
//! DG/MO/CX.

// locus: ot canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic, vacant_paradigm_diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
pub mod lockfile_schema;
pub mod rules;

pub const DA_PREFIX: &str = "DA";

pub struct DemandDriven;

impl Paradigm for DemandDriven {
    fn name(&self) -> &'static str {
        "Demand-Driven Architecture"
    }
    fn rule_prefix(&self) -> &'static str {
        DA_PREFIX
    }
    fn init(&self, _air: &AirWorkspace) -> serde_json::Value {
        // No automatic inference — accepted single-impl traits come from the user.
        serde_json::Value::Null
    }
    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic> {
        let section: lockfile_schema::DaSection =
            lockfile.paradigm_section(DA_PREFIX).unwrap_or_default();
        if section.is_vacant() && !lockfile.is_acknowledged_empty(DA_PREFIX) {
            return vec![vacant_paradigm_diagnostic(
                DA_PREFIX,
                "Demand-Driven Architecture",
                &[(
                    "enabled",
                    "master switch — set to `true` to opt in to DA001/002/007",
                )],
            )];
        }
        let mut diags = rules::da001(air, &section, mode);
        diags.extend(rules::da002(air, &section, mode));
        diags.extend(rules::da007(air, &section, mode));
        diags
    }
}
