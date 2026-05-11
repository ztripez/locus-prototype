//! Governance spine (epic #71).
//!
//! Replaces the legacy `Paradigm::check() -> Vec<Diagnostic>` path with a
//! `rules → findings → policies → decisions → diagnostics` pipeline. This
//! module is the strangler boundary: legacy paradigm output is wrapped
//! into synthetic findings by `legacy::LegacyParadigmRuleAdapter`, run
//! through the policy chain, and materialized back into `Diagnostic`s
//! that are byte-identical to the prior implementation under
//! `DefaultPassThroughPolicy`.
//!
//! Spec: `docs/superpowers/specs/2026-05-11-governance-spine-design.md`.

// locus: ot canonical

pub mod decision;
pub mod finding;
pub mod ids;
pub mod legacy;
pub mod paradigm;
mod paradigm_impls;
pub mod policy;
pub mod registry;
pub mod rule;

pub use decision::{Decision, DecisionStatus, SeverityChange};
pub use finding::{
    Confidence, Evidence, FindingSource, FindingStore, LegacyEvidence, RuleFinding,
};
pub use ids::{FindingId, FindingIdMinter, ParadigmId, PolicyId, RuleId};
pub use legacy::LegacyParadigmRuleAdapter;
pub use paradigm::ParadigmDefinition;
pub use policy::{PolicyContext, PolicyDefinition, PolicyOutput};
pub use registry::{
    GovernanceDiagnosticRegistry, ParadigmRegistry, PolicyRegistry, RegistryError, RuleRegistry,
    validate_decisions,
};
pub use rule::{RuleContext, RuleDefinition};
