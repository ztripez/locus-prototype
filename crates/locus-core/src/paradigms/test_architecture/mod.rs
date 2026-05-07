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

// ot: canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
pub mod lockfile_schema;
pub mod rules;

pub const TA_PREFIX: &str = "TA";

pub struct TestArchitecture;

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
        rules::ta001(air, &section, mode)
    }
}
