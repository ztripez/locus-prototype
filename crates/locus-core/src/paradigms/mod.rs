//! Paradigm modules.
//!
//! Each paradigm lives in its own submodule and implements the [`Paradigm`]
//! trait. Paradigms share `Diagnostic` / `Lockfile` infrastructure but never
//! depend on each other.

pub mod one_truth;

use crate::diagnostics::{CheckMode, Diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

// ot: canonical
pub trait Paradigm {
    /// Human-facing paradigm name, e.g. `"Canonical Domain Ownership"`.
    fn name(&self) -> &'static str;
    /// Two-letter rule prefix, e.g. `"OT"` for One Truth.
    fn rule_prefix(&self) -> &'static str;
    /// Build this paradigm's lockfile section from a fresh AIR scan. Called
    /// by `locus init`. The returned JSON is stored verbatim under
    /// `paradigms.<prefix>` in `locus.lock`.
    fn init(&self, air: &AirWorkspace) -> serde_json::Value;
    /// Run all of this paradigm's rules against `air` and return diagnostics.
    /// Implementations should consult `lockfile.paradigm_section(self.rule_prefix())`
    /// for accepted ownership; missing sections mean "nothing accepted yet,"
    /// which is normal before `locus init` has been run.
    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic>;
}

/// All paradigms wired into this build of Locus. Phase 2 ships OT only; later
/// phases extend the slice as `dependency_graph::DependencyGraph`,
/// `boundary_ownership::BoundaryOwnership`, etc. land.
pub fn registry() -> Vec<Box<dyn Paradigm>> {
    vec![Box::new(one_truth::OneTruth)]
}
