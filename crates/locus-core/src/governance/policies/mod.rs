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

// locus: ot canonical

pub mod default;
pub mod registry_coherence;
pub mod registry_integrity;

pub use default::DefaultPassThroughPolicy;
pub use registry_coherence::RegistryCoherencePolicy;
pub use registry_integrity::RegistryIntegrityPolicy;
