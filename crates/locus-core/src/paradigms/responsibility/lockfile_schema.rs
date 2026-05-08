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

/// Default cap for `StringCompare` + `EnumMatch` actions inside a single
/// handler function before RM003 fires. Three permits a small dispatch table
/// without normalising into a fully-fledged domain policy module.
pub const DEFAULT_MAX_HANDLER_DECISIONS: u32 = 3;

/// Default cap for `StringCompare` + `EnumMatch` actions inside a single
/// repository function before RM004 fires. Same reasoning as
/// [`DEFAULT_MAX_HANDLER_DECISIONS`] — three branches feel like incidental
/// guards, more start to look like domain policy hiding inside data access.
pub const DEFAULT_MAX_REPOSITORY_DECISIONS: u32 = 3;

impl RmSection {
    /// True when the user hasn't declared any per-rule scope. RM001 needs
    /// `default_max_action_kinds`, RM002 needs `converter_paths`, RM003
    /// needs `handler_paths`, RM004 needs `repository_paths`, RM005
    /// needs `validator_paths`, RM006 needs `domain_paths_rm`. None of
    /// these have built-in defaults that would let any rule fire — RM
    /// is fully vacant when all are empty / `None`.
    pub fn is_vacant(&self) -> bool {
        self.default_max_action_kinds.is_none()
            && self.converter_paths.is_empty()
            && self.handler_paths.is_empty()
            && self.repository_paths.is_empty()
            && self.validator_paths.is_empty()
            && self.domain_paths_rm.is_empty()
    }
}

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
    /// Module patterns matching `AirFile.module_path` for files whose
    /// functions are *converters* — pure mapping between data shapes. Any
    /// side-effect fact (`SpawnedWork`, `Logging`, `ConfigRead`) targeting
    /// a function in one of these modules is RM002. Same suffix-wildcard
    /// syntax as DG/UT — `foo::bar`, `foo::*`, `*`. Empty default keeps
    /// RM002 silent until the user opts in.
    #[serde(default)]
    pub converter_paths: Vec<String>,
    /// Module patterns matching `AirFile.module_path` for files declared as
    /// *handlers* — orchestration entry points (HTTP/RPC/CLI handlers, job
    /// dispatchers, …). Functions in these modules should orchestrate, not
    /// embed branch-rich domain policy. RM003 fires when a handler function
    /// hosts more than [`RmSection::effective_max_handler_decisions`]
    /// `StringCompare` + `EnumMatch` actions. Empty default keeps RM003 silent.
    #[serde(default)]
    pub handler_paths: Vec<String>,
    /// Per-function cap on `StringCompare` + `EnumMatch` actions inside a
    /// handler module before RM003 fires. `None` falls back to
    /// [`DEFAULT_MAX_HANDLER_DECISIONS`].
    #[serde(default)]
    pub max_handler_decisions: Option<u32>,
    /// Module patterns matching `AirFile.module_path` for files declared as
    /// *repositories* — persistence-adjacent gateways. Functions in these
    /// modules should be thin queries, not branch-rich domain logic. RM004
    /// fires when a repository function hosts more than
    /// [`RmSection::effective_max_repository_decisions`] `StringCompare` +
    /// `EnumMatch` actions. Empty default keeps RM004 silent.
    #[serde(default)]
    pub repository_paths: Vec<String>,
    /// Per-function cap on `StringCompare` + `EnumMatch` actions inside a
    /// repository module before RM004 fires. `None` falls back to
    /// [`DEFAULT_MAX_REPOSITORY_DECISIONS`].
    #[serde(default)]
    pub max_repository_decisions: Option<u32>,
    /// Module patterns for files whose functions are *validators* — pure
    /// decision functions that should never themselves perform IO. RM005
    /// fires when an `ExternalIo` or `PersistenceWrite` fact targets a
    /// function in one of these modules. Same suffix-wildcard syntax as
    /// the rest of RM. Empty default keeps RM005 silent.
    #[serde(default)]
    pub validator_paths: Vec<String>,
    /// Module patterns for files whose types are *domain types* (in the
    /// RM sense — kept separate from BO's `domain_paths` to keep paradigms
    /// independent). RM006 fires when a `PersistenceWrite` fact targets a
    /// method (a function symbol containing `::TypeName::`) defined in
    /// one of these modules. Same suffix-wildcard syntax as the rest of
    /// RM. Empty default keeps RM006 silent.
    #[serde(default)]
    pub domain_paths_rm: Vec<String>,
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

    /// Resolve the active RM003 cap. Falls back to
    /// [`DEFAULT_MAX_HANDLER_DECISIONS`] when the user populated
    /// `handler_paths` without pinning a count. Callers should still gate on
    /// `handler_paths` being non-empty before invoking the rule — that's the
    /// opt-in signal.
    pub fn effective_max_handler_decisions(&self) -> u32 {
        self.max_handler_decisions
            .unwrap_or(DEFAULT_MAX_HANDLER_DECISIONS)
    }

    /// Resolve the active RM004 cap. Falls back to
    /// [`DEFAULT_MAX_REPOSITORY_DECISIONS`] when the user populated
    /// `repository_paths` without pinning a count.
    pub fn effective_max_repository_decisions(&self) -> u32 {
        self.max_repository_decisions
            .unwrap_or(DEFAULT_MAX_REPOSITORY_DECISIONS)
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
    fn effective_default_falls_back_when_unset() {
        let s = RmSection::default();
        assert_eq!(s.effective_default(), DEFAULT_MAX_ACTION_KINDS);
    }

    #[test]
    fn effective_default_honors_explicit_cap() {
        let s = RmSection {
            default_max_action_kinds: Some(4),
            exempt_paths: Vec::new(),
            converter_paths: Vec::new(),
            ..Default::default()
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
