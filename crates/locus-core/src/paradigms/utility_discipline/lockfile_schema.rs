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

// locus: ot canonical

use serde::{Deserialize, Serialize};

impl UtSection {
    /// True when neither `utility_paths` nor `generic_utility_patterns`
    /// are populated. UT001/002/004/005 all need `utility_paths`; UT003
    /// needs `generic_utility_patterns` (which has no built-in default —
    /// users seed via `init`).
    pub fn is_vacant(&self) -> bool {
        self.utility_paths.is_empty() && self.generic_utility_patterns.is_empty()
    }
}

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
    /// UT003 — module-path patterns recognised as "generic utility" naming.
    /// A new module whose `module_path` matches one of these patterns *and*
    /// is not present in `accepted_utility_paths` raises UT003. Empty
    /// disables UT003 entirely (lockfile-driven silence). [`Self::new`] /
    /// `init` seed this with [`DEFAULT_GENERIC_UTILITY_PATTERNS`].
    #[serde(default)]
    pub generic_utility_patterns: Vec<String>,
    /// UT003 — exact module paths (or patterns) that are explicitly
    /// accepted as utility modules even though they match
    /// `generic_utility_patterns`. Mirrors DG/OT acceptance lists: the user
    /// confirms "yes, this generic-named module is fine" once.
    #[serde(default)]
    pub accepted_utility_paths: Vec<String>,
    /// UT004 — patterns matching `AirTruthAction.target` that indicate a
    /// canonical concept being constructed (e.g. `User`, `*::User`,
    /// `*::domain::*`). Empty keeps UT004 silent until the user populates
    /// the list — there's no safe automatic guess for "what counts as a
    /// canonical concept" in an arbitrary codebase.
    #[serde(default)]
    pub canonical_construct_patterns: Vec<String>,
}

/// Default seed for [`UtSection::generic_utility_patterns`]. Exposed as a
/// constant so `init` and tests can share the same list.
pub const DEFAULT_GENERIC_UTILITY_PATTERNS: &[&str] = &[
    "*::utils::*",
    "*::utils",
    "*::helpers",
    "*::common",
    "*::misc",
    "*::shared",
];

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

    #[test]
    fn round_trips_all_fields_through_serde() {
        let s = UtSection {
            utility_paths: vec!["x::utils::*".into()],
            forbidden_imports: vec!["crate::domain::*".into()],
            generic_utility_patterns: vec!["*::utils::*".into(), "*::helpers".into()],
            accepted_utility_paths: vec!["x::utils::time".into()],
            canonical_construct_patterns: vec!["*::User".into()],
        };
        let j = serde_json::to_value(&s).unwrap();
        let back: UtSection = serde_json::from_value(j).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn default_section_has_empty_new_fields() {
        let s = UtSection::default();
        assert!(s.generic_utility_patterns.is_empty());
        assert!(s.accepted_utility_paths.is_empty());
        assert!(s.canonical_construct_patterns.is_empty());
    }
}
