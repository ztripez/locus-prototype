//! Shape of the BO section inside `locus.lock`.
//!
//! Rules family BO (Boundary Ownership): protocol/infrastructure concerns
//! belong at the boundary, not inside the domain layer. The lockfile records
//! which module paths are domain/application code (`domain_paths`) and which
//! import paths are forbidden inside that code (`forbidden_in_domain` —
//! transport, persistence, serialization frameworks, etc.).
//!
//! Both fields default empty: BO is silent until the user declares the
//! split — same lockfile-driven posture as DG and UT.

// locus: ot canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BoSection {
    /// Module patterns identifying domain/application files. Matched against
    /// `AirFile.module_path`. Examples: `"crate::domain::*"`,
    /// `"crate::application::*"`.
    #[serde(default)]
    pub domain_paths: Vec<String>,
    /// Import-path patterns that domain/application files must not reach.
    /// Matched against `AirImport.path`. Examples: `"serde::*"`, `"sqlx::*"`,
    /// `"reqwest::*"`, `"tonic::*"`.
    #[serde(default)]
    pub forbidden_in_domain: Vec<String>,
    /// Persistence-shaped type-text patterns that must not appear in the
    /// signature of a function defined inside a `domain_paths` file. Matched
    /// against `AirFunction.params` type texts and `AirFunction.return_type`.
    /// Examples: `"sqlx::PgRow"`, `"diesel::*"`, `"sea_orm::*"`. BO002's
    /// signal — silent until populated.
    #[serde(default)]
    pub persistence_type_patterns: Vec<String>,
    /// Module patterns identifying canonical (domain) types. Matched against
    /// `AirType.span`'s containing `AirFile.module_path`. BO004's gate — fires
    /// when one of these types carries a derive listed in
    /// `forbidden_canonical_derives`. Silent until populated.
    #[serde(default)]
    pub canonical_paths: Vec<String>,
    /// Derive names that must NOT appear on canonical types. Defaults to the
    /// serde-shaped trio + utoipa's `ToSchema` because canonical domain types
    /// shouldn't depend on serialization/schema frameworks.
    #[serde(default = "default_forbidden_canonical_derives")]
    pub forbidden_canonical_derives: Vec<String>,
}

/// Default derive list for [`BoSection::forbidden_canonical_derives`]. Used by
/// BO004 to fire on canonical types annotated with serialization/schema
/// derives. Override via the lockfile if you accept these on domain types.
pub fn default_forbidden_canonical_derives() -> Vec<String> {
    ["Serialize", "Deserialize", "Schema", "ToSchema"]
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

impl Default for BoSection {
    fn default() -> Self {
        Self {
            domain_paths: Vec::new(),
            forbidden_in_domain: Vec::new(),
            persistence_type_patterns: Vec::new(),
            canonical_paths: Vec::new(),
            forbidden_canonical_derives: default_forbidden_canonical_derives(),
        }
    }
}

impl BoSection {
    /// True when no user declarations are populated. BO is vacant-by-
    /// definition: rules need either `domain_paths` (BO001/002/005) or
    /// `canonical_paths` (BO004) to fire on anything specific.
    pub fn is_vacant(&self) -> bool {
        self.domain_paths.is_empty()
            && self.forbidden_in_domain.is_empty()
            && self.persistence_type_patterns.is_empty()
            && self.canonical_paths.is_empty()
    }
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
