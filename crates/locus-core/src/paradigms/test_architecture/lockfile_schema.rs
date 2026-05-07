//! Shape of the TA section inside `locus.lock`.
//!
//! Rules family TA (Test Architecture Ownership): the lockfile records the
//! module patterns that identify *test* code. TA001 fires on public types
//! defined inside any module whose `module_path` matches one of these
//! patterns — test modules duplicating domain concepts as public types is the
//! "we made our own User struct in tests" smell the spec calls out.
//!
//! No paths are inferred at `init` time: test status is a user assertion,
//! not a guess. An empty `test_paths` keeps the rule silent — same UX as UT
//! and DG's lockfile-driven rules.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TaSection {
    /// Module patterns matching `AirFile.module_path` for files declared as
    /// test code (e.g. `*::tests::*`, `tests::*`). Pattern syntax mirrors UT
    /// and DG: simple suffix wildcards.
    #[serde(default)]
    pub test_paths: Vec<String>,
}

/// Pattern syntax: simple suffix wildcard, mirroring UT/DG.
/// - `foo::bar` — exact match
/// - `foo::*` — `foo` itself or any descendant (`foo::bar`, `foo::bar::baz`)
/// - `*` — anything
///
/// More expressive forms (`*::tests::*`, regex, segment filters) are deferred
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
}
