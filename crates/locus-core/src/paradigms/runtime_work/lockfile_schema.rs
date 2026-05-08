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

/// Pattern syntax: segment-aligned wildcards.
/// - `foo::bar` — exact match
/// - `foo::*` — `foo` itself or any descendant (`foo::bar`, `foo::bar::baz`)
/// - `*::foo` — `foo` itself or anywhere ending with `::foo` (`a::foo`,
///   `a::b::foo`)
/// - `*::foo::*` — `foo` appearing as any whole segment in the path
///   (`foo`, `a::foo`, `a::foo::b`, `a::b::foo::c`)
/// - `*` — anything
///
/// Duplicated locally rather than imported from a sibling paradigm so
/// each paradigm's matcher can evolve independently.
pub fn matches_pattern(pattern: &str, path: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let leading_wild = pattern.starts_with("*::");
    let trailing_wild = pattern.ends_with("::*");
    let stripped = match (leading_wild, trailing_wild) {
        (true, true) => &pattern[3..pattern.len() - 3],
        (true, false) => &pattern[3..],
        (false, true) => &pattern[..pattern.len() - 3],
        (false, false) => pattern,
    };
    if stripped.is_empty() {
        // Pattern was just `*::` or `::*` — treat as a malformed
        // wildcard rather than matching anything; callers configuring
        // these would have meant `*`.
        return false;
    }
    match (leading_wild, trailing_wild) {
        (true, true) => {
            let mid = format!("::{stripped}::");
            let starts = format!("{stripped}::");
            let ends = format!("::{stripped}");
            path == stripped
                || path.contains(&mid)
                || path.starts_with(&starts)
                || path.ends_with(&ends)
        }
        (true, false) => path == stripped || path.ends_with(&format!("::{stripped}")),
        (false, true) => path == stripped || path.starts_with(&format!("{stripped}::")),
        (false, false) => pattern == path,
    }
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

    #[test]
    fn leading_wildcard_matches_any_ending() {
        assert!(matches_pattern("*::tests", "a::b::tests"));
        assert!(matches_pattern("*::tests", "tests"));
        assert!(matches_pattern("*::tests", "a::tests"));
        assert!(!matches_pattern("*::tests", "a::tests::b"));
        assert!(!matches_pattern("*::tests", "tester")); // not segment-aligned
    }

    #[test]
    fn segment_anywhere_wildcard_matches_inline_test_modules() {
        // The headline use case: `*::tests::*` should fire on any
        // function symbol or containing-module path that has `tests`
        // as a segment somewhere in the middle.
        assert!(matches_pattern("*::tests::*", "tests"));
        assert!(matches_pattern("*::tests::*", "a::tests"));
        assert!(matches_pattern("*::tests::*", "tests::nested"));
        assert!(matches_pattern("*::tests::*", "a::b::tests"));
        assert!(matches_pattern("*::tests::*", "a::b::tests::f"));
        assert!(matches_pattern("*::tests::*", "a::tests::b::c"));
        assert!(!matches_pattern("*::tests::*", "tester::hat"));
        assert!(!matches_pattern("*::tests::*", "a::testimony"));
    }

    #[test]
    fn malformed_bare_wildcard_does_not_match_anything() {
        // `*::` and `::*` alone with no body shouldn't quietly match
        // every path — that's what `*` is for.
        assert!(!matches_pattern("*::", "anything"));
        assert!(!matches_pattern("::*", "anything"));
    }
}
