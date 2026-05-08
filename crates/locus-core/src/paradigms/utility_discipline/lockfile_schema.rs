//! Shape of the UT section inside `locus.lock`.
//!
//! Rules family UT (Utility / Shared Module Discipline): the lockfile records
//! the module patterns that are *declared* utility modules. UT001 fires on
//! public types defined inside any module whose `module_path` matches one of
//! these patterns — utility modules are by definition domain-free, and a
//! public type carries semantics that should live in a domain/feature module
//! instead.
//!
//! No paths are inferred at `init` time: utility status is a user assertion,
//! not a guess. An empty `utility_paths` keeps the rule silent — same UX as
//! DG's lockfile-driven rules.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct UtSection {
    /// Module patterns matching `AirFile.module_path` for files declared as
    /// utility modules. Pattern syntax mirrors DG: simple suffix wildcards.
    #[serde(default)]
    pub utility_paths: Vec<String>,
    /// Import-path patterns that are forbidden inside any file matching
    /// `utility_paths`. Used by UT002: a utility module is by definition
    /// domain-free, so importing a feature/domain concept (`crate::domain::*`,
    /// `*::roles::*`, …) means the helper "knows about" semantics it shouldn't.
    /// Empty by default — UT002 stays silent until the user opts in.
    #[serde(default)]
    pub forbidden_imports: Vec<String>,
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
