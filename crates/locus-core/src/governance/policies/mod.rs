//! Policy implementations.
//!
//! `default` — `DefaultPassThroughPolicy` (always last in the policy
//! chain; decides every finding not already decided).
//!
//! Future policies (RegistryIntegrityPolicy in P3, ExceptionPolicy / etc.
//! in future epics) live alongside `default` here.

// locus: ot canonical

pub mod default;

pub use default::DefaultPassThroughPolicy;
