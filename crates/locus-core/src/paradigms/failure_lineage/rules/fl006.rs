//! FL006 — `map_err(|_| ...)` losing source context.
//!
//! Reads [`AirItem::ClosureMethodCall`] items (AIR v10). Fires on a
//! `.map_err(...)` call whose closure discards its argument
//! (`closure_discards_arg == true`, i.e. `|_|` / `||` / `|_, x|`)
//! outside `invariant_owner_paths`. The original `Err` value is dropped
//! before it can be wrapped in the new error type, so failure lineage
//! is broken at the conversion site.
//!
//! Severity: `mode.elevate(Severity::Warning)` — Warning in human, Fatal
//! under `--agent-strict`. Same posture as FL002–FL005.
//!
//! Lockfile-driven silence: stays quiet until `invariant_owner_paths`
//! is populated.

use locus_air::{AirClosureMethodCall, AirItem, AirWorkspace};

use super::super::lockfile_schema::FlSection;
use super::helpers::callsite_in_invariant_owner;
use crate::diagnostics::{CheckMode, Diagnostic, Severity};
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

pub fn fl006(air: &AirWorkspace, section: &FlSection, mode: CheckMode) -> Vec<Diagnostic> {
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
                let AirItem::ClosureMethodCall(cmc) = item else {
                    continue;
                };
                if cmc.callee != "map_err" {
                    continue;
                }
                if !cmc.closure_discards_arg {
                    continue;
                }
                if callsite_in_invariant_owner(
                    module_path,
                    cmc.function.as_deref(),
                    &section.invariant_owner_paths,
                ) {
                    continue;
                }
                out.push(diagnostic_for_fl006(cmc, module_path, mode));
            }
        }
    }
    out
}

fn diagnostic_for_fl006(
    cmc: &AirClosureMethodCall,
    module_path: &str,
    mode: CheckMode,
) -> Diagnostic {
    let function_label = cmc
        .function
        .as_deref()
        .unwrap_or("<unknown enclosing function>");
    Diagnostic {
        rule_id: "FL006".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: cmc.span.clone(),
        concept: None,
        message: format!(
            "map_err(|_| ...) in `{module_path}` (fn `{function_label}`) discards \
             the original error — failure lineage broken at the conversion site"
        ),
        why: vec![
            format!("module `{module_path}`"),
            format!("enclosing function: `{function_label}`"),
            "closure pattern is `_` — original `Err` value is dropped".into(),
            "`map_err` should preserve or transform the source error, not erase it".into(),
        ],
        suggested_fix: Some(
            "replace |_| with |e| <transform>(e) so the source error is wrapped or \
             logged before being mapped to the new type; or accept the file via \
             `paradigms.FL.invariant_owner_paths` if this is a legitimate adapter \
             boundary that has already logged the source. For a one-off, suppress \
             with `// locus: allow FL006 reason=\"…\" expires=\"YYYY-MM-DD\"`"
                .into(),
        ),
    }
}

pub struct Fl006Rule;
pub static FL006_RULE: Fl006Rule = Fl006Rule;

const FL006_ID: RuleId = RuleId::new("FL006");
const FL006_PARADIGM: ParadigmId = ParadigmId::new("FL");

impl RuleDefinition for Fl006Rule {
    fn id(&self) -> RuleId {
        FL006_ID
    }
    fn paradigm(&self) -> ParadigmId {
        FL006_PARADIGM
    }
    fn title(&self) -> &'static str {
        "`map_err(|_|)` discarding error argument"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::super::lockfile_schema::FlSection;
        let section: FlSection = ctx.lockfile.paradigm_section("FL").unwrap_or_default();
        fl006(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(FL006_ID),
                rule_id: Some(FL006_ID),
                paradigm_id: Some(FL006_PARADIGM),
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
