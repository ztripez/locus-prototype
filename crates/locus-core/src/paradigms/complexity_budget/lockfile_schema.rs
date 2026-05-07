//! Lockfile section shape for CX (Complexity Budget Ownership).
//!
//! CX records per-module budgets for "how many lines a single function is
//! allowed to span." Functions whose `line_count` exceeds the configured
//! budget are flagged by CX001. The default budget is workspace-wide;
//! specific module patterns can raise or lower it (parsers/solvers may
//! legitimately be long; converters/handlers usually should not).

// ot: canonical

use serde::{Deserialize, Serialize};

/// Default budget for `default_max_function_lines` when none is set in the
/// lockfile. Fifty is a deliberate "comfortably long but not novelistic"
/// threshold — most well-factored functions sit well below this; once a
/// function passes 50 lines it usually owes the reader either a comment
/// trail or a refactor into smaller pieces.
pub const DEFAULT_MAX_FUNCTION_LINES: u32 = 50;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct CxSection {
    /// Workspace-wide budget for the line count of any single function.
    /// `None` means "fall back to [`DEFAULT_MAX_FUNCTION_LINES`]" — we keep
    /// this `Option` so `CxSection::default()` is *empty* (no fields
    /// configured) rather than carrying a magic number into round-trips.
    /// When the section is fully default (no default, no overrides) CX001
    /// stays silent; this matches the DG/MO un-onboarded UX.
    #[serde(default)]
    pub default_max_function_lines: Option<u32>,
    /// Per-module overrides. First match wins; users can layer specific
    /// patterns above broader ones by ordering the vec.
    #[serde(default)]
    pub overrides: Vec<CxOverride>,
}

impl CxSection {
    /// Effective default budget — returns the configured value or the
    /// constant fallback when the section is empty/default.
    pub fn effective_default(&self) -> u32 {
        self.default_max_function_lines
            .unwrap_or(DEFAULT_MAX_FUNCTION_LINES)
    }

    /// Find the first override whose `module` pattern matches `module_path`,
    /// if any.
    pub fn matching_override(&self, module_path: &str) -> Option<&CxOverride> {
        self.overrides
            .iter()
            .find(|o| matches_pattern(&o.module, module_path))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CxOverride {
    /// Module pattern. Same suffix-wildcard syntax as DG/MO (`foo::*`,
    /// exact, or `*`). The helper [`matches_pattern`] is duplicated locally
    /// rather than reused from another paradigm so paradigms stay
    /// decoupled.
    pub module: String,
    /// Replacement budget for any function in a file whose `module_path`
    /// matches `module`.
    pub max_function_lines: u32,
}

/// Pattern syntax: simple suffix wildcard.
/// - `foo::bar` — exact match
/// - `foo::*` — `foo` itself or any descendant (`foo::bar`, `foo::bar::baz`)
/// - `*` — anything
///
/// Mirrors `module_ownership::lockfile_schema::matches_pattern`. Kept as a
/// local copy so CX doesn't depend on MO; if a third paradigm needs the
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
        let s = CxSection::default();
        assert_eq!(s.effective_default(), DEFAULT_MAX_FUNCTION_LINES);
        assert!(s.overrides.is_empty());
    }

    #[test]
    fn configured_default_overrides_fallback() {
        let s = CxSection {
            default_max_function_lines: Some(30),
            overrides: Vec::new(),
        };
        assert_eq!(s.effective_default(), 30);
    }

    #[test]
    fn matching_override_returns_first_hit() {
        let s = CxSection {
            default_max_function_lines: None,
            overrides: vec![
                CxOverride {
                    module: "lore::parser::*".into(),
                    max_function_lines: 200,
                },
                CxOverride {
                    module: "lore::*".into(),
                    max_function_lines: 80,
                },
            ],
        };
        let hit = s
            .matching_override("lore::parser::expr")
            .expect("expected match");
        assert_eq!(hit.module, "lore::parser::*");
        assert_eq!(hit.max_function_lines, 200);
        // Falls through to the broader pattern when the specific one misses.
        let hit2 = s
            .matching_override("lore::domain::user")
            .expect("expected fallback match");
        assert_eq!(hit2.module, "lore::*");
        assert_eq!(hit2.max_function_lines, 80);
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
}
