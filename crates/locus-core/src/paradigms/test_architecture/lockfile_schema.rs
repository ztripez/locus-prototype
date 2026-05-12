//! Shape of the TA section inside `.locus/lock.json`.
//!
//! Rules family TA (Test Architecture Ownership): the lockfile records the
//! module patterns that identify *test* code. TA001 fires on public types
//! defined inside any module whose `module_path` matches one of these
//! patterns — test modules duplicating domain concepts as public types is the
//! "we made our own User struct in tests" smell the spec calls out. TA002
//! and TA003 widen the surface to *named-shadow* and *shape-shadow* tests
//! using the lockfile's `canonical_name_patterns` and `canonical_field_sets`.
//! TA004 catches port adapters living inside test code that hasn't been
//! declared a legitimate test-adapter home.
//!
//! No paths are inferred at `init` time: test status is a user assertion,
//! not a guess. An empty `test_paths` keeps the rule silent — same UX as UT
//! and DG's lockfile-driven rules. Each TA00N rule additionally short-circuits
//! when its own list (`canonical_name_patterns`, `canonical_field_sets`,
//! `port_trait_patterns`) is empty, so the per-rule onboarding gate is
//! independent.

// locus: ot canonical

use serde::{Deserialize, Serialize};

impl TaSection {
    /// True when no `test_paths` are declared. Every TA rule (TA001
    /// public types in tests, TA002/003 canonical-shadow checks, TA004
    /// port-impl in tests) is anchored on which files are tests.
    pub fn is_vacant(&self) -> bool {
        self.test_paths.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TaSection {
    /// Module patterns matching `AirFile.module_path` for files declared as
    /// test code (e.g. `*::tests::*`, `tests::*`). Pattern syntax mirrors UT
    /// and DG: simple suffix wildcards.
    #[serde(default)]
    pub test_paths: Vec<String>,
    /// Names (or name patterns) of accepted canonical types — typically the
    /// short names of types the user has accepted as canonical concepts in
    /// OT (e.g. `User`, `Email`, `Order`). TA002 fires when a type defined
    /// inside `test_paths` has a name matching any of these patterns. Empty
    /// keeps TA002 silent.
    #[serde(default)]
    pub canonical_name_patterns: Vec<String>,
    /// Field-name sets of accepted canonical concepts. Each inner `Vec` is
    /// the field-name set of one canonical concept (e.g.
    /// `["id", "email", "name"]` for `User`). TA003 computes Jaccard overlap
    /// between a test type's field-name set and each entry; an overlap of
    /// >= 0.5 against any entry trips the rule. Empty keeps TA003 silent.
    #[serde(default)]
    pub canonical_field_sets: Vec<Vec<String>>,
    /// Trait-path patterns identifying *port* traits (the abstraction side
    /// of the port/adapter split, typically suffixed `Repository`,
    /// `Gateway`, `Port`). TA004 fires when an `impl Port for Type` lands
    /// inside a `test_paths`-matching file unless the file also matches
    /// `accepted_test_adapter_paths`. Empty keeps TA004 silent.
    #[serde(default)]
    pub port_trait_patterns: Vec<String>,
    /// Module patterns where test-side port adapters are explicitly
    /// accepted (in-memory repositories, fake gateways, etc.). When a
    /// `test_paths` file is also covered by one of these, TA004 stays
    /// silent — the user has declared this is the right home for the
    /// test adapter.
    #[serde(default)]
    pub accepted_test_adapter_paths: Vec<String>,
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
        assert!(matches_pattern("foo::tests", "foo::tests"));
        assert!(!matches_pattern("foo::tests", "foo::tests::nested"));
        assert!(!matches_pattern("foo::tests", "foo"));
    }

    #[test]
    fn suffix_wildcard_includes_the_prefix_and_descendants() {
        assert!(matches_pattern("tests::*", "tests"));
        assert!(matches_pattern("tests::*", "tests::auth"));
        assert!(matches_pattern("tests::*", "tests::auth::login"));
        assert!(!matches_pattern("tests::*", "testsuite"));
        assert!(!matches_pattern("tests::*", "src"));
    }

    #[test]
    fn star_matches_anything() {
        assert!(matches_pattern("*", ""));
        assert!(matches_pattern("*", "foo::tests"));
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
