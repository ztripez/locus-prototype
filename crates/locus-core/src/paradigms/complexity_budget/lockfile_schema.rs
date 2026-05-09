//! Lockfile section shape for CX (Complexity Budget Ownership).
//!
//! CX records per-module budgets for "how many lines a single function is
//! allowed to span." Functions whose `line_count` exceeds the configured
//! budget are flagged by CX001. The default budget is workspace-wide;
//! specific module patterns can raise or lower it (parsers/solvers may
//! legitimately be long; converters/handlers usually should not).
//!
//! Beyond CX001's per-function line budget, CX records two additional
//! workspace-wide thresholds: `max_public_items` (caps the per-file public
//! API surface area, used by CX007) and `max_fan_out` (caps the number of
//! call sites issued by a single function, used by CX008). Both have
//! built-in defaults but ship with non-empty fallbacks so the section is
//! still useful for un-onboarded projects; CX008 stays silent until the
//! user populates `orchestration_paths`.

// locus: ot canonical

use serde::{Deserialize, Serialize};

/// Default budget for `default_max_function_lines` when none is set in the
/// lockfile. Fifty is a deliberate "comfortably long but not novelistic"
/// threshold — most well-factored functions sit well below this; once a
/// function passes 50 lines it usually owes the reader either a comment
/// trail or a refactor into smaller pieces.
pub const DEFAULT_MAX_FUNCTION_LINES: u32 = 50;

/// Default budget for `default_max_module_lines` (CX002). Four hundred
/// lines is "this file is starting to take on a lot." Rule-table files,
/// lockfile schemas, and similar pattern-dense modules legitimately
/// approach or exceed this — those want a per-pattern override raising
/// the budget. CLI dispatcher modules, kitchen-sink utility files, and
/// drift-grown handler files almost always trip the default.
pub const DEFAULT_MAX_MODULE_LINES: u32 = 400;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    /// CX002 — workspace-wide budget for the line count of any single
    /// file/module. `None` means "fall back to [`DEFAULT_MAX_MODULE_LINES`]".
    /// Independent of `default_max_function_lines`: a file might hold many
    /// short functions (CX002 fires) or one giant function (CX001 fires)
    /// or both.
    #[serde(default)]
    pub default_max_module_lines: Option<u32>,
    /// CX002 — per-module overrides for `default_max_module_lines`. Same
    /// pattern syntax as `overrides`. Kept as a separate list because the
    /// per-function and per-module budgets answer different questions and
    /// are usually adjusted independently.
    #[serde(default)]
    pub module_overrides: Vec<CxModuleOverride>,
    /// CX007 — cap on public `AirItem` count per file (anything exposing
    /// API: `Type` or `Function` with `Visibility::Public`). Defaults to
    /// [`default_max_public_items`].
    #[serde(default = "default_max_public_items")]
    pub max_public_items: u32,
    /// CX007 — module-path patterns whose files are exempt from
    /// `max_public_items`. Defaults to [`default_exempt_paths`] (test
    /// modules) so re-exporting modules and prelude files don't trip the
    /// rule out of the gate.
    #[serde(default = "default_exempt_paths")]
    pub exempt_paths: Vec<String>,
    /// CX008 — cap on the number of call sites a single function may
    /// issue. Defaults to [`default_max_fan_out`].
    #[serde(default = "default_max_fan_out")]
    pub max_fan_out: u32,
    /// CX008 — module-path patterns marking "orchestration" modules where
    /// high fan-out is expected (composition roots, CLI dispatch, runtime
    /// orchestrators). Default empty; CX008 stays silent until the user
    /// populates this list, mirroring the DG/MO un-onboarded UX.
    #[serde(default)]
    pub orchestration_paths: Vec<String>,
}

impl Default for CxSection {
    fn default() -> Self {
        Self {
            default_max_function_lines: None,
            overrides: Vec::new(),
            default_max_module_lines: None,
            module_overrides: Vec::new(),
            max_public_items: default_max_public_items(),
            exempt_paths: default_exempt_paths(),
            max_fan_out: default_max_fan_out(),
            orchestration_paths: Vec::new(),
        }
    }
}

/// Default cap for CX007. Thirty public items is "a generous facade" — most
/// well-factored modules expose far fewer; past this point a file is
/// usually a kitchen-sink prelude that should be split.
pub fn default_max_public_items() -> u32 {
    30
}

/// Default exempt paths for CX007. Test modules legitimately surface a lot
/// of `pub` helpers (test fixtures, mock builders) and the CX surface-area
/// signal is meaningless there.
pub fn default_exempt_paths() -> Vec<String> {
    vec!["*::tests::*".to_string(), "*::test::*".to_string()]
}

/// Default cap for CX008. Twenty-five call sites is "this function has
/// real orchestration shape" — past it, a function is usually a god method
/// reaching into too many places.
pub fn default_max_fan_out() -> u32 {
    25
}

impl CxSection {
    /// Effective default function-line budget — returns the configured
    /// value or the constant fallback when the section is empty/default.
    pub fn effective_default(&self) -> u32 {
        self.default_max_function_lines
            .unwrap_or(DEFAULT_MAX_FUNCTION_LINES)
    }

    /// Effective default module-line budget (CX002).
    pub fn effective_default_module(&self) -> u32 {
        self.default_max_module_lines
            .unwrap_or(DEFAULT_MAX_MODULE_LINES)
    }

    /// Find the first override whose `module` pattern matches `module_path`,
    /// if any.
    pub fn matching_override(&self, module_path: &str) -> Option<&CxOverride> {
        self.overrides
            .iter()
            .find(|o| matches_pattern(&o.module, module_path))
    }

    /// Find the first module-line override whose `module` pattern matches
    /// `module_path`, if any.
    pub fn matching_module_override(&self, module_path: &str) -> Option<&CxModuleOverride> {
        self.module_overrides
            .iter()
            .find(|o| matches_pattern(&o.module, module_path))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CxOverride {
    /// Module pattern. Same suffix-wildcard syntax as DG/MO (`foo::*`,
    /// exact, or `*`). The helper [`matches_pattern`] is duplicated locally
    /// rather than reused from another paradigm so paradigms stay
    /// decoupled.
    pub module: String,
    /// Replacement budget for any function in a file whose `module_path`
    /// matches `module`.
    pub max_function_lines: u32,
    /// Debt metadata — why this override exists. Adding a new override
    /// always fires `PG002` (visibility); absence of `reason` /
    /// `expires` / `owner` additionally triggers `PG006`. PG002 can be
    /// downgraded via `--allow-policy-calibration`; PG006 stays Fatal
    /// under `--agent-strict` because metadata is non-negotiable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Debt metadata — `YYYY-MM-DD` expiry. Past dates will surface as
    /// expired-debt diagnostics in a follow-up rule. Required by
    /// `PG006` on new overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
    /// Debt metadata — owner team / individual / role. Required by
    /// `PG006` on new overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// Optional stable identifier for cross-referencing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debt_id: Option<String>,
    /// Optional PR / issue reference describing the debt's origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced_by: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CxModuleOverride {
    /// Module pattern. Same syntax as `CxOverride.module`.
    pub module: String,
    /// Replacement module-line budget for any file whose `module_path`
    /// matches `module`.
    pub max_module_lines: u32,
    /// Debt metadata — see [`CxOverride::reason`]. Same PG002/PG006
    /// semantics apply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced_by: Option<String>,
}

/// Pattern syntax: segment-aligned wildcards.
/// - `foo::bar` — exact match
/// - `foo::*` — `foo` itself or any descendant (`foo::bar`, `foo::bar::baz`)
/// - `*::foo` — `foo` itself or anywhere ending with `::foo`
/// - `*::foo::*` — `foo` appearing as any whole segment
/// - `*` — anything
///
/// Slightly richer than MO's matcher because CX007's `exempt_paths` ships
/// with `*::tests::*` defaults, and CX008's `orchestration_paths` users
/// commonly want to match `*::cli::*`-style middle segments. Kept as a
/// local copy so CX doesn't depend on a peer paradigm.
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
        // `*::` / `::*` alone — malformed; treat as no-match. Users meant `*`.
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
        let s = CxSection::default();
        assert_eq!(s.effective_default(), DEFAULT_MAX_FUNCTION_LINES);
        assert!(s.overrides.is_empty());
        // CX007/CX008 fields populated by their helper defaults.
        assert_eq!(s.max_public_items, default_max_public_items());
        assert_eq!(s.exempt_paths, default_exempt_paths());
        assert_eq!(s.max_fan_out, default_max_fan_out());
        assert!(s.orchestration_paths.is_empty());
    }

    #[test]
    fn configured_default_overrides_fallback() {
        let s = CxSection {
            default_max_function_lines: Some(30),
            ..CxSection::default()
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
                    ..Default::default()
                },
                CxOverride {
                    module: "lore::*".into(),
                    max_function_lines: 80,
                    ..Default::default()
                },
            ],
            ..CxSection::default()
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
