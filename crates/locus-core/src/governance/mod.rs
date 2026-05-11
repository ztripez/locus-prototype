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

pub mod ids;
pub mod finding;

pub use ids::{FindingId, FindingIdMinter, ParadigmId, PolicyId, RuleId};
pub use finding::{Confidence, Evidence, LegacyEvidence};
