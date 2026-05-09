//! Shape of the RW section inside `locus.lock`.
//!
//! Rules family RW (Runtime Work Ownership): spawning tasks, threads, jobs,
//! or background work belongs in declared *runtime owner* modules — job
//! queues, orchestrators, supervisors, runtime entry points — not scattered
//! across handlers, services, or feature modules. The lockfile records which
//! module paths are accepted runtime owners (`runtime_owner_paths`); RW001
//! fires on any spawn-shaped action observed outside them.
//!
//! No paths are inferred at `init` time: runtime-owner status is a user
//! assertion, not a guess. An empty `runtime_owner_paths` keeps the rule
//! silent — same lockfile-driven posture as CR/DG/UT.

// locus: ot canonical

use serde::{Deserialize, Serialize};

/// Default seed for [`RwSection::runtime_state_type_patterns`]. These are
/// **type-text fragments** (not module-path patterns): a trailing `*` means
/// "anything else may follow". Matched via [`type_text_matches`].
pub const DEFAULT_RUNTIME_STATE_TYPE_PATTERNS: &[&str] = &[
    "Mutex<*",
    "RwLock<*",
    "Arc<Mutex<*",
    "Arc<RwLock<*",
    "OnceCell<*",
    "OnceLock<*",
];

/// Default seed for [`RwSection::singleton_name_patterns`]. Trailing-`*`
/// fragments matched against an `AirType.name` via [`type_text_matches`] —
/// e.g. `*Singleton` flags any name ending in `Singleton`.
pub const DEFAULT_SINGLETON_NAME_PATTERNS: &[&str] = &["*Singleton", "*Globals"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RwSection {
    /// Module patterns matching `AirFile.module_path` for files declared as
    /// runtime owners — the only places where direct task/thread spawning is
    /// legitimate. Pattern syntax mirrors CR/DG/UT: simple suffix wildcards
    /// (e.g. `bin::*`, `crate::runtime::*`, `crate::worker::*`,
    /// `crate::orchestrator`).
    #[serde(default)]
    pub runtime_owner_paths: Vec<String>,
    /// Type-text fragments (NOT module-path patterns) used by RW003 to
    /// recognise runtime-state-shaped fields on types. Trailing `*` is
    /// treated as a wildcard suffix — `Mutex<*` matches `Mutex<u64>`,
    /// `Mutex<MyCfg>`, etc. Defaults to
    /// [`DEFAULT_RUNTIME_STATE_TYPE_PATTERNS`].
    #[serde(default = "default_runtime_state_type_patterns")]
    pub runtime_state_type_patterns: Vec<String>,
    /// Type-name fragments used by RW004 to recognise singleton-shaped
    /// types (regardless of their fields). Leading `*` is treated as a
    /// wildcard prefix — `*Singleton` matches `AppSingleton`,
    /// `MetricsSingleton`. Defaults to
    /// [`DEFAULT_SINGLETON_NAME_PATTERNS`].
    #[serde(default = "default_singleton_name_patterns")]
    pub singleton_name_patterns: Vec<String>,
}

impl Default for RwSection {
    fn default() -> Self {
        Self {
            runtime_owner_paths: Vec::new(),
            runtime_state_type_patterns: default_runtime_state_type_patterns(),
            singleton_name_patterns: default_singleton_name_patterns(),
        }
    }
}

impl RwSection {
    /// True when the user hasn't declared any runtime owners — RW001/002/
    /// 003/004 all need that declaration. RW005/006 use the marker
    /// mechanism (`// locus: fact hot_path`) which is independent of this
    /// section, so the paradigm-level vacancy diagnostic specifically
    /// targets the lockfile-driven rules.
    pub fn is_vacant(&self) -> bool {
        self.runtime_owner_paths.is_empty()
    }
}

fn default_runtime_state_type_patterns() -> Vec<String> {
    DEFAULT_RUNTIME_STATE_TYPE_PATTERNS
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

fn default_singleton_name_patterns() -> Vec<String> {
    DEFAULT_SINGLETON_NAME_PATTERNS
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

/// Match a type-text fragment pattern against a type-text string.
///
/// Distinct from [`matches_pattern`] — this is **NOT** module-path matching.
/// Patterns are short fragments of rendered Rust type text; the only
/// supported wildcard is a single trailing `*` meaning "anything else may
/// follow" (`Mutex<*` matches `Mutex<u64>`), or a single leading `*`
/// meaning "anything before" (`*Singleton` matches `AppSingleton`). A
/// pattern with neither wildcard requires equality.
pub fn type_text_matches(pattern: &str, text: &str) -> bool {
    let leading = pattern.starts_with('*');
    let trailing = pattern.ends_with('*');
    if leading && trailing && pattern.len() >= 2 {
        let mid = &pattern[1..pattern.len() - 1];
        if mid.is_empty() {
            return true; // bare `**`
        }
        return text.contains(mid);
    }
    if trailing {
        let prefix = &pattern[..pattern.len() - 1];
        return text.starts_with(prefix);
    }
    if leading {
        let suffix = &pattern[1..];
        return text.ends_with(suffix);
    }
    pattern == text
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
    fn matches_pattern_exact() {
        assert!(matches_pattern("crate::runtime", "crate::runtime"));
        assert!(!matches_pattern("crate::runtime", "crate::runtime::pool"));
        assert!(!matches_pattern("crate::runtime", "crate"));
    }

    #[test]
    fn matches_pattern_suffix_wildcard_includes_prefix_and_descendants() {
        assert!(matches_pattern("crate::runtime::*", "crate::runtime"));
        assert!(matches_pattern("crate::runtime::*", "crate::runtime::pool"));
        assert!(matches_pattern(
            "crate::runtime::*",
            "crate::runtime::pool::worker"
        ));
        assert!(!matches_pattern("crate::runtime::*", "crate::runtimer"));
        assert!(!matches_pattern("crate::runtime::*", "crate::other"));
    }

    #[test]
    fn matches_pattern_star_matches_anything() {
        assert!(matches_pattern("*", ""));
        assert!(matches_pattern("*", "crate::handler"));
        assert!(matches_pattern("*", "anything::nested::module"));
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

    #[test]
    fn type_text_matches_trailing_star_is_prefix() {
        assert!(type_text_matches("Mutex<*", "Mutex<u64>"));
        assert!(type_text_matches("Mutex<*", "Mutex<MyState>"));
        assert!(type_text_matches("Arc<Mutex<*", "Arc<Mutex<HashMap<K,V>>>"));
        assert!(!type_text_matches("Mutex<*", "RwLock<u64>"));
        // Trailing-* is a prefix match — no segment alignment.
        assert!(type_text_matches("Mut*", "Mutex<u64>"));
    }

    #[test]
    fn type_text_matches_leading_star_is_suffix() {
        assert!(type_text_matches("*Singleton", "AppSingleton"));
        assert!(type_text_matches("*Singleton", "MetricsSingleton"));
        assert!(!type_text_matches("*Singleton", "SingletonAdapter"));
    }

    #[test]
    fn type_text_matches_no_wildcard_requires_equality() {
        assert!(type_text_matches("()", "()"));
        assert!(!type_text_matches("()", "(u32)"));
    }

    #[test]
    fn rw_section_default_seeds_type_patterns() {
        let section = RwSection::default();
        assert!(section.runtime_owner_paths.is_empty());
        assert!(
            section
                .runtime_state_type_patterns
                .iter()
                .any(|p| p == "Mutex<*"),
            "default Mutex pattern missing; got {:?}",
            section.runtime_state_type_patterns
        );
        assert!(
            section
                .singleton_name_patterns
                .iter()
                .any(|p| p == "*Singleton"),
            "default Singleton pattern missing; got {:?}",
            section.singleton_name_patterns
        );
    }
}
