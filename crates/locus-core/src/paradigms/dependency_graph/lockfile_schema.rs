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
