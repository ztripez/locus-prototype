//! Lockfile section shape for AB (Abstraction Discipline).
//!
//! AB records traits the user has reviewed and accepted as legitimately
//! single-impl — typically genuine port traits with one impl per environment
//! (production vs. test, native vs. wasm, …). Patterns match against the
//! trait's fully-qualified symbol; AB001 fires on any declared trait whose
//! impl count is exactly one and whose symbol is *not* covered by any
//! pattern in `accepted_single_impl_traits`.
//!
//! Default empty: out of the box, AB001 fires on every speculative trait so
//! the user has to examine and either delete or accept each one. Exemptions
//! accumulate as the user validates port boundaries — same UX shape as DG/UT.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct AbSection {
    /// Trait symbol patterns that are exempt from AB001. Pattern syntax
    /// mirrors DG/MO/UT: simple suffix wildcards.
    ///
    /// Match semantics: a trait is exempt if any of these patterns matches
    /// either its full symbol (`crate::ports::Clock`) or its short name
    /// (`Clock`). The short-name fallback lets users write `MyTrait` without
    /// pinning the full path, useful when a trait is re-exported.
    #[serde(default)]
    pub accepted_single_impl_traits: Vec<String>,
}

/// Pattern syntax: simple suffix wildcard, mirroring DG/MO/UT.
/// - `foo::bar` — exact match
/// - `foo::*` — `foo` itself or any descendant (`foo::bar`, `foo::bar::baz`)
/// - `*` — anything
///
/// Kept as a local copy so AB doesn't depend on a peer paradigm's helper; if
/// a third paradigm grows the same need beyond the current four, promote to
/// a shared module.
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
        let s = AbSection::default();
        assert!(s.accepted_single_impl_traits.is_empty());
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
        let s = AbSection {
            accepted_single_impl_traits: vec!["crate::ports::*".into(), "Clock".into()],
        };
        let j = serde_json::to_value(&s).unwrap();
        let back: AbSection = serde_json::from_value(j).unwrap();
        assert_eq!(s, back);
    }
}
