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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

    /// Callee names considered "panic-shaped" — i.e. they mask a missing
    /// invariant rather than propagate a structured error. FL002 matches
    /// each `AirItem::CallSite.callee` (last `::` segment for path-qualified
    /// macros) against these patterns. Default covers the standard
    /// agent-introduced "make it compile" family: `unwrap`, `expect`,
    /// `unwrap_or_default`, `panic`, `todo`, `unimplemented`. The user can
    /// tighten or loosen via the lockfile.
    #[serde(default = "default_forbidden_callees")]
    pub forbidden_callees: Vec<String>,

    /// Module patterns matching `AirFile.module_path` for files where the
    /// panic-shaped callees above are legitimate — typically supervisors,
    /// startup-asserting bin entry points, or test-support modules that
    /// own the invariant being asserted. FL002 stays silent until this list
    /// is populated, mirroring every other lockfile-driven rule.
    ///
    /// The spec (`docs/PARADIGMS.md` line 811: "panics/unwraps outside
    /// invariant owners *or tests*") expects test paths to be carved out.
    /// We can't auto-detect `#[cfg(test)]` from AIR — `AirFunction` /
    /// `AirFile` don't carry attribute state — so test-path patterns are a
    /// user lockfile decision. Recommended starter set when populating:
    /// `["*::tests::*", "*::test::*", "tests::*", "tests::*::*"]` plus any
    /// project-specific invariant-owner modules. We deliberately don't seed
    /// these defaults here because a non-empty seed would flip FL002 from
    /// "silent until configured" to "fires on every codebase" — a posture
    /// the rest of Locus avoids.
    #[serde(default)]
    pub invariant_owner_paths: Vec<String>,
}

impl Default for FlSection {
    fn default() -> Self {
        Self {
            domain_paths: Vec::new(),
            boundary_error_patterns: Vec::new(),
            forbidden_callees: default_forbidden_callees(),
            invariant_owner_paths: Vec::new(),
        }
    }
}

/// Default forbidden callees for FL002: the standard agent-introduced
/// "make it compile by unwrapping" family. Matched against
/// `AirCallSite.callee` (last `::` segment for path-qualified macros), so
/// these are bare names — no `std::` prefix.
pub fn default_forbidden_callees() -> Vec<String> {
    vec![
        "unwrap".to_string(),
        "expect".to_string(),
        "unwrap_or_default".to_string(),
        "panic".to_string(),
        "todo".to_string(),
        "unimplemented".to_string(),
    ]
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
    fn default_section_seeds_forbidden_callees_and_keeps_owner_paths_empty() {
        let s = FlSection::default();
        assert!(s.domain_paths.is_empty());
        assert!(s.boundary_error_patterns.is_empty());
        assert!(s.invariant_owner_paths.is_empty());
        for expected in [
            "unwrap",
            "expect",
            "unwrap_or_default",
            "panic",
            "todo",
            "unimplemented",
        ] {
            assert!(
                s.forbidden_callees.iter().any(|c| c == expected),
                "default forbidden callees missing `{expected}`: {:?}",
                s.forbidden_callees,
            );
        }
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
