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
#[path = "rules_tests.rs"]
mod tests;
