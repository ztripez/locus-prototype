//! Shape of the CR section inside `locus.lock`.
//!
//! Rules family CR (Composition Root Ownership): the lockfile records which
//! modules are accepted *composition roots* — the only places where concrete
//! services, clients, repositories, adapters, etc. may be constructed. CR001
//! fires when a "service-shaped" type (heuristic by suffix) is constructed
//! outside any declared composition root.
//!
//! No paths are inferred at `init` time: composition-root status is a user
//! assertion, not a guess. An empty `composition_root_paths` keeps the rule
//! silent — same UX as DG/UT lockfile-driven rules.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrSection {
    /// Module patterns matching `AirFile.module_path` for files declared as
    /// composition roots. Pattern syntax mirrors DG/UT: simple suffix
    /// wildcards (e.g. `bin::*`, `crate::wire`, `crate::composition`).
    #[serde(default)]
    pub composition_root_paths: Vec<String>,

    /// Type-name suffixes treated as "service-shaped" — i.e. the heuristic
    /// CR001 uses to decide a `Construct` action wires up infrastructure
    /// rather than a domain value. An empty list resolves to
    /// [`default_service_suffixes`] at rule-evaluation time, so users only
    /// need to set this when overriding the canonical seven.
    #[serde(default)]
    pub service_suffixes: Vec<String>,

    /// Maximum number of `Construct` actions a single function inside a
    /// composition-root module may emit before CR002 fires. Even a
    /// legitimate composition root should be split if one function wires
    /// 20+ services in a single block. Default: 12.
    #[serde(default = "default_wiring_density_threshold")]
    pub wiring_density_threshold: u32,
}

/// Default wiring-density threshold for CR002. Heuristic — reasonable for
/// most app shells; override per project.
pub fn default_wiring_density_threshold() -> u32 {
    12
}

impl Default for CrSection {
    fn default() -> Self {
        Self {
            composition_root_paths: Vec::new(),
            service_suffixes: Vec::new(),
            wiring_density_threshold: default_wiring_density_threshold(),
        }
    }
}

/// The canonical seven service-shaped suffixes per the spec. Used when the
/// user-supplied `service_suffixes` is empty.
pub fn default_service_suffixes() -> Vec<String> {
    [
        "Service",
        "Client",
        "Repository",
        "Adapter",
        "Connection",
        "Pool",
        "Manager",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

/// Resolve the active service-suffix list: the user override if non-empty,
/// otherwise the defaults.
pub fn effective_service_suffixes(section: &CrSection) -> Vec<String> {
    if section.service_suffixes.is_empty() {
        default_service_suffixes()
    } else {
        section.service_suffixes.clone()
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
    fn defaults_match_the_canonical_seven() {
        let defaults = default_service_suffixes();
        assert_eq!(
            defaults,
            vec![
                "Service".to_string(),
                "Client".to_string(),
                "Repository".to_string(),
                "Adapter".to_string(),
                "Connection".to_string(),
                "Pool".to_string(),
                "Manager".to_string(),
            ]
        );
    }

    #[test]
    fn effective_suffixes_falls_back_to_defaults_when_empty() {
        let section = CrSection::default();
        assert_eq!(
            effective_service_suffixes(&section),
            default_service_suffixes()
        );
    }

    #[test]
    fn effective_suffixes_uses_user_override_when_present() {
        let section = CrSection {
            composition_root_paths: Vec::new(),
            service_suffixes: vec!["Gateway".into(), "Provider".into()],
            ..Default::default()
        };
        assert_eq!(
            effective_service_suffixes(&section),
            vec!["Gateway".to_string(), "Provider".to_string()],
        );
    }

    #[test]
    fn matches_pattern_exact() {
        assert!(matches_pattern("crate::main", "crate::main"));
        assert!(!matches_pattern("crate::main", "crate::main::nested"));
    }

    #[test]
    fn matches_pattern_suffix_wildcard() {
        assert!(matches_pattern("bin::*", "bin"));
        assert!(matches_pattern("bin::*", "bin::main"));
        assert!(matches_pattern("bin::*", "bin::wire::module"));
        assert!(!matches_pattern("bin::*", "binary"));
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
