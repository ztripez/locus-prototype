//! Lockfile section shape for ER (Error Taxonomy Ownership).
//!
//! ER001 is heuristic and lockfile-free — the rule fires purely on the
//! "≥2 public Error types in one file" pattern.
//!
//! ER002 is the first rule with an opt-in lockfile field: a list of
//! forbidden error type patterns that must not appear as the `E` of a
//! `Result<T, E>` return. Empty default keeps the rule silent until the
//! user explicitly populates it (mirrors DG / FL onboarding UX).

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
}
