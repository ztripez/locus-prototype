//! Shape of the CF section inside `locus.lock`.
//!
//! Rules family CF (Config/Data Ownership): behavior-shaping decision data
//! must live in an accepted config owner. CF001 flags environment-variable
//! reads from outside that owner. CF002 flags magic decision constants
//! (literal values used as match scrutinees or `==`/`!=` comparison
//! targets) outside the config layer. CF003 flags hardcoded
//! provider/model/topic IDs (matching a user-declared pattern allowlist)
//! outside the config layer. The lockfile records which module paths *are*
//! the config layer (`config_paths`); reads / decision constants / IDs
//! anywhere else are violations.
//!
//! `config_paths` defaults empty: CF is silent until the user declares the
//! config layer — same lockfile-driven posture as DG, UT, and BO.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CfSection {
    /// Module patterns identifying files that legitimately read configuration
    /// (env vars, config files, secret stores). Matched against
    /// `AirFile.module_path`. Examples: `"crate::config::*"`,
    /// `"crate::settings::*"`, `"crate::main"`.
    #[serde(default)]
    pub config_paths: Vec<String>,

    /// Filename globs reserved for a future filesystem-aware variant of
    /// CF-family rules — e.g. detecting stray config-shaped files
    /// (`.yaml`/`.toml`/`.json`/`.yml`) outside any accepted location.
    /// CF002 used to be the placeholder for that filesystem-walk; the
    /// rule body now targets magic decision constants in
    /// `AirItem::ScrutineeLiteral`s instead. These fields stay so the
    /// allowlist survives if/when a filesystem-aware loader lands.
    #[serde(default = "default_config_file_patterns")]
    pub config_file_patterns: Vec<String>,

    /// Path globs whose matches would be exempt from a future
    /// filesystem-aware variant — files Locus should always treat as
    /// "expected" config (e.g. `Cargo.toml`, `rust-toolchain.toml`,
    /// `.github/**/*`). See `config_file_patterns` for the deferred
    /// filesystem-walk concept.
    #[serde(default = "default_accepted_config_files")]
    pub accepted_config_files: Vec<String>,

    /// Literal kinds CF002 treats as suspect when used as a decision
    /// scrutinee (match arm pattern or `==`/`!=` RHS). Defaults to
    /// `["str", "int", "float"]`; users can narrow to `["str"]` if
    /// integer thresholds are too noisy, or empty the list to
    /// effectively disable CF002 without touching `config_paths`.
    /// `bool` is intentionally omitted from the default — `if x ==
    /// true` is noise, not a magic decision constant.
    #[serde(default = "default_forbidden_literal_kinds")]
    pub forbidden_literal_kinds: Vec<String>,

    /// Glob patterns matched against the (unquoted) value of every
    /// string-kind `AirItem::ScrutineeLiteral` outside `config_paths`.
    /// Drives [`CF003`](super::rules::cf003) — hardcoded
    /// provider/model/topic IDs. Defaults empty: CF003 stays silent
    /// until the user declares the ID shapes they want to police.
    /// Examples: `["gpt-*", "claude-*", "openai/*", "anthropic/*",
    /// "topic-*", "queue-*"]`.
    #[serde(default)]
    pub forbidden_id_patterns: Vec<String>,
}

impl Default for CfSection {
    fn default() -> Self {
        Self {
            config_paths: Vec::new(),
            config_file_patterns: default_config_file_patterns(),
            accepted_config_files: default_accepted_config_files(),
            forbidden_literal_kinds: default_forbidden_literal_kinds(),
            forbidden_id_patterns: Vec::new(),
        }
    }
}

/// Seeded filename globs covering the four common config formats. Used
/// today only as a pre-population default for a future filesystem-aware
/// CF rule. (CF002 used to be the placeholder for that filesystem-walk;
/// it now targets magic decision constants in `AirItem::ScrutineeLiteral`
/// instead.)
pub fn default_config_file_patterns() -> Vec<String> {
    vec![
        "*.yaml".into(),
        "*.yml".into(),
        "*.toml".into(),
        "*.json".into(),
    ]
}

/// Seeded path globs covering files that are expected to live in a Rust
/// repository's tree (Cargo, toolchain, GitHub Actions, examples).
/// Reserved for the future filesystem-walk rule.
pub fn default_accepted_config_files() -> Vec<String> {
    vec![
        "Cargo.toml".into(),
        "Cargo.lock".into(),
        "rust-toolchain.toml".into(),
        ".github/**/*".into(),
        "examples/**/*".into(),
    ]
}

/// Default literal kinds CF002 treats as suspect: `str`, `int`,
/// `float`. `bool` is omitted — booleans in `if x == true` patterns are
/// noise, not magic decision constants. Users can narrow further (e.g.
/// `["str"]` only) or clear the list to disable CF002.
pub fn default_forbidden_literal_kinds() -> Vec<String> {
    vec!["str".into(), "int".into(), "float".into()]
}

/// Pattern syntax: segment-aligned wildcards plus a fall-through
/// character-glob for non-`::` strings (used by CF003 to match
/// dash-shaped IDs like `gpt-*`).
///
/// Segment-aligned (rooted in `::` separators):
/// - `foo::bar` — exact match
/// - `foo::*` — `foo` itself or any descendant (`foo::bar`, `foo::bar::baz`)
/// - `*::foo` — `foo` itself or anywhere ending with `::foo` (`a::foo`,
///   `a::b::foo`)
/// - `*::foo::*` — `foo` appearing as any whole segment in the path
///   (`foo`, `a::foo`, `a::foo::b`, `a::b::foo::c`)
/// - `*` — anything
///
/// Character-glob fallback (when a single `*` appears at the start or
/// end of a pattern that does NOT use `::` segments):
/// - `gpt-*` — any string starting with `gpt-`
/// - `*-events` — any string ending with `-events`
/// - `*foo*` — any string containing `foo`
///
/// CF003's `forbidden_id_patterns` use the character-glob form against
/// the (unquoted) value of string scrutinee literals. CF002's
/// `config_paths` gating uses the segment-aligned form against module
/// paths.
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
    if leading_wild || trailing_wild {
        if stripped.is_empty() {
            // Pattern was just `*::` or `::*` — treat as a malformed
            // wildcard rather than matching anything; callers
            // configuring these would have meant `*`.
            return false;
        }
        return match (leading_wild, trailing_wild) {
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
            (false, false) => unreachable!(),
        };
    }
    // Character-glob fall-through. No `::` segment wildcards in the
    // pattern, so a leading or trailing `*` (or both) is treated as a
    // generic prefix / suffix / contains glob. Mid-pattern `*`s aren't
    // supported — use multiple patterns.
    let leading_char_wild = pattern.starts_with('*');
    let trailing_char_wild = pattern.ends_with('*');
    if !leading_char_wild && !trailing_char_wild {
        return pattern == path;
    }
    let inner = match (leading_char_wild, trailing_char_wild) {
        (true, true) => &pattern[1..pattern.len().saturating_sub(1)],
        (true, false) => &pattern[1..],
        (false, true) => &pattern[..pattern.len() - 1],
        (false, false) => unreachable!(),
    };
    if inner.is_empty() {
        // Pattern was `*` (handled earlier) or `**`; treat the latter
        // as a malformed wildcard.
        return false;
    }
    match (leading_char_wild, trailing_char_wild) {
        (true, true) => path.contains(inner),
        (true, false) => path.ends_with(inner),
        (false, true) => path.starts_with(inner),
        (false, false) => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match() {
        assert!(matches_pattern("crate::config", "crate::config"));
        assert!(!matches_pattern("crate::config", "crate::config::loader"));
        assert!(!matches_pattern("crate::config", "crate"));
    }

    #[test]
    fn suffix_wildcard_includes_the_prefix_and_descendants() {
        assert!(matches_pattern("crate::config::*", "crate::config"));
        assert!(matches_pattern("crate::config::*", "crate::config::loader"));
        assert!(matches_pattern(
            "crate::config::*",
            "crate::config::env::reader"
        ));
        assert!(!matches_pattern("crate::config::*", "crate::configurator"));
        assert!(!matches_pattern("crate::config::*", "crate::handler"));
    }

    #[test]
    fn star_matches_anything() {
        assert!(matches_pattern("*", ""));
        assert!(matches_pattern("*", "crate::handler"));
        assert!(matches_pattern("*", "crate::config::loader"));
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

    // ---- Character-glob fall-through (CF003 ID patterns) ----

    #[test]
    fn char_glob_trailing_wildcard_matches_prefix() {
        assert!(matches_pattern("gpt-*", "gpt-4"));
        assert!(matches_pattern("gpt-*", "gpt-4o"));
        assert!(matches_pattern("gpt-*", "gpt-"));
        assert!(!matches_pattern("gpt-*", "claude-4"));
        assert!(!matches_pattern("gpt-*", "ggpt-4"));
    }

    #[test]
    fn char_glob_leading_wildcard_matches_suffix() {
        assert!(matches_pattern("*-events", "queue-events"));
        assert!(matches_pattern("*-events", "user-events"));
        assert!(!matches_pattern("*-events", "events-log"));
    }

    #[test]
    fn char_glob_both_ends_matches_contains() {
        assert!(matches_pattern("*topic*", "topic-foo"));
        assert!(matches_pattern("*topic*", "foo-topic"));
        assert!(matches_pattern("*topic*", "foo-topic-bar"));
        assert!(matches_pattern("*topic*", "topic"));
        assert!(!matches_pattern("*topic*", "tropic"));
    }

    #[test]
    fn char_glob_double_star_does_not_match_anything() {
        // `**` is not part of the supported syntax — treat as malformed
        // rather than matching everything.
        assert!(!matches_pattern("**", "anything"));
    }
}
