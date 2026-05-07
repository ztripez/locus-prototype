//! Lockfile section shape for DA (Demand-Driven Architecture).
//!
//! DA records the user's accepted answer to "which abstractions are demanded
//! today and which are speculative?" The section is lockfile-driven and
//! silent by default — until the user opts in with `enabled = true`, no DA
//! rule fires on un-onboarded code.
//!
//! Phase scope: just enough surface for [`DA001`](super::rules::da001) — a
//! toggle plus an allow-list of trait patterns the user has accepted as
//! single-implementation by design (real ports, marker traits, intentional
//! seams). Future DA rules will hang their own fields here.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct DaSection {
    /// Master switch for the paradigm. When `false` (the default), every DA
    /// rule short-circuits to an empty diagnostic list — same lockfile-driven
    /// silence convention as DG/MO/CX. Users opt in once they're ready to
    /// audit speculative architecture.
    #[serde(default)]
    pub enabled: bool,
    /// Patterns that mark a trait as an accepted single-implementation
    /// abstraction. Matched against either the trait's full symbol
    /// (`my_crate::ports::Clock`) or its short name (`Clock`). Anything
    /// matching is exempt from DA001 — typical entries are real ports
    /// (interface for an external boundary), marker traits, and traits whose
    /// second implementation is expected to land soon (recorded with a
    /// follow-up note in code review, not here).
    #[serde(default)]
    pub accepted_single_impl: Vec<String>,
}

/// Pattern syntax: simple suffix wildcard.
/// - `foo::bar` — exact match
/// - `foo::*` — `foo` itself or any descendant (`foo::bar`, `foo::bar::baz`)
/// - `*` — anything
///
/// Mirrors the helper in DG/MO/CX. Kept as a local copy so DA stays
/// decoupled from the other paradigms; if a fourth paradigm needs the same
/// helper, promote it to a shared module then.
pub fn matches_pattern(pattern: &str, path: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix("::*") {
        return path == prefix || path.starts_with(&format!("{prefix}::"));
    }
    pattern == path
}

impl DaSection {
    /// Returns true when any pattern in `accepted_single_impl` matches either
    /// `symbol` (full path) or `name` (short identifier). Both are tried so
    /// users can choose granularity — `Clock` for "any trait named Clock" or
    /// `my_crate::ports::Clock` for the specific one.
    pub fn is_accepted(&self, symbol: &str, name: &str) -> bool {
        self.accepted_single_impl
            .iter()
            .any(|pat| matches_pattern(pat, symbol) || matches_pattern(pat, name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_section_is_disabled_and_empty() {
        let s = DaSection::default();
        assert!(!s.enabled);
        assert!(s.accepted_single_impl.is_empty());
    }

    #[test]
    fn is_accepted_matches_short_name_and_full_symbol() {
        let s = DaSection {
            enabled: true,
            accepted_single_impl: vec![
                "Clock".into(),                        // by short name
                "my_crate::ports::EmailSender".into(), // by full symbol
                "my_crate::infra::*".into(),           // by suffix wildcard
            ],
        };
        // Short-name pattern hits short name, regardless of symbol.
        assert!(s.is_accepted("any::module::Clock", "Clock"));
        // Full-symbol pattern hits exact symbol.
        assert!(s.is_accepted("my_crate::ports::EmailSender", "EmailSender"));
        // Wildcard pattern hits descendants of the namespace.
        assert!(s.is_accepted("my_crate::infra::redis::Cache", "Cache"));
        // Nothing matches — not accepted.
        assert!(!s.is_accepted("other::module::Manager", "Manager"));
    }

    #[test]
    fn pattern_helper_matches_dg_semantics() {
        assert!(matches_pattern("foo::bar", "foo::bar"));
        assert!(!matches_pattern("foo::bar", "foo::bar::baz"));
        assert!(matches_pattern("foo::*", "foo"));
        assert!(matches_pattern("foo::*", "foo::bar::baz"));
        assert!(!matches_pattern("foo::*", "foobar"));
        assert!(matches_pattern("*", "anything"));
    }

    #[test]
    fn round_trips_through_serde() {
        let s = DaSection {
            enabled: true,
            accepted_single_impl: vec!["Clock".into(), "my_crate::ports::*".into()],
        };
        let j = serde_json::to_value(&s).unwrap();
        let back: DaSection = serde_json::from_value(j).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn missing_fields_deserialize_to_default() {
        // Empty object → default (silent) section.
        let s: DaSection = serde_json::from_str("{}").unwrap();
        assert_eq!(s, DaSection::default());
        // Partial object — only `enabled` set, `accepted_single_impl` defaults.
        let s: DaSection = serde_json::from_str(r#"{"enabled": true}"#).unwrap();
        assert!(s.enabled);
        assert!(s.accepted_single_impl.is_empty());
    }
}
