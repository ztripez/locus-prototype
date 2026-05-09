//! CX rule implementations.
//!
//! Implemented:
//! - [`cx001`]: function exceeds its line budget.
//! - [`cx002`]: file/module exceeds its line budget.
//! - [`cx007`]: file exposes more public API items than `max_public_items`.
//! - [`cx008`]: function issues more call sites than `max_fan_out` and
//!   doesn't live under an accepted `orchestration_paths` module.
//!
//! Future CX rules will cover the spec's broader complexity story
//! (responsibility entropy, branchy converters, …). CX001 caps function
//! length, CX002 caps module length, CX007 caps a file's public API
//! surface, CX008 caps a function's outbound fan-out — together they
//! cover the major shape-overrun cases without a deep AST audit.

use std::collections::HashMap;

use locus_air::{AirItem, AirWorkspace, Visibility};

use super::lockfile_schema::{CxSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};
use locus_air::AirSpan;

/// CX001 — function exceeds its line budget.
///
/// For each `AirFile` with a `module_path`, walk every `AirItem::Function`
/// and compare its `line_count` against the file's effective budget:
/// - if the file's `module_path` matches an override's `module` pattern,
///   the override's `max_function_lines` wins (first match);
/// - otherwise the section's `default_max_function_lines` (or the constant
///   fallback) is used.
///
/// One diagnostic per function that overshoots its budget.
///
/// Severity: Warning by default. `--agent-strict` elevates to Fatal via
/// [`CheckMode::elevate`].
///
/// Fires by default — the section's built-in fallback budget
/// ([`super::lockfile_schema::DEFAULT_MAX_FUNCTION_LINES`]) is treated as
/// real configuration. Configuration narrows: users raise the budget on
/// dense modules via `paradigms.CX.overrides`, or replace the workspace
/// default via `default_max_function_lines`. Add the prefix to
/// `acknowledged_empty` to silence the paradigm entirely.
pub fn cx001(air: &AirWorkspace, section: &CxSection, mode: CheckMode) -> Vec<Diagnostic> {
    let default_budget = section.effective_default();
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let matched_override = section.matching_override(module_path);
            let budget = matched_override
                .map(|o| o.max_function_lines)
                .unwrap_or(default_budget);

            // Advisory-tier elevation: CX001 only blocks under
            // `--agent-strict` once the user has narrowed the rule for this
            // call site (per-module override, or an explicit workspace
            // default). Built-in fallback alone keeps the rule a Warning
            // smoke alarm. See `CheckMode::elevate_when_actionable` and
            // issue #6.
            let narrowed =
                matched_override.is_some() || section.default_max_function_lines.is_some();

            for item in &file.items {
                let AirItem::Function(func) = item else {
                    continue;
                };
                if func.line_count <= budget {
                    continue;
                }

                let mut why = vec![
                    format!(
                        "function `{}` spans {} line(s)",
                        func.symbol, func.line_count
                    ),
                    if let Some(o) = matched_override {
                        format!("budget {budget} from override `module = {}`", o.module)
                    } else {
                        format!("budget {budget} (workspace default)")
                    },
                ];
                if matched_override.is_none() && section.default_max_function_lines.is_none() {
                    why.push(format!(
                        "no `default_max_function_lines` configured; using built-in fallback {}",
                        default_budget
                    ));
                }

                out.push(Diagnostic {
                    rule_id: "CX001".to_string(),
                    severity: mode.elevate_when_actionable(Severity::Warning, narrowed),
                    span: func.span.clone(),
                    concept: None,
                    message: format!(
                        "function `{}` is {} lines, budget {} ({})",
                        func.symbol,
                        func.line_count,
                        budget,
                        match matched_override {
                            Some(o) => format!("override `{}`", o.module),
                            None => "workspace default".to_string(),
                        }
                    ),
                    why,
                    suggested_fix: Some(
                        "split the function into smaller steps each owning one decision, \
                         or — if this length is intended (e.g. a parser arm or state \
                         machine) — raise the budget by adding an override to \
                         `paradigms.CX.overrides` in `locus.lock`"
                            .into(),
                    ),
                });
            }
        }
    }
    out
}

/// CX002 — module exceeds its line budget.
///
/// For each `AirFile` with a `module_path`, compare the file's
/// `line_count` against the file's effective module budget:
/// - if the file's `module_path` matches a `module_overrides` entry, the
///   override's `max_module_lines` wins (first match);
/// - otherwise the section's `default_max_module_lines` (or the constant
///   fallback [`super::lockfile_schema::DEFAULT_MAX_MODULE_LINES`]) is used.
///
/// One diagnostic per oversized file. Anchored at line 1 of the file (the
/// violation is the file's responsibility, not any specific item).
///
/// Severity: Warning by default. `--agent-strict` elevates to Fatal via
/// [`CheckMode::elevate`].
///
/// Fires by default — the section's built-in fallback is treated as
/// real configuration so un-onboarded code isn't a CX002 blind spot.
/// Once a project starts hitting CX002 noise on legitimately-dense
/// modules (rule tables, large lockfile schemas), the user raises the
/// budget via `paradigms.CX.module_overrides` or
/// `paradigms.CX.default_max_module_lines`.
pub fn cx002(air: &AirWorkspace, section: &CxSection, mode: CheckMode) -> Vec<Diagnostic> {
    let default_budget = section.effective_default_module();
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let matched_override = section.matching_module_override(module_path);
            let budget = matched_override
                .map(|o| o.max_module_lines)
                .unwrap_or(default_budget);

            if file.line_count <= budget {
                continue;
            }

            // See CX001 above for the advisory-tier elevation rationale.
            let narrowed = matched_override.is_some() || section.default_max_module_lines.is_some();

            let mut why = vec![
                format!("file `{}` spans {} line(s)", file.path, file.line_count),
                if let Some(o) = matched_override {
                    format!("budget {budget} from override `module = {}`", o.module)
                } else {
                    format!("budget {budget} (workspace default)")
                },
            ];
            if matched_override.is_none() && section.default_max_module_lines.is_none() {
                why.push(format!(
                    "no `default_max_module_lines` configured; using built-in fallback {}",
                    default_budget
                ));
            }

            out.push(Diagnostic {
                rule_id: "CX002".to_string(),
                severity: mode.elevate_when_actionable(Severity::Warning, narrowed),
                span: AirSpan::new(file.path.clone(), 1, 1),
                concept: None,
                message: format!(
                    "module `{module_path}` is {} lines, budget {} ({})",
                    file.line_count,
                    budget,
                    match matched_override {
                        Some(o) => format!("override `{}`", o.module),
                        None => "workspace default".to_string(),
                    }
                ),
                why,
                suggested_fix: Some(
                    "split the module into smaller, more focused files each owning one \
                     responsibility, or — if this density is intended (e.g. a rule table, \
                     a lockfile schema, a state machine) — raise the budget by adding an \
                     override to `paradigms.CX.module_overrides` in `locus.lock`"
                        .into(),
                ),
            });
        }
    }
    out
}

/// CX007 — excessive public surface.
///
/// For each `AirFile` with a `module_path`, count `AirItem` entries that
/// expose API: `Type` and `Function` items with `Visibility::Public`. Fire
/// one diagnostic per file whose count exceeds `section.max_public_items`
/// AND whose `module_path` doesn't match any pattern in
/// `section.exempt_paths`.
///
/// Severity: Warning by default. `--agent-strict` elevates to Fatal via
/// [`CheckMode::elevate`].
///
/// Unlike CX001 there's no "silent on default section" guard: the section
/// ships with a sensible `max_public_items` (30) plus default exempt
/// paths covering test modules, so the rule is useful out of the box.
/// Files without a `module_path` are skipped — we can't apply
/// `exempt_paths` without one.
pub fn cx007(air: &AirWorkspace, section: &CxSection, mode: CheckMode) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            if section
                .exempt_paths
                .iter()
                .any(|pat| matches_pattern(pat, module_path))
            {
                continue;
            }

            let public_count = file
                .items
                .iter()
                .filter(|it| match it {
                    AirItem::Type(t) => t.visibility == Visibility::Public,
                    AirItem::Function(f) => f.visibility == Visibility::Public,
                    _ => false,
                })
                .count() as u32;

            if public_count <= section.max_public_items {
                continue;
            }

            // Anchor the diagnostic at the file's first item span when we
            // have one; otherwise fall back to a synthetic span at line 1
            // of the file path so the diagnostic still points somewhere.
            let span = file
                .items
                .iter()
                .find_map(|it| match it {
                    AirItem::Type(t) => Some(t.span.clone()),
                    AirItem::Function(f) => Some(f.span.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| locus_air::AirSpan::new(file.path.clone(), 1, 1));

            out.push(Diagnostic {
                rule_id: "CX007".to_string(),
                severity: mode.elevate(Severity::Warning),
                span,
                concept: None,
                message: format!(
                    "module `{module_path}` exposes {public_count} public items, budget {} \
                     — likely a kitchen-sink facade",
                    section.max_public_items
                ),
                why: vec![
                    format!("file `{}`", file.path),
                    format!("module path `{module_path}`"),
                    format!(
                        "public item count {public_count} > max_public_items {}",
                        section.max_public_items
                    ),
                ],
                suggested_fix: Some(
                    "split the module into smaller, more focused units; or — if this \
                     facade is intentional (e.g. a public prelude) — exempt the \
                     module by adding its path pattern to `paradigms.CX.exempt_paths` \
                     in `locus.lock`, or raise `paradigms.CX.max_public_items`"
                        .into(),
                ),
            });
        }
    }
    out
}

/// CX008 — high fan-out outside orchestration owners.
///
/// For each `AirItem::Function`, count its enclosing `AirItem::CallSite`
/// items (where `cs.function == Some(func.symbol)`). Fire one diagnostic
/// per function whose call-site count exceeds `section.max_fan_out` AND
/// whose enclosing module doesn't match any pattern in
/// `section.orchestration_paths`.
///
/// Severity: Warning by default; Fatal under `--agent-strict`.
///
/// Lockfile-driven silence: when `orchestration_paths` is empty the rule
/// stays silent entirely. The thinking: deciding "where high fan-out is
/// legitimate" is a deliberate user act (composition roots, CLI dispatch,
/// runtime orchestrators); without that declaration, every fan-out is
/// either accepted or noise, so we don't fire pre-onboarding. Mirrors the
/// DG/MO un-onboarded UX.
pub fn cx008(air: &AirWorkspace, section: &CxSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.orchestration_paths.is_empty() {
        return Vec::new();
    }

    // Step 1: count call sites per enclosing-function symbol.
    let mut fan_out: HashMap<&str, u32> = HashMap::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                if let AirItem::CallSite(cs) = item
                    && let Some(sym) = cs.function.as_deref()
                {
                    *fan_out.entry(sym).or_insert(0) += 1;
                }
            }
        }
    }

    // Step 2: walk every Function, look up its count, fire if it exceeds
    // the cap AND the enclosing module isn't an orchestration path.
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let module_path = file.module_path.as_deref();
            for item in &file.items {
                let AirItem::Function(func) = item else {
                    continue;
                };
                let Some(&count) = fan_out.get(func.symbol.as_str()) else {
                    continue;
                };
                if count <= section.max_fan_out {
                    continue;
                }

                let exempt = module_path
                    .map(|mp| {
                        section
                            .orchestration_paths
                            .iter()
                            .any(|pat| matches_pattern(pat, mp))
                    })
                    .unwrap_or(false);
                if exempt {
                    continue;
                }

                out.push(Diagnostic {
                    rule_id: "CX008".to_string(),
                    severity: mode.elevate(Severity::Warning),
                    span: func.span.clone(),
                    concept: None,
                    message: format!(
                        "function `{}` issues {count} call sites, budget {} \
                         — high fan-out outside an accepted orchestration module",
                        func.symbol, section.max_fan_out
                    ),
                    why: vec![
                        format!("function symbol `{}`", func.symbol),
                        match module_path {
                            Some(mp) => format!("module path `{mp}`"),
                            None => "module path unknown".to_string(),
                        },
                        format!(
                            "call-site count {count} > max_fan_out {}",
                            section.max_fan_out
                        ),
                    ],
                    suggested_fix: Some(
                        "extract sub-steps into helper functions, or — if this \
                         function is a legitimate orchestrator — add its module \
                         path to `paradigms.CX.orchestration_paths` in \
                         `locus.lock`"
                            .into(),
                    ),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::lockfile_schema::{CxOverride, CxSection};
    use super::*;
    use locus_air::{
        AIR_SCHEMA_VERSION, AirCallSite, AirFile, AirFunction, AirPackage, AirSpan, AirType,
        CallKind, TypeKind, Visibility,
    };

    fn func(name: &str, line_count: u32) -> AirItem {
        AirItem::Function(AirFunction {
            name: name.into(),
            symbol: format!("x::{name}"),
            visibility: Visibility::Public,
            params: Vec::new(),
            return_type: None,
            span: AirSpan::new("t.rs", 1, line_count.max(1)),
            line_count,
            decorators: Vec::new(),
            symbol_segments: Vec::new(),
            doc: None,
        })
    }

    fn air_with(module: Option<&str>, items: Vec<AirItem>) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "t.rs".into(),
                    module_path: module.map(str::to_string),
                    items,
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                }],
            }],
            facts: Vec::new(),
        }
    }

    fn configured(default_budget: u32) -> CxSection {
        CxSection {
            default_max_function_lines: Some(default_budget),
            overrides: Vec::new(),
            ..CxSection::default()
        }
    }

    #[test]
    fn cx001_fires_with_built_in_fallback_on_default_section() {
        // Default section uses DEFAULT_MAX_FUNCTION_LINES (50) as the
        // budget. A 500-line function trips it without any user
        // configuration — the rule fires by default per the
        // "noisy-by-default, configuration narrows" convention.
        let air = air_with(Some("foo::bar"), vec![func("big", 500)]);
        let section = CxSection::default();
        let diags = cx001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1, "expected one diag, got {diags:?}");
        assert!(
            diags[0].why.iter().any(|w| w.contains("built-in fallback")),
            "expected built-in fallback explanation in why; got {:?}",
            diags[0].why,
        );
    }

    #[test]
    fn cx001_quiet_when_function_within_built_in_fallback() {
        // 30-line function under the 50-line built-in fallback → no diag.
        let air = air_with(Some("foo::bar"), vec![func("small", 30)]);
        let section = CxSection::default();
        assert!(cx001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cx001_fires_when_line_count_exceeds_default_budget() {
        // 60 lines under default budget of 50 → fires.
        let air = air_with(Some("foo::bar"), vec![func("big", 60)]);
        let section = configured(50);
        let diags = cx001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1, "expected one diag, got {diags:?}");
        assert_eq!(diags[0].rule_id, "CX001");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("x::big"));
        assert!(diags[0].message.contains("60"));
        assert!(diags[0].message.contains("budget 50"));
    }

    #[test]
    fn cx001_quiet_when_line_count_at_or_below_budget() {
        let section = configured(50);
        // exactly at budget
        let air = air_with(Some("foo::bar"), vec![func("ok", 50)]);
        assert!(cx001(&air, &section, CheckMode::Human).is_empty());
        // under budget
        let air = air_with(Some("foo::bar"), vec![func("tiny", 10)]);
        assert!(cx001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cx001_override_raises_budget_effectively() {
        // Default 50; parser function is 120 lines, override gives 200.
        let air = air_with(Some("lore::parser::expr"), vec![func("parse_expr", 120)]);
        let section = CxSection {
            default_max_function_lines: Some(50),
            overrides: vec![CxOverride {
                module: "lore::parser::*".into(),
                max_function_lines: 200,
            }],
            ..CxSection::default()
        };
        assert!(
            cx001(&air, &section, CheckMode::Human).is_empty(),
            "override should raise budget above the function's line count"
        );
    }

    #[test]
    fn cx001_override_lowers_budget_effectively() {
        // Default 50; converter function is 40 lines (within default). Override
        // lowers the converter budget to 20 → fires.
        let air = air_with(Some("lore::convert::user"), vec![func("to_dto", 40)]);
        let section = CxSection {
            default_max_function_lines: Some(50),
            overrides: vec![CxOverride {
                module: "lore::convert::*".into(),
                max_function_lines: 20,
            }],
            ..CxSection::default()
        };
        let diags = cx001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1, "override should lower budget below count");
        assert_eq!(diags[0].rule_id, "CX001");
        assert!(diags[0].message.contains("budget 20"));
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("override") && w.contains("lore::convert::*")),
            "expected override mention in `why`; got {:?}",
            diags[0].why
        );
    }

    #[test]
    fn cx001_agent_strict_elevates_to_fatal() {
        let air = air_with(Some("foo::bar"), vec![func("big", 60)]);
        let section = configured(50);
        let diags = cx001(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].severity,
            Severity::Fatal,
            "agent-strict should elevate Warning to Fatal"
        );
    }

    /// Advisory-tier elevation: under `--agent-strict` the rule stays
    /// Warning when the user hasn't narrowed it (default section, no
    /// workspace budget, no per-module override). Built-in fallback alone
    /// is a smoke alarm, not a CI blocker. See `CheckMode::elevate_when_actionable`
    /// and issue #6.
    #[test]
    fn cx001_agent_strict_stays_warning_when_using_built_in_fallback() {
        let air = air_with(Some("foo::bar"), vec![func("big", 500)]);
        let section = CxSection::default();
        let diags = cx001(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].severity,
            Severity::Warning,
            "un-narrowed advisory rule stays Warning under agent-strict; \
             user must declare a budget before this becomes a CI blocker",
        );
    }

    /// Once the user has set a workspace default, the rule is "narrowed" —
    /// they've explicitly opted into the budget. Agent-strict should
    /// elevate to Fatal at that point.
    #[test]
    fn cx001_agent_strict_elevates_when_workspace_default_set() {
        let air = air_with(Some("foo::bar"), vec![func("big", 60)]);
        let section = CxSection {
            default_max_function_lines: Some(50),
            ..CxSection::default()
        };
        let diags = cx001(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    /// Per-module override is also a "narrowed" signal — the user has
    /// made an explicit budget decision for this module path, so
    /// agent-strict should elevate.
    #[test]
    fn cx001_agent_strict_elevates_when_module_override_matches() {
        use super::super::lockfile_schema::CxOverride;
        let air = air_with(Some("foo::bar"), vec![func("big", 200)]);
        let section = CxSection {
            // No workspace default; only a per-module override.
            default_max_function_lines: None,
            overrides: vec![CxOverride {
                module: "foo::*".into(),
                max_function_lines: 100,
            }],
            ..CxSection::default()
        };
        let diags = cx001(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    fn air_with_lines(module: Option<&str>, line_count: u32) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "t.rs".into(),
                    module_path: module.map(str::to_string),
                    items: Vec::new(),
                    hints: Vec::new(),
                    parse_error: None,
                    line_count,
                }],
            }],
            facts: Vec::new(),
        }
    }

    #[test]
    fn cx002_fires_with_built_in_fallback_on_default_section() {
        let air = air_with_lines(Some("foo::bar"), 5_000);
        let section = CxSection::default();
        let diags = cx002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Warning);
    }

    /// Advisory-tier elevation: CX002 stays Warning under agent-strict
    /// when no workspace default and no per-module override are set.
    #[test]
    fn cx002_agent_strict_stays_warning_when_using_built_in_fallback() {
        let air = air_with_lines(Some("foo::bar"), 5_000);
        let section = CxSection::default();
        let diags = cx002(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].severity,
            Severity::Warning,
            "un-narrowed advisory rule stays Warning under agent-strict",
        );
    }

    #[test]
    fn cx002_agent_strict_elevates_when_workspace_default_set() {
        let air = air_with_lines(Some("foo::bar"), 1_000);
        let section = CxSection {
            default_max_module_lines: Some(500),
            ..CxSection::default()
        };
        let diags = cx002(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn cx002_agent_strict_elevates_when_module_override_matches() {
        use super::super::lockfile_schema::CxModuleOverride;
        let air = air_with_lines(Some("foo::bar"), 1_500);
        let section = CxSection {
            default_max_module_lines: None,
            module_overrides: vec![CxModuleOverride {
                module: "foo::*".into(),
                max_module_lines: 1_000,
            }],
            ..CxSection::default()
        };
        let diags = cx002(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn cx001_skips_files_without_module_path() {
        // No module_path → can't apply overrides → skip entirely.
        let air = air_with(None, vec![func("big", 500)]);
        let section = configured(50);
        assert!(cx001(&air, &section, CheckMode::Human).is_empty());
    }

    // --- CX007 fixtures + tests ----------------------------------------

    fn pub_type(name: &str) -> AirItem {
        AirItem::Type(AirType {
            kind: TypeKind::Struct,
            name: name.into(),
            symbol: format!("x::{name}"),
            visibility: Visibility::Public,
            fields: Vec::new(),
            variants: Vec::new(),
            decorators: Vec::new(),
            symbol_segments: Vec::new(),
            span: AirSpan::new("t.rs", 1, 1),
            doc: None,
        })
    }

    fn priv_type(name: &str) -> AirItem {
        AirItem::Type(AirType {
            kind: TypeKind::Struct,
            name: name.into(),
            symbol: format!("x::{name}"),
            visibility: Visibility::Private,
            fields: Vec::new(),
            variants: Vec::new(),
            decorators: Vec::new(),
            symbol_segments: Vec::new(),
            span: AirSpan::new("t.rs", 1, 1),
            doc: None,
        })
    }

    fn priv_fn(name: &str) -> AirItem {
        AirItem::Function(AirFunction {
            name: name.into(),
            symbol: format!("x::{name}"),
            visibility: Visibility::Private,
            params: Vec::new(),
            return_type: None,
            span: AirSpan::new("t.rs", 1, 1),
            line_count: 1,
            decorators: Vec::new(),
            symbol_segments: Vec::new(),
            doc: None,
        })
    }

    fn cx007_section(max: u32, exempt: Vec<&str>) -> CxSection {
        CxSection {
            max_public_items: max,
            exempt_paths: exempt.into_iter().map(str::to_string).collect(),
            ..CxSection::default()
        }
    }

    #[test]
    fn cx007_quiet_when_public_count_at_or_below_budget() {
        // 3 public items, budget 5 → silent. Both at-budget and under-budget.
        let air = air_with(
            Some("x::core"),
            vec![pub_type("A"), pub_type("B"), pub_type("C")],
        );
        let section = cx007_section(5, vec![]);
        assert!(cx007(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cx007_fires_when_public_count_exceeds_budget() {
        // 4 public items vs budget 3 → one diag.
        let items = vec![
            pub_type("A"),
            pub_type("B"),
            pub_type("C"),
            func("d", 5), // public by default in our `func` helper
        ];
        let air = air_with(Some("x::core"), items);
        let section = cx007_section(3, vec![]);
        let diags = cx007(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].rule_id, "CX007");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("x::core"));
        assert!(diags[0].message.contains("4"));
        assert!(diags[0].message.contains("budget 3"));
    }

    #[test]
    fn cx007_only_counts_public_items() {
        // 2 public + 5 private = total 7, but only public counts → silent at budget 3.
        let items = vec![
            pub_type("A"),
            pub_type("B"),
            priv_type("p1"),
            priv_type("p2"),
            priv_type("p3"),
            priv_fn("hidden1"),
            priv_fn("hidden2"),
        ];
        let air = air_with(Some("x::core"), items);
        let section = cx007_section(3, vec![]);
        assert!(cx007(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cx007_exempt_paths_silence_diagnostic() {
        // 5 public items, budget 3, but module matches `*::tests::*` exempt → silent.
        let items = vec![
            pub_type("A"),
            pub_type("B"),
            pub_type("C"),
            pub_type("D"),
            pub_type("E"),
        ];
        let air = air_with(Some("x::tests::helpers"), items);
        let section = cx007_section(3, vec!["*::tests::*"]);
        assert!(cx007(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cx007_default_exempt_paths_cover_test_modules() {
        // Default section ships with `*::tests::*` and `*::test::*` exempts.
        let items = (0..40)
            .map(|i| pub_type(&format!("T{i}")))
            .collect::<Vec<_>>();
        let air = air_with(Some("x::tests::big"), items);
        let section = CxSection::default();
        assert!(cx007(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cx007_agent_strict_elevates_to_fatal() {
        let items = vec![pub_type("A"), pub_type("B"), pub_type("C"), pub_type("D")];
        let air = air_with(Some("x::core"), items);
        let section = cx007_section(3, vec![]);
        let diags = cx007(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    // --- CX008 fixtures + tests ----------------------------------------

    fn callsite(callee: &str, in_function: &str) -> AirItem {
        AirItem::CallSite(AirCallSite {
            callee: callee.into(),
            kind: CallKind::Function,
            function: Some(in_function.into()),
            span: AirSpan::new("t.rs", 5, 5),
        })
    }

    fn cx008_section(max: u32, orchestration: Vec<&str>) -> CxSection {
        CxSection {
            max_fan_out: max,
            orchestration_paths: orchestration.into_iter().map(str::to_string).collect(),
            ..CxSection::default()
        }
    }

    #[test]
    fn cx008_silent_when_orchestration_paths_empty() {
        // Even with rampant fan-out, no orchestration declaration means silent.
        // Mirrors DG/MO lockfile-driven convention.
        let mut items = vec![func("dispatch", 5)];
        for i in 0..50 {
            items.push(callsite(&format!("callee{i}"), "x::dispatch"));
        }
        let air = air_with(Some("x::core"), items);
        let section = CxSection::default(); // empty orchestration_paths
        assert!(cx008(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cx008_fires_when_count_exceeds_budget_outside_orchestration() {
        // 6 call sites, budget 5, in `x::core` (not under orchestration) → fires.
        let mut items = vec![func("dispatch", 5)];
        for i in 0..6 {
            items.push(callsite(&format!("callee{i}"), "x::dispatch"));
        }
        let air = air_with(Some("x::core"), items);
        let section = cx008_section(5, vec!["x::cli::*"]);
        let diags = cx008(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].rule_id, "CX008");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("x::dispatch"));
        assert!(diags[0].message.contains("6"));
        assert!(diags[0].message.contains("budget 5"));
    }

    #[test]
    fn cx008_quiet_when_count_at_or_below_budget() {
        let mut items = vec![func("dispatch", 5)];
        for i in 0..5 {
            // exactly at budget
            items.push(callsite(&format!("c{i}"), "x::dispatch"));
        }
        let air = air_with(Some("x::core"), items);
        let section = cx008_section(5, vec!["x::cli::*"]);
        assert!(cx008(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cx008_orchestration_path_silences_diagnostic() {
        // 10 call sites, budget 3, but module matches orchestration → silent.
        let mut items = vec![func("dispatch", 5)];
        for i in 0..10 {
            items.push(callsite(&format!("c{i}"), "x::dispatch"));
        }
        let air = air_with(Some("x::cli::dispatch"), items);
        let section = cx008_section(3, vec!["x::cli::*"]);
        assert!(cx008(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cx008_agent_strict_elevates_to_fatal() {
        let mut items = vec![func("dispatch", 5)];
        for i in 0..6 {
            items.push(callsite(&format!("c{i}"), "x::dispatch"));
        }
        let air = air_with(Some("x::core"), items);
        let section = cx008_section(5, vec!["x::cli::*"]);
        let diags = cx008(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn cx008_only_counts_call_sites_in_owning_function() {
        // Two functions; only one issues lots of call sites.
        let mut items = vec![func("dispatch", 5), func("tiny", 5)];
        for i in 0..6 {
            items.push(callsite(&format!("c{i}"), "x::dispatch"));
        }
        items.push(callsite("only", "x::tiny"));
        let air = air_with(Some("x::core"), items);
        let section = cx008_section(5, vec!["x::cli::*"]);
        let diags = cx008(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert!(diags[0].message.contains("x::dispatch"));
        assert!(!diags[0].message.contains("x::tiny"));
    }
}
