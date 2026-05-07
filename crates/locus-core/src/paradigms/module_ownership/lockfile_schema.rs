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
}

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

/// Pattern syntax: simple suffix wildcard.
/// - `foo::bar` — exact match
/// - `foo::*` — `foo` itself or any descendant (`foo::bar`, `foo::bar::baz`)
/// - `*` — anything
///
/// Mirrors `dependency_graph::lockfile_schema::matches_pattern`. Kept as a
/// local copy so MO doesn't depend on DG; if a third paradigm needs the
/// same helper, promote it to a shared module then.
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
        };
        let j = serde_json::to_value(&s).unwrap();
        let back: MoSection = serde_json::from_value(j).unwrap();
        assert_eq!(s, back);
    }
}
