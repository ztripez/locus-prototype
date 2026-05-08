//! Shape of the FO section inside `locus.lock`.
//!
//! Rules family FO (Feature Ownership): records the named feature regions of
//! the workspace so FO rules can detect domain concepts being defined inside
//! more than one feature. The shape mirrors DG's `FeatureDefinition` minus
//! `public_api` — FO only cares about *where* a feature lives, not what it
//! exposes. The duplicate is intentional: paradigms shouldn't depend on each
//! other's lockfile schemas.

// ot: canonical

use serde::{Deserialize, Serialize};

impl FoSection {
    /// True when no features and no shared paths are declared. FO001
    /// (cross-feature concept duplication) needs `features`; FO004
    /// (shared knowing about features) needs both lists.
    pub fn is_vacant(&self) -> bool {
        self.features.is_empty() && self.shared_paths.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct FoSection {
    /// Named feature regions of the workspace. FO001 fires when the same
    /// public type name is defined in two different features. An empty list
    /// keeps every FO rule silent.
    #[serde(default)]
    pub features: Vec<FoFeature>,

    /// Module patterns identifying *shared* (cross-feature) regions of the
    /// workspace — DTO crates, normalized-event payloads, common utility
    /// types. FO004 fires when a type defined inside one of these regions
    /// has a field whose `type_text` mentions a symbol owned by a declared
    /// feature: a shared module that knows about feature internals stops
    /// being shared. Empty list keeps FO004 silent.
    #[serde(default)]
    pub shared_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FoFeature {
    /// Human-readable feature name (`"billing"`, `"identity"`, …).
    pub name: String,
    /// Module pattern matching every path that *belongs* to this feature.
    /// e.g. `"lore_engine_billing::*"` or `"crate::billing::*"`.
    pub module: String,
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
