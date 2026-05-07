//! Shape of the FL section inside `locus.lock`.
//!
//! Rules family FL (Failure Lineage Ownership): the lockfile records two
//! lists of patterns that together describe where transport-level failures
//! must not leak.
//!
//! - `domain_paths` — module patterns marking files whose function signatures
//!   must speak the domain's error vocabulary.
//! - `boundary_error_patterns` — patterns matching error type names that are
//!   transport / boundary level (e.g. `reqwest::Error`, `sqlx::Error`,
//!   `std::io::Error`). Encountering one of these as the `E` of a
//!   `Result<T, E>` returned from a domain-path function is a structural
//!   failure-lineage violation: the boundary error escaped without being
//!   wrapped in a domain error type.
//!
//! Both lists default to empty and FL001 stays silent until the user has
//! onboarded their codebase — same UX shape as DG / UT lockfile-driven rules.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct FlSection {
    /// Module patterns matching `AirFile.module_path` for files declared as
    /// "domain" — i.e. files whose function signatures must not leak boundary
    /// error types. Pattern syntax mirrors UT/DG: simple suffix wildcards.
    #[serde(default)]
    pub domain_paths: Vec<String>,

    /// Patterns matching the `E` in a function's `Result<T, E>` return type
    /// when E is a transport / boundary error and therefore must not appear
    /// in a domain function signature. Pattern syntax mirrors `domain_paths`.
    #[serde(default)]
    pub boundary_error_patterns: Vec<String>,
}

/// Pattern syntax: simple suffix wildcard, mirroring DG / UT.
/// - `foo::bar` — exact match
/// - `foo::*` — `foo` itself or any descendant (`foo::bar`, `foo::bar::baz`)
/// - `*` — anything
///
/// Duplicated locally (rather than imported from UT) so the FL paradigm slice
/// has no implicit dependency on a sibling paradigm. If the matcher ever
/// needs to grow (e.g. mid-segment wildcards), each paradigm can evolve
/// independently.
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
    fn exact_match() {
        assert!(matches_pattern("foo::bar", "foo::bar"));
        assert!(!matches_pattern("foo::bar", "foo::bar::baz"));
        assert!(!matches_pattern("foo::bar", "foo"));
    }

    #[test]
    fn suffix_wildcard_includes_the_prefix_and_descendants() {
        assert!(matches_pattern("foo::*", "foo"));
        assert!(matches_pattern("foo::*", "foo::bar"));
        assert!(matches_pattern("foo::*", "foo::bar::baz"));
        assert!(!matches_pattern("foo::*", "foobar"));
        assert!(!matches_pattern("foo::*", "bar"));
    }

    #[test]
    fn star_matches_anything() {
        assert!(matches_pattern("*", ""));
        assert!(matches_pattern("*", "anything"));
        assert!(matches_pattern("*", "anything::nested"));
    }

    #[test]
    fn boundary_error_patterns_can_target_type_paths() {
        // Pattern syntax is path-shaped, not Rust-namespaced — the rule will
        // run it against the rendered error type string from
        // `AirFunction.return_type`.
        assert!(matches_pattern("reqwest::Error", "reqwest::Error"));
        assert!(matches_pattern("reqwest::*", "reqwest::Error"));
        assert!(matches_pattern(
            "reqwest::*",
            "reqwest::header::InvalidHeader"
        ));
        assert!(!matches_pattern("reqwest::*", "sqlx::Error"));
    }
}
