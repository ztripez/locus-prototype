//! FL007 — catch-all `Err(_) =>` arm body silently swallows.
//!
//! Reads [`AirItem::MatchArm`] items (AIR v10). Fires on an arm whose
//! pattern matches an `Err` variant *and* contains a wildcard binder
//! (`Err(_) => …`) AND whose body shape is one of `Empty`, `Literal`,
//! `Call` — the silent default-producing shapes. Arms that `Return`,
//! `Propagate` (use `?`), or run a multi-statement `Block` are not
//! flagged: their author has already taken explicit action.
//!
//! Pattern detection is text-based: the visitor records the arm's
//! pattern as rendered text, so we look for the `Err` prefix /
//! `Err(` substring in combination with the boolean
//! `pattern_has_wildcard`. This intentionally accepts both the bare
//! `Err(_)` form and qualified shapes like `MyError::Err(_)` or
//! `Result::Err(_)`.
//!
//! Severity: `mode.elevate(Severity::Warning)`.
//!
//! Lockfile-driven silence: stays quiet until `invariant_owner_paths`
//! is populated.

use locus_air::{AirItem, AirMatchArm, AirWorkspace};

use super::super::lockfile_schema::FlSection;
use super::helpers::{body_shape_label, callsite_in_invariant_owner, is_silent_body_shape};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

pub fn fl007(air: &AirWorkspace, section: &FlSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.invariant_owner_paths.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            for item in &file.items {
                let AirItem::MatchArm(arm) = item else {
                    continue;
                };
                if !arm.pattern_has_wildcard {
                    continue;
                }
                if !pattern_targets_err_variant(&arm.pattern) {
                    continue;
                }
                if !is_silent_body_shape(arm.body_shape) {
                    continue;
                }
                if callsite_in_invariant_owner(
                    module_path,
                    arm.function.as_deref(),
                    &section.invariant_owner_paths,
                ) {
                    continue;
                }
                out.push(diagnostic_for_fl007(arm, module_path, mode));
            }
        }
    }
    out
}

/// Is the arm pattern an `Err` variant — bare `Err(...)` or path-qualified
/// (`Result::Err(...)`, `MyEnum::Err(...)`)? FL007 fires on these; FL011
/// is the bare-`_` complement.
fn pattern_targets_err_variant(pattern: &str) -> bool {
    let p = pattern.trim();
    p == "Err"
        || p.starts_with("Err(")
        || p.contains("::Err(")
        || p.contains("::Err ")
        || p.ends_with("::Err")
}

fn diagnostic_for_fl007(arm: &AirMatchArm, module_path: &str, mode: CheckMode) -> Diagnostic {
    let function_label = arm
        .function
        .as_deref()
        .unwrap_or("<unknown enclosing function>");
    let body_label = body_shape_label(arm.body_shape);
    Diagnostic {
        rule_id: "FL007".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: arm.span.clone(),
        concept: None,
        message: format!(
            "catch-all `Err(_) => {body_label}` arm in `{module_path}` (fn `{function_label}`) \
             silently swallows the failure"
        ),
        why: vec![
            format!("module `{module_path}`"),
            format!("enclosing function: `{function_label}`"),
            format!("arm pattern `{}` matches every `Err` variant", arm.pattern),
            format!("arm body is a `{body_label}` (silent default)"),
            "the failure has no owner — caller can't tell anything went wrong".into(),
        ],
        suggested_fix: Some(format!(
            "rewrite to bind the error and either log/wrap it or propagate via `?` — \
             e.g. `Err(e) => return Err(MyError::from(e))`. If `{module_path}` is a \
             legitimate invariant owner (supervisor, test-support module), add it \
             to `paradigms.FL.invariant_owner_paths`. For a one-off accepted \
             swallow, suppress with `// locus: allow FL007 reason=\"…\" \
             expires=\"YYYY-MM-DD\"`"
        )),
    }
}

pub struct Fl007Rule;
pub static FL007_RULE: Fl007Rule = Fl007Rule;

const FL007_ID: RuleId = RuleId::new("FL007");
const FL007_PARADIGM: ParadigmId = ParadigmId::new("FL");

impl RuleDefinition for Fl007Rule {
    fn id(&self) -> RuleId {
        FL007_ID
    }
    fn paradigm(&self) -> ParadigmId {
        FL007_PARADIGM
    }
    fn title(&self) -> &'static str {
        "catch-all `Err(_)` arm with silent body"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::super::lockfile_schema::FlSection;
        let section: FlSection = ctx.lockfile.paradigm_section("FL").unwrap_or_default();
        fl007(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(FL007_ID),
                rule_id: Some(FL007_ID),
                paradigm_id: Some(FL007_PARADIGM),
                default_severity: d.severity,
                span: Some(d.span),
                concept: d.concept,
                message: d.message,
                evidence: vec![],
                why: d.why,
                suggested_fix: d.suggested_fix,
                diagnostic_code: None,
            })
            .collect()
    }
}
