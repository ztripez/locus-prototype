//! Identity newtypes for the governance pipeline.
//!
//! All four are deliberately small and `const`-constructible so static
//! registries can use them. `FindingIdMinter` produces deterministic
//! sequential IDs — stable output for a fixed input/registry/policy
//! order, which matters for golden snapshots and (future) SARIF.

// locus: ot canonical

use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};

// Static-only IDs: `&'static str` storage with `const fn new` so static
// registries work. `Serialize` is derived (we emit IDs as strings in
// JSON/SARIF); `Deserialize` is intentionally NOT derived — IDs are only
// constructed from string literals at compile time. Re-add when a future
// epic needs to round-trip findings through serialized form (would
// switch the storage to `Cow<'static, str>` and lose `Copy`).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct RuleId(&'static str);

impl RuleId {
    pub const fn new(s: &'static str) -> Self {
        Self(s)
    }
    pub fn as_str(&self) -> &'static str {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct ParadigmId(&'static str);

impl ParadigmId {
    pub const fn new(s: &'static str) -> Self {
        Self(s)
    }
    pub fn as_str(&self) -> &'static str {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct PolicyId(&'static str);

impl PolicyId {
    pub const fn new(s: &'static str) -> Self {
        Self(s)
    }
    pub fn as_str(&self) -> &'static str {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, serde::Deserialize)]
pub struct FindingId(u64);

impl FindingId {
    pub fn as_u64(&self) -> u64 {
        self.0
    }

    /// Test-only constructor. Production code must use FindingIdMinter so
    /// IDs are deterministic and unique across a run.
    #[cfg(test)]
    pub fn from_raw_for_test(raw: u64) -> Self {
        Self(raw)
    }
}

/// Deterministic counter. Single-threaded use in pipeline::run; the atomic
/// is for safety, not concurrency — we never share the minter across
/// threads in MVP.
#[derive(Debug)]
pub struct FindingIdMinter {
    next: AtomicU64,
}

impl FindingIdMinter {
    pub fn new() -> Self {
        Self { next: AtomicU64::new(0) }
    }

    pub fn next(&self) -> FindingId {
        FindingId(self.next.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for FindingIdMinter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_through_as_str() {
        assert_eq!(RuleId::new("CX001").as_str(), "CX001");
        assert_eq!(ParadigmId::new("CX").as_str(), "CX");
        assert_eq!(PolicyId::new("default-pass-through").as_str(), "default-pass-through");
    }

    #[test]
    fn ids_compare_by_value() {
        assert_eq!(RuleId::new("CX001"), RuleId::new("CX001"));
        assert_ne!(RuleId::new("CX001"), RuleId::new("CX002"));
    }

    #[test]
    fn finding_id_minter_is_sequential_and_deterministic() {
        let m = FindingIdMinter::new();
        let a = m.next();
        let b = m.next();
        let c = m.next();
        assert_eq!(a.as_u64(), 0);
        assert_eq!(b.as_u64(), 1);
        assert_eq!(c.as_u64(), 2);
        // Two fresh minters produce identical sequences for the same call count.
        let m2 = FindingIdMinter::new();
        assert_eq!(m2.next().as_u64(), 0);
    }

    #[test]
    fn ids_serde_round_trip() {
        let id = RuleId::new("OT002");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"OT002\"");
    }
}
