//! Shape of the DG section inside `locus.lock`.
//!
//! Rules family DG (Dependency Graph / Direction): the lockfile records the
//! *forbidden* edges in the architecture graph. Allowed edges are everything
//! else — explicit allowlists invert poorly when most of the workspace is
//! fine and only a few crossings are wrong.

// ot: canonical

use serde::{Deserialize, Serialize};

impl DgSection {
    /// True when no architectural direction is declared. DG002 (cycle
    /// detection) is structural and fires regardless; DG001/003/004 need
    /// `forbidden_edges`, `features`, or `shared_paths`.
    pub fn is_vacant(&self) -> bool {
        self.forbidden_edges.is_empty() && self.features.is_empty() && self.shared_paths.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct DgSection {
    /// Each entry forbids all imports where the importing module matches
    /// `from` and the imported path matches `to`.
    #[serde(default)]
    pub forbidden_edges: Vec<ForbiddenEdge>,
    /// Named feature regions of the workspace — used by DG003 to enforce
    /// that cross-feature imports go through the destination feature's
    /// public API.
    #[serde(default)]
    pub features: Vec<FeatureDefinition>,
    /// Module patterns whose code is shared infrastructure. Used by DG004 to
    /// catch shared modules that depend on feature-specific code (the
    /// dependency direction must stay feature → shared, never the reverse).
    #[serde(default)]
    pub shared_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForbiddenEdge {
    /// Module pattern of the *importer*. Matches `AirFile.module_path`.
    pub from: String,
    /// Module pattern the importer must not reach. Matches `AirImport.path`.
    pub to: String,
    /// Optional human-readable reason — surfaced in the diagnostic.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FeatureDefinition {
    /// Human-readable feature name (`"billing"`, `"identity"`, …).
    pub name: String,
    /// Module pattern matching every path that *belongs* to this feature.
    /// e.g. `"lore_engine_billing::*"` or `"crate::billing::*"`.
    pub module: String,
    /// Patterns describing this feature's public-API surface. Imports from
    /// other features must match one of these patterns. An empty list means
    /// the feature has no public API — every cross-feature import into it
    /// is internal-only and trips DG003.
    #[serde(default)]
    pub public_api: Vec<String>,
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
