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
//!
//! Both fields default to a sensible baseline: `observer_paths` empty (the
//! user declares observer modules explicitly), `forbidden_log_targets` to the
//! print/dbg family. When both end up empty (e.g. the user has explicitly
//! cleared the defaults to disable OB001) the rule short-circuits — OB stays
//! silent on un-onboarded code rather than nagging.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObSection {
    /// Module patterns matching `AirFile.module_path` for files where any
    /// kind of log call is legitimate. Pattern syntax mirrors the other
    /// paradigms: simple suffix wildcards. Examples: `"*::tests::*"`,
    /// `"tests::*"`, `"examples::*"`, `"*::cli::*"`, `"*::main"`.
    #[serde(default)]
    pub observer_paths: Vec<String>,
    /// Macro path patterns considered raw/inappropriate — anything matched is
    /// flagged by OB001 when called from a non-observer file. Defaults to the
    /// `println!`/`dbg!` family. Users can extend (e.g. ban a noisy custom
    /// macro) or shrink (e.g. allow `dbg!` during a hot debug session) the
    /// list.
    #[serde(default = "default_forbidden_log_targets")]
    pub forbidden_log_targets: Vec<String>,
}

impl Default for ObSection {
    fn default() -> Self {
        Self {
            observer_paths: Vec::new(),
            forbidden_log_targets: default_forbidden_log_targets(),
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

impl ObSection {
    /// View into the effective forbidden-log-target list. Just borrows the
    /// stored `Vec` — kept as a method so callers don't reach across the
    /// field boundary directly and so we have a single place to add policy
    /// (e.g. case-folding) later if it's ever needed.
    pub fn effective_forbidden_log_targets(&self) -> &[String] {
        &self.forbidden_log_targets
    }
}

/// Pattern syntax: simple suffix wildcard, mirroring DG / UT / BO.
/// - `foo::bar` — exact match
/// - `foo::*` — `foo` itself or any descendant (`foo::bar`, `foo::bar::baz`)
/// - `*` — anything
///
/// More expressive forms (`*::tests::*`, regex, segment filters) are deferred
/// until the simple shape proves insufficient. Duplicated locally so the OB
/// paradigm doesn't depend on DG / UT / BO.
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
    fn effective_forbidden_log_targets_reflects_user_overrides() {
        let section = ObSection {
            observer_paths: vec!["tests::*".into()],
            forbidden_log_targets: vec!["tracing::info".into()],
        };
        assert_eq!(
            section.effective_forbidden_log_targets(),
            &["tracing::info".to_string()]
        );
    }
}
