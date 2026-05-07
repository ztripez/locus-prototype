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

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
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
}

/// Pattern syntax: simple suffix wildcard, mirroring DG and UT.
/// - `foo::bar` — exact match
/// - `foo::*` — `foo` itself or any descendant (`foo::bar`, `foo::bar::baz`)
/// - `*` — anything
///
/// More expressive forms (`*::domain::*`, regex, segment filters) are deferred
/// until the simple shape proves insufficient. Duplicated locally so the BO
/// paradigm doesn't depend on DG or UT.
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
}
