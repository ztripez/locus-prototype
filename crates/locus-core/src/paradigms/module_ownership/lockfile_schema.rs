//! Lockfile section shape for MO (Module / File Ownership).
//!
//! MO records per-module budgets for "how much public surface a single file
//! is allowed to own." A file with too many public top-level types is
//! probably mixing responsibilities (MO001). The default budget is
//! workspace-wide; specific module patterns can raise or lower it (API
//! surfaces legitimately have many public types; domain modules usually
//! shouldn't).

// ot: canonical

use serde::{Deserialize, Serialize};

/// Default budget for `default_max_public_types` when none is set in the
/// lockfile. Five is a deliberate "small but not trivial" threshold — most
/// well-factored modules sit at one or two public types; six begins to feel
/// like a god module unless the file is an explicit API surface.
pub const DEFAULT_MAX_PUBLIC_TYPES: u32 = 5;

/// Default entropy threshold for MO002 — a file carrying this many distinct
/// architectural roles is a "responsibility blob." Three is intentionally
/// low: a single file legitimately owning a canonical type, plus its
/// converters, plus a handler is exactly the shape MO002 wants to flag.
pub const DEFAULT_ENTROPY_THRESHOLD: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct MoSection {
    /// Workspace-wide budget for the number of `pub` top-level types per
    /// file. `None` means "fall back to [`DEFAULT_MAX_PUBLIC_TYPES`]" — we
    /// keep this `Option` so `MoSection::default()` is *empty* (no fields
    /// configured) rather than carrying a magic number into round-trips.
    #[serde(default)]
    pub default_max_public_types: Option<u32>,
    /// Per-module overrides. First match wins; users can layer specific
    /// patterns above broader ones by ordering the vec.
    #[serde(default)]
    pub overrides: Vec<MoOverride>,
    /// MO002 — number of distinct architectural roles a single file is
    /// allowed to carry before being flagged as a responsibility blob.
    /// `None` means "fall back to [`DEFAULT_ENTROPY_THRESHOLD`]". Same
    /// `Option` convention as `default_max_public_types` so the section's
    /// `Default` is empty.
    #[serde(default)]
    pub entropy_threshold: Option<u32>,
    /// MO002/MO004 — function-name glob patterns that mark a function as
    /// a "handler" role (one of the entropy contributors). Empty means
    /// "fall back to the built-in default list" (`*_handler`, `handle_*`).
    /// Patterns are bare-string globs with optional leading and/or trailing
    /// `*` (no `::` segmentation) — these match function `name`, not symbol.
    #[serde(default)]
    pub handler_name_patterns: Vec<String>,
    /// MO002 — import-path patterns that mark a file as touching a
    /// persistence layer (one of the entropy contributors). Empty means
    /// "fall back to the built-in default list" (`*::sqlx::*`,
    /// `*::diesel::*`, `*::sea_orm::*`). Pattern syntax is segment-aligned
    /// — same as `MoOverride::module`.
    #[serde(default)]
    pub persistence_import_patterns: Vec<String>,
}

/// Built-in default handler-name patterns when
/// `MoSection::handler_name_patterns` is empty.
pub const DEFAULT_HANDLER_NAME_PATTERNS: &[&str] = &["*_handler", "handle_*"];

/// Built-in default persistence import patterns when
/// `MoSection::persistence_import_patterns` is empty.
pub const DEFAULT_PERSISTENCE_IMPORT_PATTERNS: &[&str] =
    &["*::sqlx::*", "*::diesel::*", "*::sea_orm::*"];

impl MoSection {
    /// Effective default budget — returns the configured value or the
    /// constant fallback when the section is empty/default.
    pub fn effective_default(&self) -> u32 {
        self.default_max_public_types
            .unwrap_or(DEFAULT_MAX_PUBLIC_TYPES)
    }

    /// Find the first override whose `module` pattern matches `module_path`,
    /// if any.
    pub fn matching_override(&self, module_path: &str) -> Option<&MoOverride> {
        self.overrides
            .iter()
            .find(|o| matches_pattern(&o.module, module_path))
    }

    /// Effective MO002 entropy threshold — configured value or fallback.
    pub fn effective_entropy_threshold(&self) -> u32 {
        self.entropy_threshold.unwrap_or(DEFAULT_ENTROPY_THRESHOLD)
    }

    /// Iterator over the effective handler-name patterns: configured patterns
    /// when present, else the built-in defaults. Used by MO002 (entropy
    /// detection) and MO004 (handler-with-canonical co-location).
    pub fn effective_handler_name_patterns(&self) -> Vec<&str> {
        if self.handler_name_patterns.is_empty() {
            DEFAULT_HANDLER_NAME_PATTERNS.to_vec()
        } else {
            self.handler_name_patterns
                .iter()
                .map(String::as_str)
                .collect()
        }
    }

    /// Iterator over the effective persistence-import patterns: configured
    /// patterns when present, else the built-in defaults.
    pub fn effective_persistence_import_patterns(&self) -> Vec<&str> {
        if self.persistence_import_patterns.is_empty() {
            DEFAULT_PERSISTENCE_IMPORT_PATTERNS.to_vec()
        } else {
            self.persistence_import_patterns
                .iter()
                .map(String::as_str)
                .collect()
        }
    }
}

/// Glob matcher for bare names (function names, not module paths).
///
/// Supports:
/// - exact match (`foo`)
/// - leading wildcard (`*foo` — name ends with `foo`)
/// - trailing wildcard (`foo*` — name starts with `foo`)
/// - both (`*foo*` — name contains `foo`)
/// - lone `*` (matches anything)
///
/// This is deliberately weaker than [`matches_pattern`] (no `::` segmentation)
/// because function names are flat strings, not paths.
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
        // pattern was just `*` — handled above; can't reach here with len < 2.
        _ => return false,
    };
    if body.is_empty() {
        // pattern was `**` or similar malformed shape; refuse silent matches.
        return false;
    }
    match (leading, trailing) {
        (true, true) => name.contains(body),
        (true, false) => name.ends_with(body),
        (false, true) => name.starts_with(body),
        (false, false) => name == body,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MoOverride {
    /// Module pattern. Same suffix-wildcard syntax as DG (`foo::*`, exact, or
    /// `*`). The helper [`matches_pattern`] is duplicated locally rather than
    /// reused from `dependency_graph::lockfile_schema` so paradigms stay
    /// decoupled.
    pub module: String,
    /// Replacement budget for any file whose `module_path` matches `module`.
    pub max_public_types: u32,
}

/// Pattern syntax: segment-aligned wildcards (mirrors UT/TA semantics).
/// - `foo::bar` — exact match
/// - `foo::*` — `foo` itself or any descendant (`foo::bar`, `foo::bar::baz`)
/// - `*::foo` — `foo` itself or anywhere ending with `::foo`
/// - `*::foo::*` — `foo` appearing as any whole segment in the path
/// - `*` — anything
///
/// MO002 needs the segment-anywhere form (`*::sqlx::*`, `*::fs::*`) for its
/// import / call-site contributors. Kept as a local copy so MO doesn't
/// depend on a sibling paradigm; if a third paradigm needs the same shape,
/// promote it to a shared module then.
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
        // Pattern was just `*::` or `::*` — treat as malformed rather than
        // matching anything; users meaning "match anything" should write `*`.
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
    fn default_section_uses_fallback_budget() {
        let s = MoSection::default();
        assert_eq!(s.effective_default(), DEFAULT_MAX_PUBLIC_TYPES);
        assert!(s.overrides.is_empty());
    }

    #[test]
    fn configured_default_overrides_fallback() {
        let s = MoSection {
            default_max_public_types: Some(3),
            overrides: Vec::new(),
            ..Default::default()
        };
        assert_eq!(s.effective_default(), 3);
    }

    #[test]
    fn matching_override_returns_first_hit() {
        let s = MoSection {
            default_max_public_types: None,
            overrides: vec![
                MoOverride {
                    module: "lore::api::*".into(),
                    max_public_types: 20,
                },
                MoOverride {
                    module: "lore::*".into(),
                    max_public_types: 10,
                },
            ],
            ..Default::default()
        };
        let hit = s
            .matching_override("lore::api::v1")
            .expect("expected match");
        assert_eq!(hit.module, "lore::api::*");
        assert_eq!(hit.max_public_types, 20);
        // Falls through to the broader pattern when the specific one misses.
        let hit2 = s
            .matching_override("lore::domain::user")
            .expect("expected fallback match");
        assert_eq!(hit2.module, "lore::*");
        assert_eq!(hit2.max_public_types, 10);
    }

    #[test]
    fn matching_override_returns_none_when_nothing_matches() {
        let s = MoSection {
            default_max_public_types: None,
            overrides: vec![MoOverride {
                module: "lore::api::*".into(),
                max_public_types: 20,
            }],
            ..Default::default()
        };
        assert!(s.matching_override("other::thing").is_none());
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
        let s = MoSection {
            default_max_public_types: Some(7),
            overrides: vec![MoOverride {
                module: "lore::api::*".into(),
                max_public_types: 20,
            }],
            entropy_threshold: Some(4),
            handler_name_patterns: vec!["on_*".into()],
            persistence_import_patterns: vec!["*::redis::*".into()],
        };
        let j = serde_json::to_value(&s).unwrap();
        let back: MoSection = serde_json::from_value(j).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn entropy_threshold_falls_back_to_constant() {
        let s = MoSection::default();
        assert_eq!(s.effective_entropy_threshold(), DEFAULT_ENTROPY_THRESHOLD);
        let s2 = MoSection {
            entropy_threshold: Some(7),
            ..Default::default()
        };
        assert_eq!(s2.effective_entropy_threshold(), 7);
    }

    #[test]
    fn handler_and_persistence_patterns_default_when_empty() {
        let s = MoSection::default();
        assert_eq!(
            s.effective_handler_name_patterns(),
            DEFAULT_HANDLER_NAME_PATTERNS.to_vec()
        );
        assert_eq!(
            s.effective_persistence_import_patterns(),
            DEFAULT_PERSISTENCE_IMPORT_PATTERNS.to_vec()
        );
    }

    #[test]
    fn handler_and_persistence_patterns_use_user_list_when_set() {
        let s = MoSection {
            handler_name_patterns: vec!["on_*".into(), "*_callback".into()],
            persistence_import_patterns: vec!["*::redis::*".into()],
            ..Default::default()
        };
        assert_eq!(
            s.effective_handler_name_patterns(),
            vec!["on_*", "*_callback"]
        );
        assert_eq!(
            s.effective_persistence_import_patterns(),
            vec!["*::redis::*"]
        );
    }

    #[test]
    fn matches_name_glob_handles_prefix_suffix_and_contains() {
        assert!(matches_name_glob("handle_*", "handle_request"));
        assert!(matches_name_glob("handle_*", "handle_"));
        assert!(!matches_name_glob("handle_*", "doesnt_handle"));
        assert!(matches_name_glob("*_handler", "request_handler"));
        assert!(!matches_name_glob("*_handler", "handler_x"));
        assert!(matches_name_glob("*foo*", "barfoobaz"));
        assert!(matches_name_glob("*foo*", "foo"));
        assert!(!matches_name_glob("*foo*", "barbaz"));
        assert!(matches_name_glob("exact", "exact"));
        assert!(!matches_name_glob("exact", "exactly"));
        assert!(matches_name_glob("*", "anything"));
    }

    #[test]
    fn matches_name_glob_refuses_malformed_double_star() {
        assert!(!matches_name_glob("**", "anything"));
    }
}
