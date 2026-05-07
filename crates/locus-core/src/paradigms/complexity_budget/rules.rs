//! CX rule implementations.
//!
//! Implemented:
//! - [`cx001`]: function exceeds its line budget.
//!
//! Future CX rules will cover the spec's broader complexity story
//! (responsibility entropy, fan-in/out caps for utility modules, branchy
//! converters, …). CX001 is the first slice — the simplest, most useful
//! complexity check: line count per function vs a configurable budget.

use locus_air::{AirItem, AirWorkspace};

use super::lockfile_schema::CxSection;
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

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
/// Lockfile-driven silence: when the section is fully default (no
/// `default_max_function_lines` set AND no overrides), CX001 emits
/// nothing. Same convention as the other lockfile-driven rules — pre-
/// onboarding, we don't have the user's intent and shouldn't fire on
/// un-configured projects.
pub fn cx001(air: &AirWorkspace, section: &CxSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.default_max_function_lines.is_none() && section.overrides.is_empty() {
        return Vec::new();
    }
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
                    severity: mode.elevate(Severity::Warning),
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

#[cfg(test)]
mod tests {
    use super::super::lockfile_schema::{CxOverride, CxSection};
    use super::*;
    use locus_air::{AIR_SCHEMA_VERSION, AirFile, AirFunction, AirPackage, AirSpan, Visibility};

    fn func(name: &str, line_count: u32) -> AirItem {
        AirItem::Function(AirFunction {
            name: name.into(),
            symbol: format!("x::{name}"),
            visibility: Visibility::Public,
            params: Vec::new(),
            return_type: None,
            span: AirSpan::new("t.rs", 1, line_count.max(1)),
            line_count,
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
        }
    }

    #[test]
    fn cx001_silent_on_default_section() {
        // No fields configured — must stay silent regardless of function shape.
        // Mirrors the DG/MO lockfile-driven convention.
        let air = air_with(Some("foo::bar"), vec![func("big", 500)]);
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

    #[test]
    fn cx001_skips_files_without_module_path() {
        // No module_path → can't apply overrides → skip entirely.
        let air = air_with(None, vec![func("big", 500)]);
        let section = configured(50);
        assert!(cx001(&air, &section, CheckMode::Human).is_empty());
    }
}
