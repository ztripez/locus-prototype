//! Shape of the CF section inside `locus.lock`.
//!
//! Rules family CF (Config/Data Ownership): behavior-shaping decision data
//! must live in an accepted config owner. CF001 — the first rule landing —
//! flags environment-variable reads from outside that owner. The lockfile
//! records which module paths *are* the config layer (`config_paths`); reads
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

    /// Filename globs used by [`CF002`](super::rules::cf002) to detect
    /// stray config-shaped files (`.yaml`/`.toml`/`.json`/`.yml`) outside
    /// any accepted location. **Reserved**: CF002 itself is not yet
    /// implemented — Locus rules consume `AirWorkspace`, not the
    /// filesystem, so the rule body is deferred until a filesystem-aware
    /// loader lands. Lockfile field is here so users can pre-populate.
    #[serde(default = "default_config_file_patterns")]
    pub config_file_patterns: Vec<String>,

    /// Path globs whose matches are exempt from CF002 — files Locus
    /// should always treat as "expected" config (e.g. `Cargo.toml`,
    /// `rust-toolchain.toml`, `.github/**/*`). **Reserved**: see
    /// `config_file_patterns`. Pre-populating today preserves the
    /// allowlist for the future filesystem-walk rule.
    #[serde(default = "default_accepted_config_files")]
    pub accepted_config_files: Vec<String>,
}

impl Default for CfSection {
    fn default() -> Self {
        Self {
            config_paths: Vec::new(),
            config_file_patterns: default_config_file_patterns(),
            accepted_config_files: default_accepted_config_files(),
        }
    }
}

/// Seeded filename globs covering the four common config formats. Used
/// today only as a pre-population default for the (future)
/// filesystem-walk rule [`CF002`](super::rules::cf002).
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
}
