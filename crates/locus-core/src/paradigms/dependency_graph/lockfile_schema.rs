//! Shape of the DG section inside `locus.lock`.
//!
//! Rules family DG (Dependency Graph / Direction): the lockfile records the
//! *forbidden* edges in the architecture graph. Allowed edges are everything
//! else — explicit allowlists invert poorly when most of the workspace is
//! fine and only a few crossings are wrong.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct DgSection {
    /// Each entry forbids all imports where the importing module matches
    /// `from` and the imported path matches `to`.
    #[serde(default)]
    pub forbidden_edges: Vec<ForbiddenEdge>,
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

/// Pattern syntax: simple suffix wildcard.
/// - `foo::bar` — exact match
/// - `foo::*` — `foo` itself or any descendant (`foo::bar`, `foo::bar::baz`)
/// - `*` — anything
///
/// More expressive patterns (`*::api::*`, regex, segment filters) are deferred
/// until the simple form proves insufficient.
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
