//! Shape of the RW section inside `locus.lock`.
//!
//! Rules family RW (Runtime Work Ownership): spawning tasks, threads, jobs,
//! or background work belongs in declared *runtime owner* modules — job
//! queues, orchestrators, supervisors, runtime entry points — not scattered
//! across handlers, services, or feature modules. The lockfile records which
//! module paths are accepted runtime owners (`runtime_owner_paths`); RW001
//! fires on any spawn-shaped action observed outside them.
//!
//! No paths are inferred at `init` time: runtime-owner status is a user
//! assertion, not a guess. An empty `runtime_owner_paths` keeps the rule
//! silent — same lockfile-driven posture as CR/DG/UT.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct RwSection {
    /// Module patterns matching `AirFile.module_path` for files declared as
    /// runtime owners — the only places where direct task/thread spawning is
    /// legitimate. Pattern syntax mirrors CR/DG/UT: simple suffix wildcards
    /// (e.g. `bin::*`, `crate::runtime::*`, `crate::worker::*`,
    /// `crate::orchestrator`).
    #[serde(default)]
    pub runtime_owner_paths: Vec<String>,
}

/// Pattern syntax: simple suffix wildcard, mirroring CR/DG/UT.
/// - `foo::bar` — exact match
/// - `foo::*` — `foo` itself or any descendant (`foo::bar`, `foo::bar::baz`)
/// - `*` — anything
///
/// Duplicated locally so the RW paradigm doesn't depend on CR/DG/UT.
pub fn matches_pattern(pattern: &str, path: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix("::*") {
        return path == prefix || path.starts_with(&format!("{prefix}::"));
    }
    pattern == path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_pattern_exact() {
        assert!(matches_pattern("crate::runtime", "crate::runtime"));
        assert!(!matches_pattern("crate::runtime", "crate::runtime::pool"));
        assert!(!matches_pattern("crate::runtime", "crate"));
    }

    #[test]
    fn matches_pattern_suffix_wildcard_includes_prefix_and_descendants() {
        assert!(matches_pattern("crate::runtime::*", "crate::runtime"));
        assert!(matches_pattern("crate::runtime::*", "crate::runtime::pool"));
        assert!(matches_pattern(
            "crate::runtime::*",
            "crate::runtime::pool::worker"
        ));
        assert!(!matches_pattern("crate::runtime::*", "crate::runtimer"));
        assert!(!matches_pattern("crate::runtime::*", "crate::other"));
    }

    #[test]
    fn matches_pattern_star_matches_anything() {
        assert!(matches_pattern("*", ""));
        assert!(matches_pattern("*", "crate::handler"));
        assert!(matches_pattern("*", "anything::nested::module"));
    }
}
