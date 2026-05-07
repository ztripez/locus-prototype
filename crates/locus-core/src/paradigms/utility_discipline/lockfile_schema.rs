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

/// Pattern syntax: simple suffix wildcard, mirroring DG.
/// - `foo::bar` — exact match
/// - `foo::*` — `foo` itself or any descendant (`foo::bar`, `foo::bar::baz`)
/// - `*` — anything
///
/// More expressive forms (`*::utils::*`, regex, segment filters) are deferred
/// until the simple shape proves insufficient.
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
