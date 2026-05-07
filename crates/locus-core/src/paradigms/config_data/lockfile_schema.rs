//! Shape of the CF section inside `locus.lock`.
//!
//! Rules family CF (Config/Data Ownership): behavior-shaping decision data
//! must live in an accepted config owner. CF001 — the first rule landing —
//! flags environment-variable reads from outside that owner. The lockfile
//! records which module paths *are* the config layer (`config_paths`); reads
//! anywhere else are violations.
//!
//! `config_paths` defaults empty: CF is silent until the user declares the
//! config layer — same lockfile-driven posture as DG, UT, and BO.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct CfSection {
    /// Module patterns identifying files that legitimately read configuration
    /// (env vars, config files, secret stores). Matched against
    /// `AirFile.module_path`. Examples: `"crate::config::*"`,
    /// `"crate::settings::*"`, `"crate::main"`.
    #[serde(default)]
    pub config_paths: Vec<String>,
}

/// Pattern syntax: simple suffix wildcard, mirroring DG, UT, and BO.
/// - `foo::bar` — exact match
/// - `foo::*` — `foo` itself or any descendant (`foo::bar`, `foo::bar::baz`)
/// - `*` — anything
///
/// Duplicated locally so the CF paradigm doesn't depend on DG, UT, or BO. If
/// a fourth paradigm needs the same helper, promote it to a shared module
/// then.
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
        assert!(matches_pattern("crate::config", "crate::config"));
        assert!(!matches_pattern("crate::config", "crate::config::loader"));
        assert!(!matches_pattern("crate::config", "crate"));
    }

    #[test]
    fn suffix_wildcard_includes_the_prefix_and_descendants() {
        assert!(matches_pattern("crate::config::*", "crate::config"));
        assert!(matches_pattern("crate::config::*", "crate::config::loader"));
        assert!(matches_pattern(
            "crate::config::*",
            "crate::config::env::reader"
        ));
        assert!(!matches_pattern("crate::config::*", "crate::configurator"));
        assert!(!matches_pattern("crate::config::*", "crate::handler"));
    }

    #[test]
    fn star_matches_anything() {
        assert!(matches_pattern("*", ""));
        assert!(matches_pattern("*", "crate::handler"));
        assert!(matches_pattern("*", "crate::config::loader"));
    }
}
