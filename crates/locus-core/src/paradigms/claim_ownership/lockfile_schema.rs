//! Lockfile section shape for CL (Claim Ownership).
//!
//! CL001 (orphan external reference) fires when a doc comment cites an
//! issue / PR / URL but doesn't carry a local rationale. Because the rule
//! is a heuristic over natural language, it ships **opt-in** —
//! `require_local_rationale` defaults to `false`, so CL is silent until
//! the user turns it on. This mirrors DC's `require_public_docs` opt-in.
//!
//! Once enabled, `exempt_paths` lets the user carve out regions where
//! the rule shouldn't apply (test modules, generated code, vendored
//! files) without disabling it entirely.
//!
//! Spec: `docs/superpowers/specs/2026-05-09-claim-ownership-paradigm.md`.

// locus: ot canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ClSection {
    /// Top-level switch for CL001 (orphan external reference). Default
    /// `false` keeps the rule silent until the user opts in.
    #[serde(default)]
    pub require_local_rationale: bool,

    /// Module patterns matching `AirFile.module_path` whose contents skip
    /// CL001. Typical entries: `*::tests::*`, `*::generated::*`,
    /// `*::vendor::*`. Pattern syntax mirrors DC/UT — simple
    /// segment-aligned wildcards (see [`matches_pattern`]).
    #[serde(default)]
    pub exempt_paths: Vec<String>,
}

impl ClSection {
    /// True if the section carries no real configuration. Used to keep
    /// `init` outputs round-trip-stable.
    pub fn is_vacant(&self) -> bool {
        !self.require_local_rationale && self.exempt_paths.is_empty()
    }
}

/// Glob matcher for module paths. Mirrors the DG / OT matcher: exact,
/// `prefix::*` (descendants), `*::suffix` (segment-aligned tail), and
/// `*::middle::*` (segment-anywhere) shapes.
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
    fn default_section_is_vacant_and_silent() {
        let s = ClSection::default();
        assert!(s.is_vacant());
        assert!(!s.require_local_rationale);
        assert!(s.exempt_paths.is_empty());
    }

    #[test]
    fn populated_toggle_makes_section_non_vacant() {
        let s = ClSection {
            require_local_rationale: true,
            ..ClSection::default()
        };
        assert!(!s.is_vacant());
    }

    #[test]
    fn exempt_path_pattern_matches_segment_anywhere() {
        assert!(matches_pattern("*::tests::*", "a::b::tests::c"));
        assert!(matches_pattern("*::tests::*", "tests"));
        assert!(!matches_pattern("*::tests::*", "a::testimony"));
    }
}
