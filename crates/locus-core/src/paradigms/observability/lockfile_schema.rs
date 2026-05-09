//! Shape of the OB section inside `locus.lock`.
//!
//! Rules family OB (Observability Ownership): logs, metrics, events, and
//! audit records that represent system behaviour must use accepted owners and
//! facilities. The first rule (OB001) is the heuristic catch-all for the
//! "agent stitched in `println!` while patching" anti-pattern: raw print/dbg
//! macros bypass any structured logging facility, leak to stdout/stderr in
//! production, and are a clear sign that observability isn't owned.
//!
//! The lockfile records:
//! - `observer_paths`: module patterns where any kind of log call is
//!   legitimate (tests, examples, the CLI's user-facing output, dedicated
//!   observer modules). A file whose `module_path` matches any of these is
//!   skipped wholesale by OB001.
//! - `forbidden_log_targets`: macro path patterns considered raw/inappropriate
//!   (the `println!`/`dbg!` family by default). Anything in this list, fired
//!   from a non-observer file, trips OB001.
//! - `metric_macro_patterns` / `metric_owner_paths`: macro callee patterns
//!   that emit metrics (`metrics::counter!`, `metrics::histogram!`,
//!   `metrics::gauge!` by default) and the modules accepted as the metric
//!   owner. OB002 fires on a metric emission landing outside the owner.
//! - `event_macro_patterns` / `event_owner_paths`: matching pair for
//!   event-emission macros (`event!`, `emit!`, `publish!`,
//!   `tracing::event!` by default). OB003 fires on an event emission
//!   landing outside the owner.
//!
//! All fields default to a sensible baseline: `observer_paths` empty (the
//! user declares observer modules explicitly), `forbidden_log_targets` to the
//! print/dbg family, `metric_macro_patterns` and `event_macro_patterns` to
//! their default seeds. When the relevant pair becomes empty (e.g. the user
//! has cleared the defaults to disable OB00n) the rule short-circuits — OB
//! stays silent on un-onboarded code rather than nagging.

// locus: ot canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObSection {
    /// Module patterns matching `AirFile.module_path` for files where any
    /// kind of log call is legitimate. Pattern syntax mirrors the other
    /// paradigms: simple suffix wildcards. Examples: `"*::tests::*"`,
    /// `"tests::*"`, `"examples::*"`, `"*::cli::*"`, `"*::main"`.
    #[serde(default)]
    pub observer_paths: Vec<String>,
    /// Callee-path patterns considered raw/inappropriate logging targets.
    /// OB001 matches each `FactKind::Logging` fact's `evidence` (the
    /// loader-recorded callee, e.g. `"println"`, `"tracing::info"`) against
    /// these patterns; matches in non-observer files fire the rule.
    /// Default covers the bare `println!` / `eprintln!` / `print!` /
    /// `eprint!` / `dbg!` macro family — the structured/raw policy is a
    /// user lockfile decision, not a fact-kind taxonomy.
    #[serde(default = "default_forbidden_log_targets")]
    pub forbidden_log_targets: Vec<String>,
    /// Macro-callee patterns identifying *metric emission* sites
    /// (`metrics::counter!`, `metrics::histogram!`, `metrics::gauge!`).
    /// OB002 matches each `AirItem::CallSite` of `CallKind::Meta` against
    /// these patterns. Default is the `metrics` crate family — the user
    /// can replace it with whatever macro names their project uses.
    #[serde(default = "default_metric_macro_patterns")]
    pub metric_macro_patterns: Vec<String>,
    /// Module patterns where metric emission is the accepted owner. OB002
    /// fires when a `metric_macro_patterns` call lands outside any of
    /// these. Empty keeps OB002 silent.
    #[serde(default)]
    pub metric_owner_paths: Vec<String>,
    /// Macro-callee patterns identifying *event emission* sites — typically
    /// `event!`, `emit!`, `publish!`, or any project-specific event macro.
    /// OB003 matches each `AirItem::CallSite` of `CallKind::Meta` against
    /// these patterns.
    #[serde(default = "default_event_macro_patterns")]
    pub event_macro_patterns: Vec<String>,
    /// Module patterns where event emission is the accepted owner. OB003
    /// fires when an `event_macro_patterns` call lands outside any of
    /// these. Empty keeps OB003 silent.
    #[serde(default)]
    pub event_owner_paths: Vec<String>,
}

impl Default for ObSection {
    fn default() -> Self {
        Self {
            observer_paths: Vec::new(),
            forbidden_log_targets: default_forbidden_log_targets(),
            metric_macro_patterns: default_metric_macro_patterns(),
            metric_owner_paths: Vec::new(),
            event_macro_patterns: default_event_macro_patterns(),
            event_owner_paths: Vec::new(),
        }
    }
}

/// Default forbidden log targets: the bare `println!`/`dbg!` macro family.
/// Targets are the macro path text the visitor records on the `Log`
/// truth-action — for these the visitor records the bare 1-segment macro
/// name (no `std::` prefix), so the patterns here match that shape.
pub fn default_forbidden_log_targets() -> Vec<String> {
    vec![
        "println".to_string(),
        "eprintln".to_string(),
        "print".to_string(),
        "eprint".to_string(),
        "dbg".to_string(),
    ]
}

/// Default metric-emission macro patterns. Aligned with the `metrics`
/// crate family — the most common Rust convention. Pattern shape is the
/// rendered callee text the visitor records on `AirItem::CallSite` of
/// `CallKind::Meta`, so `metrics::counter!` is recorded as
/// `metrics::counter`.
pub fn default_metric_macro_patterns() -> Vec<String> {
    vec![
        "metrics::counter".to_string(),
        "metrics::histogram".to_string(),
        "metrics::gauge".to_string(),
    ]
}

/// Default event-emission macro patterns. Covers the common bare
/// `event!`/`emit!`/`publish!` shapes plus `tracing::event!` (the lower
/// level the higher-level `tracing::info!` etc. desugar through).
pub fn default_event_macro_patterns() -> Vec<String> {
    vec![
        "event".to_string(),
        "emit".to_string(),
        "publish".to_string(),
        "tracing::event".to_string(),
    ]
}

impl ObSection {
    /// View into the effective forbidden-log-target list. Just borrows the
    /// stored `Vec` — kept as a method so callers don't reach across the
    /// field boundary directly and so we have a single place to add policy
    /// (e.g. case-folding) later if it's ever needed.
    pub fn effective_forbidden_log_targets(&self) -> &[String] {
        &self.forbidden_log_targets
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
    fn matches_pattern_exact_and_wildcard_and_star() {
        // Exact
        assert!(matches_pattern("foo::bar", "foo::bar"));
        assert!(!matches_pattern("foo::bar", "foo::bar::baz"));
        assert!(!matches_pattern("foo::bar", "foo"));
        // Suffix wildcard
        assert!(matches_pattern("foo::*", "foo"));
        assert!(matches_pattern("foo::*", "foo::bar"));
        assert!(matches_pattern("foo::*", "foo::bar::baz"));
        assert!(!matches_pattern("foo::*", "foobar"));
        // Bare star
        assert!(matches_pattern("*", ""));
        assert!(matches_pattern("*", "anything::nested"));
    }

    #[test]
    fn default_forbidden_log_targets_covers_print_dbg_family() {
        let defaults = default_forbidden_log_targets();
        for expected in ["println", "eprintln", "print", "eprint", "dbg"] {
            assert!(
                defaults.iter().any(|t| t == expected),
                "default forbidden targets missing `{expected}`: {defaults:?}"
            );
        }
        // No `tracing::*` / `log::*` in the defaults — structured facilities
        // are explicitly NOT default-forbidden.
        assert!(!defaults.iter().any(|t| t.starts_with("tracing")));
        assert!(!defaults.iter().any(|t| t.starts_with("log::")));
    }

    #[test]
    fn default_section_uses_default_forbidden_targets_and_empty_observers() {
        let section = ObSection::default();
        assert!(section.observer_paths.is_empty());
        assert_eq!(
            section.effective_forbidden_log_targets(),
            default_forbidden_log_targets().as_slice()
        );
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
    fn effective_forbidden_log_targets_reflects_user_overrides() {
        let section = ObSection {
            observer_paths: vec!["tests::*".into()],
            forbidden_log_targets: vec!["tracing::info".into()],
            ..ObSection::default()
        };
        assert_eq!(
            section.effective_forbidden_log_targets(),
            &["tracing::info".to_string()]
        );
    }
}
