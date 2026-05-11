//! Paradigm modules.
//!
//! Each paradigm lives in its own submodule and implements the [`Paradigm`]
//! trait. Paradigms share `Diagnostic` / `Lockfile` infrastructure but never
//! depend on each other.

pub mod abstraction_discipline;
pub mod boundary_ownership;
pub mod claim_ownership;
pub mod complexity_budget;
pub mod composition_root;
pub mod config_data;
pub mod demand_driven;
pub mod dependency_graph;
pub mod documentation;
pub mod error_taxonomy;
pub mod failure_lineage;
pub mod feature_ownership;
pub mod module_ownership;
pub mod observability;
pub mod one_truth;
pub mod port_adapter;
pub mod responsibility;
pub mod runtime_work;
pub mod test_architecture;
pub mod utility_discipline;

use crate::diagnostics::{CheckMode, Diagnostic};
use crate::init::Suggestion;
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

// locus: ot canonical
// locus: allow MO005 — Paradigm trait is the module registry interface, intentionally lives in this mod.rs
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
    /// Emit init-time onboarding suggestions for this paradigm. Default
    /// returns no suggestions; paradigms override to propose layer paths,
    /// concept clusters, threshold dial-ins, or vacancy nudges.
    fn suggest(&self, _air: &AirWorkspace, _lockfile: &Lockfile) -> Vec<Suggestion> {
        Vec::new()
    }
}

/// All paradigms wired into this build of Locus. Order: OT, DG, then
/// alphabetical-by-prefix (AB, BO, CF, CL, CR, CX, DA, DC, ER, FL, FO,
/// MO, OB, PA, RM, RW, TA, UT) — preserves existing test expectations
/// that key off OT/DG.
// locus: allow MO005 — registry() is composition glue by definition: it wires paradigm singletons into the runtime
pub fn registry() -> Vec<Box<dyn Paradigm>> {
    vec![
        Box::new(one_truth::OneTruth),
        Box::new(dependency_graph::DependencyGraph),
        Box::new(abstraction_discipline::AbstractionDiscipline),
        Box::new(boundary_ownership::BoundaryOwnership),
        Box::new(config_data::ConfigData),
        Box::new(claim_ownership::ClaimOwnership),
        Box::new(composition_root::CompositionRoot),
        Box::new(complexity_budget::ComplexityBudget),
        Box::new(demand_driven::DemandDriven),
        Box::new(documentation::Documentation),
        Box::new(error_taxonomy::ErrorTaxonomy),
        Box::new(failure_lineage::FailureLineage),
        Box::new(feature_ownership::FeatureOwnership),
        Box::new(module_ownership::ModuleOwnership),
        Box::new(observability::Observability),
        Box::new(port_adapter::PortAdapter),
        Box::new(responsibility::Responsibility),
        Box::new(runtime_work::RuntimeWork),
        Box::new(test_architecture::TestArchitecture),
        Box::new(utility_discipline::UtilityDiscipline),
    ]
}

#[cfg(test)]
mod suggest_default_tests {
    use super::*;
    use locus_air::AirWorkspace;

    // locus: allow MO005 — test-only Stub is the minimal Paradigm fixture; no better home in this test
    struct Stub;
    // locus: allow MO005 — test-only Paradigm impl for Stub
    impl Paradigm for Stub {
        fn name(&self) -> &'static str {
            "Stub"
        }
        fn rule_prefix(&self) -> &'static str {
            "ZZ"
        }
        fn init(&self, _: &AirWorkspace) -> serde_json::Value {
            serde_json::Value::Null
        }
        fn check(&self, _: &AirWorkspace, _: &Lockfile, _: CheckMode) -> Vec<Diagnostic> {
            Vec::new()
        }
    }

    #[test]
    // locus: allow MO005 — test function in test-only inline mod; no better home than this mod.rs
    fn default_suggest_returns_empty() {
        let p = Stub;
        let air = AirWorkspace::new(Vec::new());
        let lf = Lockfile::empty();
        assert!(p.suggest(&air, &lf).is_empty());
    }
}
