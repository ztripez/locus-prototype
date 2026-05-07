//! CR — Composition Root Ownership.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 7: Composition Root Ownership".
//!
//! Stub for parallel implementation. Fill in `lockfile_schema.rs` with the
//! section type, `rules.rs` with rule functions, and (optionally) an
//! `edit.rs` for CLI mutators. Wire rule dispatch into `check` when the
//! first rule lands.

// ot: canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

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
        Vec::new()
    }
}
