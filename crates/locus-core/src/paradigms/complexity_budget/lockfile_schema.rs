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
    ///
    /// Entries may be plain strings (legacy form) or `CxExemptPathEntry`
    /// structs carrying debt metadata (`expires`, `reason`, `owner`,
    /// `debt_id`, `introduced_by`). Legacy strings are accepted via the
    /// `#[serde(untagged)]` enum; after deserialization all entries are
    /// resolved to [`CxExemptPath`] via [`CxSection::resolved_exempt_paths`].
    #[serde(default = "default_exempt_path_entries")]
    pub exempt_paths: Vec<CxExemptPathEntry>,
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
            exempt_paths: default_exempt_path_entries(),
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
///
/// Kept for use in tests and doc-level explanations. The lockfile's default
/// serializer function is [`default_exempt_path_entries`].
pub fn default_exempt_paths() -> Vec<String> {
    vec!["*::tests::*".to_string(), "*::test::*".to_string()]
}

/// Default serde constructor for `CxSection::exempt_paths` (Vec of
/// `CxExemptPathEntry`). Wraps each default string pattern as a
/// `CxExemptPathEntry::Legacy` so the default section round-trips cleanly.
pub fn default_exempt_path_entries() -> Vec<CxExemptPathEntry> {
    default_exempt_paths()
        .into_iter()
        .map(CxExemptPathEntry::Legacy)
        .collect()
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

    /// Resolve all `exempt_paths` entries to [`CxExemptPath`] structs.
    /// Legacy `String` entries are promoted to pattern-only structs with
    /// all metadata fields set to `None`. This is the canonical accessor
    /// for rules and policy-guard code — they should never read
    /// `self.exempt_paths` directly.
    pub fn resolved_exempt_paths(&self) -> Vec<CxExemptPath> {
        self.exempt_paths
            .iter()
            .map(|entry| entry.clone().into())
            .collect()
    }
}

/// A single entry in `CxSection::exempt_paths` that carries optional debt
/// metadata mirroring the shape of [`CxOverride`] and [`MoOverride`].
///
/// Adding a new override always fires `PG003` (exempt-path addition). Absence
/// of `reason` / `expires` / `owner` additionally triggers `PG007`. PG003 can
/// be downgraded via `--allow-policy-calibration`; PG007 stays Fatal under
/// `--agent-strict` because metadata is non-negotiable.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CxExemptPath {
    /// The glob pattern to match against module paths. Same segment-aligned
    /// wildcard syntax as `CxOverride::module` (`foo::*`, `*::tests::*`, `*`).
    pub pattern: String,
    /// Debt metadata — `YYYY-MM-DD` expiry date. Required by `PG007` on
    /// new exempt-path entries added after the schema upgrade.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
    /// Debt metadata — human-readable explanation of why this exemption
    /// exists. Required by `PG007` on new entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Debt metadata — owner team / individual / role. Required by
    /// `PG007` on new entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// Optional stable cross-reference identifier for the debt record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debt_id: Option<String>,
    /// Optional PR / issue reference describing the exemption's origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced_by: Option<String>,
}

/// Serde-transparent enum that accepts both the legacy plain-string form
/// (`"*::tests::*"`) and the new struct form (`{"pattern": …, "reason": …}`)
/// for entries in `CxSection::exempt_paths`.
///
/// Use `CxSection::resolved_exempt_paths` to get a `Vec<CxExemptPath>` where
/// every `Legacy` entry has been promoted to a pattern-only struct.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum CxExemptPathEntry {
    /// Legacy form: a bare glob string. Deserializes from `"*::tests::*"`.
    Legacy(String),
    /// New form: a struct with `pattern` plus optional debt metadata.
    Full(CxExemptPath),
}

impl From<CxExemptPathEntry> for CxExemptPath {
    fn from(entry: CxExemptPathEntry) -> Self {
        match entry {
            CxExemptPathEntry::Legacy(pattern) => CxExemptPath {
                pattern,
                ..Default::default()
            },
            CxExemptPathEntry::Full(ep) => ep,
        }
    }
}

impl CxExemptPathEntry {
    /// Borrow the glob pattern regardless of which variant this entry is.
    pub fn pattern(&self) -> &str {
        match self {
            CxExemptPathEntry::Legacy(s) => s.as_str(),
            CxExemptPathEntry::Full(ep) => ep.pattern.as_str(),
        }
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
        assert_eq!(s.exempt_paths, default_exempt_path_entries());
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

    // ---- CxExemptPath / CxExemptPathEntry tests ----------------------

    #[test]
    fn legacy_string_parses_as_cx_exempt_path_entry() {
        let json = r#""*::tests::*""#;
        let entry: CxExemptPathEntry = serde_json::from_str(json).unwrap();
        match &entry {
            CxExemptPathEntry::Legacy(s) => assert_eq!(s, "*::tests::*"),
            CxExemptPathEntry::Full(_) => panic!("expected Legacy variant"),
        }
        let resolved: CxExemptPath = entry.into();
        assert_eq!(resolved.pattern, "*::tests::*");
        assert!(resolved.reason.is_none());
        assert!(resolved.expires.is_none());
        assert!(resolved.owner.is_none());
        assert!(resolved.debt_id.is_none());
        assert!(resolved.introduced_by.is_none());
    }

    #[test]
    fn struct_form_parses_as_cx_exempt_path_entry() {
        let json = r#"{"pattern": "locus_air::*", "reason": "canonical data crate", "expires": "2027-05-09", "owner": "@core"}"#;
        let entry: CxExemptPathEntry = serde_json::from_str(json).unwrap();
        match &entry {
            CxExemptPathEntry::Full(ep) => {
                assert_eq!(ep.pattern, "locus_air::*");
                assert_eq!(ep.reason.as_deref(), Some("canonical data crate"));
                assert_eq!(ep.expires.as_deref(), Some("2027-05-09"));
                assert_eq!(ep.owner.as_deref(), Some("@core"));
            }
            CxExemptPathEntry::Legacy(_) => panic!("expected Full variant"),
        }
    }

    #[test]
    fn mixed_legacy_and_struct_forms_parse_in_cx_section() {
        let json = r#"{
            "exempt_paths": [
                "*::tests::*",
                {"pattern": "locus_air::*", "reason": "ok", "expires": "2027-01-01", "owner": "@core"}
            ]
        }"#;
        let section: CxSection = serde_json::from_str(json).unwrap();
        assert_eq!(section.exempt_paths.len(), 2);
        assert_eq!(section.exempt_paths[0].pattern(), "*::tests::*");
        assert_eq!(section.exempt_paths[1].pattern(), "locus_air::*");

        let resolved = section.resolved_exempt_paths();
        assert_eq!(resolved[0].pattern, "*::tests::*");
        assert!(resolved[0].reason.is_none(), "legacy entry has no reason");
        assert_eq!(resolved[1].pattern, "locus_air::*");
        assert_eq!(resolved[1].reason.as_deref(), Some("ok"));
    }

    #[test]
    fn cx_section_with_legacy_strings_round_trips_as_structs() {
        // The current .locus/lock.json format: Vec<String> entries.
        let json = r#"{"exempt_paths": ["*::tests::*", "locus_air::*"]}"#;
        let section: CxSection = serde_json::from_str(json).unwrap();
        assert_eq!(section.exempt_paths.len(), 2);
        // Both are Legacy variants.
        assert!(matches!(
            &section.exempt_paths[0],
            CxExemptPathEntry::Legacy(s) if s == "*::tests::*"
        ));
        assert!(matches!(
            &section.exempt_paths[1],
            CxExemptPathEntry::Legacy(s) if s == "locus_air::*"
        ));
    }

    #[test]
    fn cx_section_full_struct_round_trips() {
        let original = CxSection {
            exempt_paths: vec![
                CxExemptPathEntry::Legacy("*::tests::*".to_string()),
                CxExemptPathEntry::Full(CxExemptPath {
                    pattern: "locus_air::*".to_string(),
                    reason: Some("canonical data crate".to_string()),
                    expires: Some("2027-05-09".to_string()),
                    owner: Some("@core".to_string()),
                    debt_id: Some("CX-locus-air-exempt".to_string()),
                    introduced_by: Some("PR #48".to_string()),
                }),
            ],
            ..CxSection::default()
        };
        let json = serde_json::to_value(&original).unwrap();
        let back: CxSection = serde_json::from_value(json).unwrap();
        assert_eq!(original, back);
    }

    #[test]
    fn legacy_entry_pattern_accessor() {
        let entry = CxExemptPathEntry::Legacy("foo::*".to_string());
        assert_eq!(entry.pattern(), "foo::*");
    }

    #[test]
    fn full_entry_pattern_accessor() {
        let entry = CxExemptPathEntry::Full(CxExemptPath {
            pattern: "bar::*".to_string(),
            ..Default::default()
        });
        assert_eq!(entry.pattern(), "bar::*");
    }

    #[test]
    fn resolved_exempt_paths_returns_all_entries_as_cx_exempt_path() {
        let section = CxSection {
            exempt_paths: vec![
                CxExemptPathEntry::Legacy("*::tests::*".to_string()),
                CxExemptPathEntry::Full(CxExemptPath {
                    pattern: "locus_air::*".to_string(),
                    reason: Some("data crate".to_string()),
                    ..Default::default()
                }),
            ],
            ..CxSection::default()
        };
        let resolved = section.resolved_exempt_paths();
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].pattern, "*::tests::*");
        assert!(resolved[0].reason.is_none());
        assert_eq!(resolved[1].pattern, "locus_air::*");
        assert_eq!(resolved[1].reason.as_deref(), Some("data crate"));
    }
}
