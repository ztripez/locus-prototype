//! TA — Test Architecture Ownership.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 19: Test Architecture Ownership".
//! Reads declared test module patterns from `paradigms.TA.test_paths` in
//! `locus.lock` and flags public types defined inside any matching module —
//! tests must not create new domain truth, and a public type in test code is
//! typically a shadow of a domain concept that belongs on the canonical
//! production path.
//!
//! Phase scope so far:
//! - TA001: test module defines a public domain-shaped type.
//! - TA002: test type whose name overlaps an accepted canonical concept.
//! - TA003: test struct whose name and field-set both echo a canonical concept.
//! - TA004: port impl in test code outside accepted test-adapter modules.

// locus: ot canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic, vacant_paradigm_diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
pub mod init;
pub mod lockfile_schema;
pub mod rules;

pub const TA_PREFIX: &str = "TA";

// locus: allow MO005 — paradigm host struct intentionally lives in mod.rs by convention
pub struct TestArchitecture;

// locus: allow MO005 — paradigm Paradigm impl intentionally lives in mod.rs by convention
impl Paradigm for TestArchitecture {
    fn name(&self) -> &'static str {
        "Test Architecture Ownership"
    }
    fn rule_prefix(&self) -> &'static str {
        TA_PREFIX
    }
    fn init(&self, _air: &AirWorkspace) -> serde_json::Value {
        // Test status is a user assertion, not an inference. `init` returns
        // an empty section; the user adds patterns via the TA edit surface
        // (future) or by hand-editing `locus.lock`.
        serde_json::Value::Null
    }
    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic> {
        let section: lockfile_schema::TaSection =
            lockfile.paradigm_section(TA_PREFIX).unwrap_or_default();
        if section.is_vacant() && !lockfile.is_acknowledged_empty(TA_PREFIX) {
            return vec![vacant_paradigm_diagnostic(
                TA_PREFIX,
                "Test Architecture Ownership",
                &[(
                    "test_paths",
                    "module patterns identifying test code (e.g. `*::tests::*`, `tests::*`)",
                )],
            )];
        }
        let mut diags = rules::ta001(air, &section, mode);
        diags.extend(rules::ta002(air, &section, mode));
        diags.extend(rules::ta003(air, &section, mode));
        diags.extend(rules::ta004(air, &section, mode));
        diags
    }
    fn suggest(&self, air: &AirWorkspace, lockfile: &Lockfile) -> Vec<crate::init::Suggestion> {
        init::suggest(air, lockfile)
    }
}
