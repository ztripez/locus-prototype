//! Lockfile section shape for PA (Port/Adapter Ownership).
//!
//! PA001 fires when a trait declaration and its single implementation share
//! a source file — the classic "I made a port to abstract this thing, but I
//! never actually abstracted anything" smell. Co-location of port and adapter
//! is acceptable for utility helper traits that don't represent ports;
//! `accepted_colocated_traits` is the user's escape hatch for those cases.
//!
//! Default empty: out of the box, every co-located trait+impl pair fires PA001
//! so the user examines and either physically splits the port from its
//! adapter, or accepts the trait as a non-port utility. Exemptions accumulate
//! as port boundaries get reviewed — same UX shape as AB/DG/MO/UT.
//!
//! Note PA001 deliberately overlaps with AB001: AB asks "is this trait
//! justified at all?", PA asks "if it's a port, is it physically split from
//! its adapter?". Different framings; users can disable whichever they prefer.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaSection {
    /// Trait symbol patterns that are exempt from PA001. Pattern syntax
    /// mirrors AB/DG/MO/UT: simple suffix wildcards.
    ///
    /// Match semantics: a trait is exempt if any of these patterns matches
    /// either its full symbol (`crate::ports::Clock`) or its short name
    /// (`Clock`). The short-name fallback lets users write `MyTrait` without
    /// pinning the full path, useful when a trait is re-exported.
    #[serde(default)]
    pub accepted_colocated_traits: Vec<String>,
    /// Module patterns for application/domain layer files. PA002 fires when
    /// an `AirImport` in one of these files points at a concrete adapter
    /// framework. Examples: `"crate::application::*"`,
    /// `"crate::domain::*"`. Silent until populated.
    #[serde(default)]
    pub application_paths: Vec<String>,
    /// Concrete-adapter import-path patterns the application/domain layer
    /// must not depend on. Matched against `AirImport.path`. Examples:
    /// `"reqwest::*"`, `"sqlx::*"`, `"redis::*"`, `"hyper::*"`. PA002's
    /// signal — silent until populated.
    #[serde(default)]
    pub concrete_adapter_patterns: Vec<String>,
    /// Type patterns (matched against `AirTruthAction.target` for `Construct`
    /// actions) identifying concrete adapters. PA004 fires when a
    /// construction of one of these types happens outside any
    /// `accepted_construction_paths` file. Silent until populated.
    #[serde(default)]
    pub adapter_type_patterns: Vec<String>,
    /// Module patterns for files where adapter construction is acceptable —
    /// composition roots, bootstrap modules, etc. Matched against
    /// `AirFile.module_path`. Defaults seed the canonical entry points
    /// (`*::main`, `*::bootstrap::*`, `*::composition::*`).
    #[serde(default = "default_accepted_construction_paths")]
    pub accepted_construction_paths: Vec<String>,
}

/// Default `accepted_construction_paths` — the conventional composition-root
/// entry points. Override via the lockfile if your project uses a different
/// shape.
pub fn default_accepted_construction_paths() -> Vec<String> {
    ["*::main", "*::bootstrap::*", "*::composition::*"]
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

impl Default for PaSection {
    fn default() -> Self {
        Self {
            accepted_colocated_traits: Vec::new(),
            application_paths: Vec::new(),
            concrete_adapter_patterns: Vec::new(),
            adapter_type_patterns: Vec::new(),
            accepted_construction_paths: default_accepted_construction_paths(),
        }
    }
}

/// Pattern syntax: segment-aligned wildcards, matching BO/CR.
/// - `foo::bar` — exact match
/// - `foo::*` — `foo` itself or any descendant (`foo::bar`, `foo::bar::baz`)
/// - `*::foo` — `foo` itself or anywhere ending with `::foo`
/// - `*::foo::*` — `foo` appearing as a whole segment anywhere
/// - `*` — anything
///
/// Kept as a local copy so PA doesn't depend on a peer paradigm's helper; if
/// a third paradigm grows the same need beyond the current handful, promote
/// to a shared module.
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
    fn default_section_has_no_exemptions() {
        let s = PaSection::default();
        assert!(s.accepted_colocated_traits.is_empty());
    }

    #[test]
    fn pattern_helper_matches_dg_semantics() {
        assert!(matches_pattern("foo::Bar", "foo::Bar"));
        assert!(!matches_pattern("foo::Bar", "foo::Baz"));
        assert!(matches_pattern("foo::*", "foo"));
        assert!(matches_pattern("foo::*", "foo::bar::Baz"));
        assert!(!matches_pattern("foo::*", "foobar"));
        assert!(matches_pattern("*", "anything::nested"));
    }

    #[test]
    fn round_trips_through_serde() {
        let s = PaSection {
            accepted_colocated_traits: vec!["crate::utils::*".into(), "Helper".into()],
            ..Default::default()
        };
        let j = serde_json::to_value(&s).unwrap();
        let back: PaSection = serde_json::from_value(j).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn default_accepted_construction_paths_seeds_canonical_entry_points() {
        let s = PaSection::default();
        assert_eq!(
            s.accepted_construction_paths,
            vec![
                "*::main".to_string(),
                "*::bootstrap::*".to_string(),
                "*::composition::*".to_string(),
            ]
        );
    }

    #[test]
    fn leading_wildcard_matches_any_ending() {
        assert!(matches_pattern("*::main", "crate::main"));
        assert!(matches_pattern("*::main", "main"));
        assert!(!matches_pattern("*::main", "main::nested"));
    }
}
