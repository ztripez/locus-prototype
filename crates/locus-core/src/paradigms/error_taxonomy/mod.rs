//! ER — Error Taxonomy Ownership.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 13: Error Taxonomy Ownership".
//!
//! Phase scope:
//! - ER001: multiple public error types in the same module (heuristic
//!   warning that flags taxonomy forks like `UserError` + `CreateUserError`
//!   sitting next to each other in one file).
//! - ER002: a `Result<_, E>` return whose `E` matches a forbidden
//!   "string-shaped" / catch-all pattern. Lockfile-driven via
//!   [`lockfile_schema::ErSection::forbidden_error_types`]; silent until
//!   that list is populated.
//! - ER003: a domain enum embeds a boundary error type as a variant field.
//!   Lockfile-driven via [`lockfile_schema::ErSection::domain_paths`] plus
//!   [`lockfile_schema::ErSection::boundary_error_patterns`]; silent until
//!   both are populated.
//! - ER005: catch-all `Err(_)` arm body collapsing distinct errors into a
//!   single value (taxonomy-collapse view of the same shape FL007 sees).
//! - ER007: a variant name appears on two or more `*Error*` enums in the
//!   workspace (taxonomy drift). Heuristic and lockfile-free.

// ot: canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic, vacant_paradigm_diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
pub mod lockfile_schema;
pub mod rules;

pub const ER_PREFIX: &str = "ER";

pub struct ErrorTaxonomy;

impl Paradigm for ErrorTaxonomy {
    fn name(&self) -> &'static str {
        "Error Taxonomy Ownership"
    }
    fn rule_prefix(&self) -> &'static str {
        ER_PREFIX
    }
    fn init(&self, _air: &AirWorkspace) -> serde_json::Value {
        // No automatic inference yet — ER001 is heuristic and lockfile-free.
        // ER002+ may populate accepted-error entries here.
        serde_json::Value::Null
    }
    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic> {
        let section: lockfile_schema::ErSection =
            lockfile.paradigm_section(ER_PREFIX).unwrap_or_default();
        // ER001 and ER007 are heuristic and lockfile-free — keep them on
        // even when the rest of the paradigm is vacant.
        let mut diags = rules::er001(air, &section, mode);
        diags.extend(rules::er007(air, mode));
        if section.is_vacant() && !lockfile.is_acknowledged_empty(ER_PREFIX) {
            diags.push(vacant_paradigm_diagnostic(
                ER_PREFIX,
                "Error Taxonomy Ownership",
                &[
                    (
                        "forbidden_error_types",
                        "patterns matching catch-all error shapes forbidden as `Result<_, E>`",
                    ),
                    (
                        "domain_paths",
                        "module patterns identifying domain code (ER003)",
                    ),
                    (
                        "boundary_error_patterns",
                        "patterns matching boundary error types (ER003)",
                    ),
                    (
                        "error_collapse_owner_paths",
                        "module patterns where catch-all `Err(_) => default` is legitimate (ER005)",
                    ),
                ],
            ));
            return diags;
        }
        diags.extend(rules::er002(air, &section, mode));
        diags.extend(rules::er003(air, &section, mode));
        diags.extend(rules::er005(air, &section, mode));
        diags
    }
}
