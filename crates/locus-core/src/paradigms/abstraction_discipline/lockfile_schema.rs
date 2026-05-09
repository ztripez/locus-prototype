//! Lockfile section shape for AB (Abstraction Discipline).
//!
//! AB records traits the user has reviewed and accepted as legitimately
//! single-impl — typically genuine port traits with one impl per environment
//! (production vs. test, native vs. wasm, …). Patterns match against the
//! trait's fully-qualified symbol; AB001 fires on any declared trait whose
//! impl count is exactly one and whose symbol is *not* covered by any
//! pattern in `accepted_single_impl_traits`.
//!
//! Default empty for AB001: out of the box, AB001 fires on every
//! speculative trait so the user has to examine and either delete or
//! accept each one. Exemptions accumulate as the user validates port
//! boundaries — same UX shape as DG/UT.
//!
//! AB002 layers a separate signal on top: types whose *names* match the
//! "manager / processor / coordinator" suspect-abstraction patterns from
//! the spec. Defaults seed the spec's canonical list of role-name
//! suffixes; users can pin individual exceptions to
//! `accepted_abstraction_names`.

// locus: ot canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    /// AB002 — name-suffix patterns identifying "suspect abstractions"
    /// (manager / processor / coordinator / …). A type whose short name
    /// matches one of these patterns and isn't otherwise exempted will be
    /// flagged. Pattern syntax is a leading-`*` suffix glob: `*Manager`
    /// matches `UserManager`, `*` matches anything. Defaults to
    /// [`default_suspect_abstraction_patterns`] — the spec's seeded list
    /// of role-name red flags.
    #[serde(default = "default_suspect_abstraction_patterns")]
    pub suspect_abstraction_patterns: Vec<String>,
    /// AB002 — exact symbol/short-name patterns for types that match a
    /// suspect pattern but have been reviewed and accepted. Pattern
    /// syntax mirrors `accepted_single_impl_traits`. Default empty: every
    /// suspect-named type is flagged until the user explicitly accepts
    /// it.
    #[serde(default)]
    pub accepted_abstraction_names: Vec<String>,
}

impl Default for AbSection {
    fn default() -> Self {
        Self {
            accepted_single_impl_traits: Vec::new(),
            suspect_abstraction_patterns: default_suspect_abstraction_patterns(),
            accepted_abstraction_names: Vec::new(),
        }
    }
}

/// Default suspect-abstraction name patterns. Mirrors the spec's seeded
/// list of role-name suffixes that almost always indicate the
/// "manager / processor / DataHandler" pattern: types named after a
/// generic role rather than a domain concept. Users can prune patterns
/// they don't want enforced (e.g. an existing `*Service` codebase).
pub fn default_suspect_abstraction_patterns() -> Vec<String> {
    vec![
        "*Manager".to_string(),
        "*Service".to_string(),
        "*Processor".to_string(),
        "*Coordinator".to_string(),
        "*Orchestrator".to_string(),
        "*Engine".to_string(),
        "*Handler".to_string(),
        "*Helper".to_string(),
        "*Util".to_string(),
    ]
}

/// Match a name against an AB002 suspect pattern. Distinct from
/// [`matches_pattern`] because the AB002 pattern syntax is a *name*-suffix
/// (`*Manager`) rather than a path-segment glob (`foo::*`). The two share
/// no semantics — a name matcher that also handled `::` would invite false
/// matches against trait paths.
pub fn matches_name_pattern(pattern: &str, name: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return name.ends_with(suffix);
    }
    pattern == name
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
        // AB002 ships seeded patterns and an empty acceptance list.
        assert_eq!(
            s.suspect_abstraction_patterns,
            default_suspect_abstraction_patterns()
        );
        assert!(s.accepted_abstraction_names.is_empty());
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
    fn name_pattern_helper_matches_suffix_glob() {
        assert!(matches_name_pattern("*Manager", "UserManager"));
        assert!(matches_name_pattern("*Manager", "Manager"));
        assert!(!matches_name_pattern("*Manager", "ManagerThing"));
        assert!(matches_name_pattern("Foo", "Foo"));
        assert!(!matches_name_pattern("Foo", "FooBar"));
        assert!(matches_name_pattern("*", "anything"));
    }

    #[test]
    fn default_suspect_patterns_cover_canonical_role_names() {
        let defaults = default_suspect_abstraction_patterns();
        for expected in [
            "*Manager",
            "*Service",
            "*Processor",
            "*Coordinator",
            "*Orchestrator",
            "*Engine",
            "*Handler",
            "*Helper",
            "*Util",
        ] {
            assert!(
                defaults.iter().any(|p| p == expected),
                "default suspect patterns missing `{expected}`: {defaults:?}"
            );
        }
    }

    #[test]
    fn round_trips_through_serde() {
        let s = AbSection {
            accepted_single_impl_traits: vec!["crate::ports::*".into(), "Clock".into()],
            suspect_abstraction_patterns: vec!["*Manager".into(), "*Helper".into()],
            accepted_abstraction_names: vec!["x::core::Helper".into()],
        };
        let j = serde_json::to_value(&s).unwrap();
        let back: AbSection = serde_json::from_value(j).unwrap();
        assert_eq!(s, back);
    }
}
