//! Lockfile section shape for ER (Error Taxonomy Ownership).
//!
//! ER001 is heuristic and lockfile-free — the rule fires purely on the
//! "≥2 public Error types in one file" pattern.
//!
//! ER002 is the first rule with an opt-in lockfile field: a list of
//! forbidden error type patterns that must not appear as the `E` of a
//! `Result<T, E>` return. Empty default keeps the rule silent until the
//! user explicitly populates it (mirrors DG / FL onboarding UX).
//!
//! ER003 adds two more opt-in lists: `domain_paths` (module patterns
//! marking files whose *enum variants' field types* must not be boundary
//! errors) and `boundary_error_patterns` (the same boundary type shapes
//! FL001 names, repeated here so the ER section stands alone). Both
//! default to empty — ER003 stays silent until both are populated.
//!
//! ER005 is lockfile-driven via `error_collapse_owner_paths` — the list
//! of modules where a catch-all `Err(_) => default` arm is legitimate
//! (top-level error handlers, presentation/edge layers). Empty default
//! keeps ER005 silent until populated.
//!
//! ER007 is heuristic and lockfile-free — duplicate variant names across
//! `*Error*` enums are flagged via a workspace-wide pass.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ErSection {
    /// Patterns matching the `E` in a function's `Result<T, E>` return when
    /// `E` is a "string-shaped" or otherwise too-loose error type that
    /// collapses the project's error taxonomy.
    ///
    /// Pattern syntax: a single `*` wildcard somewhere in the pattern. The
    /// matcher splits on the `*` and accepts any input that starts with the
    /// prefix and ends with the suffix. Patterns without `*` match exactly.
    /// Examples (recommended user shapes — none are seeded by default):
    ///
    /// - `"String"`, `"&str"` — bare-string error returns.
    /// - `"Box<dyn Error>"`, `"Box<dyn std::error::Error>"` — type-erased
    ///   `dyn Error` (use `"Box<dyn *>"` to catch any `Box<dyn …>`).
    /// - `"anyhow::Error"`, `"eyre::Report"` — third-party catch-alls
    ///   (use `"anyhow::*"` / `"eyre::*"` to net any sub-path).
    /// - `"*::Error"` — anything whose last path segment is `Error`
    ///   (very broad; usually only useful as an exploratory check).
    ///
    /// Match is performed against the **trimmed** error-type text — leading
    /// whitespace and a single leading `&` are stripped before matching, so
    /// `"&str"` patterns line up with `Result<_, &str>` and reference-typed
    /// errors don't slip past `String`-shaped patterns.
    ///
    /// Default: empty. ER002 stays silent until populated.
    #[serde(default)]
    pub forbidden_error_types: Vec<String>,

    /// Module patterns matching `AirFile.module_path` for files declared as
    /// "domain" — i.e. files whose enum-variant fields must not embed a
    /// boundary error type. Pattern syntax mirrors FL/DG (segment-aligned
    /// wildcards via [`matches_pattern`]).
    ///
    /// Used by ER003 alongside [`Self::boundary_error_patterns`]. ER003
    /// stays silent until both lists are non-empty.
    ///
    /// Default: empty.
    #[serde(default)]
    pub domain_paths: Vec<String>,

    /// Patterns matching boundary / transport error type names that must
    /// not appear as a *field type* on an enum variant declared in a
    /// domain module. Same matcher style as FL001 / DG001.
    ///
    /// Recommended starter set when populating:
    /// `["reqwest::Error", "sqlx::Error", "http::*", "std::io::Error"]`.
    ///
    /// Default: empty. ER003 stays silent until populated.
    #[serde(default)]
    pub boundary_error_patterns: Vec<String>,

    /// Module patterns matching `AirFile.module_path` (or the enclosing
    /// function's containing module) where catch-all `Err(_) => default`
    /// match arms are legitimate. Typical entries: top-level error
    /// handlers, presentation/edge layers, CLI surfaces — anywhere
    /// collapsing distinct error variants into a single value is the
    /// intentional design (the layer is meant to flatten the taxonomy
    /// before responding to the user).
    ///
    /// Pattern syntax: segment-aligned wildcards via [`matches_pattern`]
    /// (same shape FL/DG use). Examples:
    ///
    /// - `"*::cli::*"` — anywhere under a `cli` module segment.
    /// - `"my_app::presentation::*"` — a specific presentation layer.
    /// - `"*::tests::*"` — test modules where collapse is fine.
    ///
    /// Used by ER005. Default: empty. ER005 stays silent until populated.
    #[serde(default)]
    pub error_collapse_owner_paths: Vec<String>,
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
/// each paradigm's matcher can evolve independently. The shape mirrors
/// the FL / DG matchers verbatim.
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
    fn matches_pattern_exact_and_wildcards() {
        assert!(matches_pattern("foo::bar", "foo::bar"));
        assert!(!matches_pattern("foo::bar", "foo::bar::baz"));
        assert!(matches_pattern("foo::*", "foo::bar::baz"));
        assert!(matches_pattern("*::Error", "std::io::Error"));
        assert!(!matches_pattern("*::Error", "MyError"));
        assert!(matches_pattern("*::tests::*", "a::tests::b"));
        assert!(matches_pattern("*", "anything"));
    }

    #[test]
    fn matches_pattern_rejects_malformed_wildcards() {
        assert!(!matches_pattern("*::", "anything"));
        assert!(!matches_pattern("::*", "anything"));
    }
}
