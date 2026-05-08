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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

    /// Function-name globs for DA002 — names that look like factory
    /// functions (`create_*`, `make_*`, `*_factory`, `build_*`). DA002
    /// fires when a function whose name matches any pattern only ever
    /// constructs **one** type (i.e. the abstraction has no variation —
    /// it's a renamed constructor). Empty list keeps DA002 silent.
    #[serde(default = "default_factory_name_patterns")]
    pub factory_name_patterns: Vec<String>,

    /// Type-name globs for DA007 — names that look like strategy /
    /// policy / mode enums. DA007 fires when an enum whose name matches
    /// any pattern has **exactly one** variant (a stub abstraction — no
    /// actual variation, just speculative shape). Empty list keeps DA007
    /// silent.
    #[serde(default = "default_strategy_name_patterns")]
    pub strategy_name_patterns: Vec<String>,
}

impl DaSection {
    /// True when DA is opted out (the master switch is `false`). All DA
    /// rules gate on `enabled`, so without it the paradigm produces no
    /// signal — LOCUS002 nudges users to either flip the switch or ack
    /// the paradigm.
    pub fn is_vacant(&self) -> bool {
        !self.enabled
    }
}

impl Default for DaSection {
    fn default() -> Self {
        Self {
            enabled: false,
            accepted_single_impl: Vec::new(),
            factory_name_patterns: default_factory_name_patterns(),
            strategy_name_patterns: default_strategy_name_patterns(),
        }
    }
}

/// Seeded factory-name patterns for DA002 — the canonical "looks like a
/// constructor" naming conventions. Glob syntax (`*` at either end) per
/// [`matches_name_glob`].
pub fn default_factory_name_patterns() -> Vec<String> {
    vec![
        "create_*".into(),
        "make_*".into(),
        "*_factory".into(),
        "build_*".into(),
    ]
}

/// Seeded strategy-name patterns for DA007 — the canonical "looks like a
/// pluggable variation" enum-name conventions.
pub fn default_strategy_name_patterns() -> Vec<String> {
    vec!["*Strategy".into(), "*Mode".into(), "*Policy".into()]
}

/// Glob syntax for function/type names: `*` may appear at either end of
/// the pattern. Mirrors `module_ownership::lockfile_schema::matches_name_glob`,
/// duplicated locally so DA stays decoupled from MO.
/// - `foo` — exact name match
/// - `foo*` — name starts with `foo`
/// - `*foo` — name ends with `foo`
/// - `*foo*` — name contains `foo`
/// - `*` — anything
pub fn matches_name_glob(pattern: &str, name: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let leading = pattern.starts_with('*');
    let trailing = pattern.ends_with('*');
    let body = match (leading, trailing) {
        (true, true) if pattern.len() >= 2 => &pattern[1..pattern.len() - 1],
        (true, false) => &pattern[1..],
        (false, true) => &pattern[..pattern.len() - 1],
        (false, false) => pattern,
        _ => return false,
    };
    if body.is_empty() {
        return false;
    }
    match (leading, trailing) {
        (true, true) => name.contains(body),
        (true, false) => name.ends_with(body),
        (false, true) => name.starts_with(body),
        (false, false) => name == body,
    }
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
        // Factory/strategy pattern lists default to seeded values so the
        // opt-in flip is just `enabled = true`.
        assert!(!s.factory_name_patterns.is_empty());
        assert!(!s.strategy_name_patterns.is_empty());
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
            ..DaSection::default()
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
            ..DaSection::default()
        };
        let j = serde_json::to_value(&s).unwrap();
        let back: DaSection = serde_json::from_value(j).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn missing_fields_deserialize_to_default() {
        // Empty object → default (silent-ish) section: `enabled` false, but
        // factory/strategy seed lists populated. The seeds default to a
        // non-empty list so opting `enabled = true` is the only switch
        // users need to flip — no extra "now seed the patterns" step.
        let s: DaSection = serde_json::from_str("{}").unwrap();
        assert_eq!(s, DaSection::default());
        // Partial object — only `enabled` set, `accepted_single_impl` defaults.
        let s: DaSection = serde_json::from_str(r#"{"enabled": true}"#).unwrap();
        assert!(s.enabled);
        assert!(s.accepted_single_impl.is_empty());
        // Seeded patterns survive the partial JSON.
        assert!(!s.factory_name_patterns.is_empty());
        assert!(!s.strategy_name_patterns.is_empty());
    }

    // ---- name-glob helper ----

    #[test]
    fn name_glob_matches_prefix_and_suffix_shapes() {
        assert!(matches_name_glob("create_*", "create_widget"));
        assert!(matches_name_glob("create_*", "create_"));
        assert!(!matches_name_glob("create_*", "make_widget"));

        assert!(matches_name_glob("*_factory", "widget_factory"));
        assert!(matches_name_glob("*_factory", "_factory"));
        assert!(!matches_name_glob("*_factory", "factory_widget"));

        assert!(matches_name_glob("*Strategy", "RetryStrategy"));
        assert!(matches_name_glob("*Strategy", "Strategy"));
        assert!(!matches_name_glob("*Strategy", "StrategyRetry"));

        assert!(matches_name_glob("*", "anything"));
        assert!(!matches_name_glob("**", "anything")); // malformed
    }
}
