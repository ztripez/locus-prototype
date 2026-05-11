//! Policy implementations.
//!
//! `default` — `DefaultPassThroughPolicy` (always last in the policy
//! chain; decides every finding not already decided).
//!
//! `registry_integrity` — `RegistryIntegrityPolicy` (runs before
//! pass-through; emits LOCUS003 migration-debt advisories).

// locus: ot canonical

pub mod default;
pub mod registry_integrity;

pub use default::DefaultPassThroughPolicy;
pub use registry_integrity::RegistryIntegrityPolicy;
