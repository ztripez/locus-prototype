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

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
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

/// Pattern syntax: simple suffix wildcard, mirroring DG/UT.
/// - `foo::bar` — exact match
/// - `foo::*` — `foo` itself or any descendant (`foo::bar`, `foo::bar::baz`)
/// - `*` — anything
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
}
