//! Lockfile section shape for DC (Documentation / Comment Ownership).
//!
//! DC001 fires on public types and functions that have no doc comment.
//! Because "public API must be documented" is a project-wide policy choice,
//! the rule is gated on an explicit opt-in: `require_public_docs` defaults
//! to `false`, so DC is silent until the user turns it on. `exempt_paths`
//! lets the user carve out regions where the rule shouldn't apply
//! (test modules, generated code, FFI shims) without disabling the rule
//! entirely.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct DcSection {
    /// Top-level switch. Default `false` keeps DC001 silent until the user
    /// opts in — "public API must be documented" is a project policy, not
    /// a universal default.
    #[serde(default)]
    pub require_public_docs: bool,

    /// Module patterns matching `AirFile.module_path` whose contents skip
    /// the doc requirement. Typical entries: `*::tests::*`,
    /// `*::generated::*`, `*::ffi::*`. Pattern syntax mirrors UT/DG: simple
    /// suffix wildcards.
    #[serde(default)]
    pub exempt_paths: Vec<String>,
}

/// Pattern syntax: simple suffix wildcard, mirroring UT/DG.
/// - `foo::bar` — exact match
/// - `foo::*` — `foo` itself or any descendant (`foo::bar`, `foo::bar::baz`)
/// - `*` — anything
///
/// Duplicated locally rather than shared with UT to keep paradigm slices
/// independent — each paradigm owns its lockfile shape and helpers.
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
