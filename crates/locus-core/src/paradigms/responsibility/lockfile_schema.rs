//! Shape of the RM section inside `locus.lock`.
//!
//! Rules family RM (Responsibility Mixing): the lockfile records a per-function
//! cap on the number of *distinct* `ActionKind` values an `AirTruthAction`
//! enclosing function may produce, plus a list of module patterns whose
//! functions are exempt from the check (test scaffolding, `main` wiring, etc).
//!
//! The cap is opt-in: when `default_max_action_kinds` is `None`, the entire
//! rule stays silent — same UX as DG/UT lockfile-driven rules. Once the user
//! sets it (typically to [`DEFAULT_MAX_ACTION_KINDS`]), RM001 fires on any
//! function whose body mixes more than that many distinct kinds of work.

// ot: canonical

use serde::{Deserialize, Serialize};

/// Default per-function cap when `default_max_action_kinds` is set without
/// an explicit value via [`RmSection::effective_default`]. Two means a
/// function may freely mix construction with one of {validate, normalize,
/// enum-match, string-compare}, but not three or more.
pub const DEFAULT_MAX_ACTION_KINDS: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct RmSection {
    /// Maximum number of distinct `ActionKind` values a single function's
    /// `AirTruthAction` entries may produce. `None` keeps RM001 silent.
    #[serde(default)]
    pub default_max_action_kinds: Option<u32>,
    /// Module patterns matching `AirFile.module_path` for files whose
    /// functions are exempt from RM checks. Same suffix-wildcard syntax as
    /// DG/UT — `foo::bar`, `foo::*`, `*`.
    #[serde(default)]
    pub exempt_paths: Vec<String>,
}

impl RmSection {
    /// Resolve the active cap. Falls back to [`DEFAULT_MAX_ACTION_KINDS`] when
    /// the user opted in but did not pin a value. Callers should still gate on
    /// `default_max_action_kinds.is_some()` first if they want the rule to be
    /// silent in the un-configured case.
    pub fn effective_default(&self) -> u32 {
        self.default_max_action_kinds
            .unwrap_or(DEFAULT_MAX_ACTION_KINDS)
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
    fn effective_default_falls_back_when_unset() {
        let s = RmSection::default();
        assert_eq!(s.effective_default(), DEFAULT_MAX_ACTION_KINDS);
    }

    #[test]
    fn effective_default_honors_explicit_cap() {
        let s = RmSection {
            default_max_action_kinds: Some(4),
            exempt_paths: Vec::new(),
        };
        assert_eq!(s.effective_default(), 4);
    }

    #[test]
    fn matches_pattern_exact_and_suffix_wildcard() {
        assert!(matches_pattern("foo::bar", "foo::bar"));
        assert!(!matches_pattern("foo::bar", "foo::bar::baz"));
        assert!(matches_pattern("foo::*", "foo"));
        assert!(matches_pattern("foo::*", "foo::bar::baz"));
        assert!(!matches_pattern("foo::*", "foobar"));
        assert!(matches_pattern("*", "anything::nested"));
    }
}
