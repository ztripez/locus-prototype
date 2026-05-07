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

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
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
}

/// Pattern syntax: simple suffix wildcard, mirroring AB/DG/MO/UT.
/// - `foo::Bar` — exact match
/// - `foo::*` — `foo` itself or any descendant (`foo::bar`, `foo::bar::baz`)
/// - `*` — anything
///
/// Kept as a local copy so PA doesn't depend on a peer paradigm's helper; if
/// a third paradigm grows the same need beyond the current handful, promote
/// to a shared module.
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
        };
        let j = serde_json::to_value(&s).unwrap();
        let back: PaSection = serde_json::from_value(j).unwrap();
        assert_eq!(s, back);
    }
}
