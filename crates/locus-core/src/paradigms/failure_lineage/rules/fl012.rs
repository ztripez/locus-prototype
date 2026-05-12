//! FL012 — retry-shaped loop without accepted retry policy.
//!
//! Reads [`AirItem::RetryLoop`] items (AIR v12). Fires when:
//!
//! - `propagates: true` (the loop body uses `?`),
//! - `has_break: true` (there's a success-exit path), and
//! - the file's `module_path` (or the function's containing module)
//!   is **not** in `retry_policy_owner_paths`.
//!
//! The user declares which modules legitimately implement retry policies
//! (backoff, max attempts, jitter). Loops elsewhere are likely ad-hoc
//! retries — repeated fallible work without the cross-cutting policy
//! concerns a real retry needs.
//!
//! Severity: `mode.elevate(Severity::Warning)` — Warning in human,
//! Fatal under `--agent-strict`.
//!
//! Lockfile-driven silence: stays quiet until `retry_policy_owner_paths`
//! is populated. Same UX shape as the other FL lockfile-driven rules.

use locus_air::{AirItem, AirRetryLoop, AirWorkspace, LoopKind};

use super::super::lockfile_schema::FlSection;
use super::helpers::callsite_in_invariant_owner;
use crate::diagnostics::{CheckMode, Diagnostic, Severity};
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

pub fn fl012(air: &AirWorkspace, section: &FlSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.retry_policy_owner_paths.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            for item in &file.items {
                let AirItem::RetryLoop(loopy) = item else {
                    continue;
                };
                if !loopy.propagates {
                    continue;
                }
                if !loopy.has_break {
                    continue;
                }
                if callsite_in_invariant_owner(
                    module_path,
                    loopy.function.as_deref(),
                    &section.retry_policy_owner_paths,
                ) {
                    continue;
                }
                out.push(diagnostic_for_fl012(loopy, module_path, mode));
            }
        }
    }
    out
}

fn diagnostic_for_fl012(loopy: &AirRetryLoop, module_path: &str, mode: CheckMode) -> Diagnostic {
    let function_label = loopy
        .function
        .as_deref()
        .unwrap_or("<unknown enclosing function>");
    let kind_label = loop_kind_label(loopy.loop_kind);
    Diagnostic {
        rule_id: "FL012".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: loopy.span.clone(),
        concept: None,
        message: format!(
            "retry-shaped {kind_label} loop in `{module_path}` (fn `{function_label}`) \
             — propagation + break with no declared retry policy"
        ),
        why: vec![
            format!("module `{module_path}`"),
            format!("enclosing function: `{function_label}`"),
            format!("loop kind: `{kind_label}`"),
            "loop body uses `?` and contains `break` — fits the \
             retry-without-policy shape"
                .into(),
            format!(
                "no `paradigms.FL.retry_policy_owner_paths` entry covers \
                 `{module_path}` — the rule treats this site as ad-hoc"
            ),
        ],
        suggested_fix: Some(format!(
            "extract the retry into a declared retry-policy module that \
             owns backoff, max attempts, and jitter; or, if `{module_path}` \
             is a legitimate retry owner, add it to \
             `paradigms.FL.retry_policy_owner_paths`. For a one-off accepted \
             retry, suppress with `// locus: allow FL012 reason=\"…\" \
             expires=\"YYYY-MM-DD\"`"
        )),
    }
}

/// Render a [`LoopKind`] for diagnostic messages. Lower-case so the
/// headline reads "retry-shaped loop / for / while loop in `…`".
fn loop_kind_label(kind: LoopKind) -> &'static str {
    match kind {
        LoopKind::Loop => "loop",
        LoopKind::For => "for",
        LoopKind::While => "while",
    }
}

pub struct Fl012Rule;
pub static FL012_RULE: Fl012Rule = Fl012Rule;

const FL012_ID: RuleId = RuleId::new("FL012");
const FL012_PARADIGM: ParadigmId = ParadigmId::new("FL");

impl RuleDefinition for Fl012Rule {
    fn id(&self) -> RuleId {
        FL012_ID
    }
    fn paradigm(&self) -> ParadigmId {
        FL012_PARADIGM
    }
    fn title(&self) -> &'static str {
        "retry-shaped loop without declared policy"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::super::lockfile_schema::FlSection;
        let section: FlSection = ctx.lockfile.paradigm_section("FL").unwrap_or_default();
        fl012(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(FL012_ID),
                rule_id: Some(FL012_ID),
                paradigm_id: Some(FL012_PARADIGM),
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
