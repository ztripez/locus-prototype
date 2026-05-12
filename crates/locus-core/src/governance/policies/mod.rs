//! Policy implementations.
//!
//! `default` — `DefaultPassThroughPolicy` (always last in the policy
//! chain; decides every finding not already decided).
//!
//! `registry_integrity` — `RegistryIntegrityPolicy` (runs before
//! pass-through; emits LOCUS003 migration-debt advisories).
//!
//! `registry_coherence` — `RegistryCoherencePolicy` (runs after
//! registry_integrity; emits LOCUS004 architecture-drift advisories).
//!
//! `concept_sot` — `ConceptSourceOfTruthPolicy` (runs after
//! registry_coherence; emits LOCUS005 architecture-intent violations
//! for bypasses of declared source-of-truth paths).

// locus: ot canonical

pub mod concept_sot;
pub mod default;
pub mod registry_coherence;
pub mod registry_integrity;

pub use concept_sot::ConceptSourceOfTruthPolicy;
pub use default::DefaultPassThroughPolicy;
pub use registry_coherence::RegistryCoherencePolicy;
pub use registry_integrity::RegistryIntegrityPolicy;
