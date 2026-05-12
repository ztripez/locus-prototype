//! FL003 — silent error discard.
//!
//! Catches the *opposite* failure mode from FL002. Where FL002 flags loud
//! panics that abort the process, FL003 flags **silent** discards: method
//! calls that convert a `Result` into a value-or-default without
//! propagating the error. Spec: `docs/PARADIGMS.md` line 804–807
//! (".ok() / unwrap_or_default masking, etc.").
//!
//! Detection is restricted to **method calls** (`AirCallSite` with
//! `kind == Method`) — bare-name `Function` calls and macros never carry
//! silent-discard semantics. Receiver-type resolution is out of AIR's
//! scope today, so we match purely on callee name; in practice the std
//! surface for `.ok()` / `.err()` is `Result`-only, so the
//! false-positive rate is low. Users who hit a legitimate non-Result
//! `.ok()` (e.g. via a third-party trait) suppress with
//! `// locus: allow FL003 reason="..." expires="..."`.
//!
//! Severity: Warning by default; Fatal under `--agent-strict`.
//!
//! Shares `invariant_owner_paths` with FL002. The semantics line up:
//! "modules where the rule's anti-pattern is legitimate" applies equally
//! to test fixtures that legitimately do `result.ok()` to assert
//! best-effort behaviour. Silent until `invariant_owner_paths` is
//! populated.

use locus_air::{AirCallSite, AirItem, AirWorkspace, CallKind};

use super::super::lockfile_schema::{FlSection, matches_pattern};
use super::helpers::callsite_in_invariant_owner;
use crate::diagnostics::{CheckMode, Diagnostic, Severity};
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

pub fn fl003(air: &AirWorkspace, section: &FlSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.invariant_owner_paths.is_empty() || section.silent_discard_callees.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            for item in &file.items {
                let AirItem::CallSite(cs) = item else {
                    continue;
                };
                // Method-only — `.ok()` is the smoking gun, and we don't
                // want to flag a free function happening to be named `ok`.
                if !matches!(cs.kind, CallKind::Method) {
                    continue;
                }
                if callsite_in_invariant_owner(
                    module_path,
                    cs.function.as_deref(),
                    &section.invariant_owner_paths,
                ) {
                    continue;
                }
                let last = cs.callee.rsplit("::").next().unwrap_or(&cs.callee);
                let Some(silent_pattern) = section
                    .silent_discard_callees
                    .iter()
                    .find(|pat| matches_pattern(pat, last))
                else {
                    continue;
                };
                out.push(diagnostic_for_fl003(cs, module_path, silent_pattern, mode));
            }
        }
    }
    out
}

fn diagnostic_for_fl003(
    cs: &AirCallSite,
    module_path: &str,
    silent_pattern: &str,
    mode: CheckMode,
) -> Diagnostic {
    let function_label = cs
        .function
        .as_deref()
        .unwrap_or("<unknown enclosing function>");
    Diagnostic {
        rule_id: "FL003".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: cs.span.clone(),
        concept: None,
        message: format!(
            "silent error discard `.{}()` in `{module_path}` (fn `{function_label}`) — \
             matches `paradigms.FL.silent_discard_callees` pattern `{silent_pattern}`",
            cs.callee,
        ),
        why: vec![
            format!("method call `.{}()`", cs.callee),
            format!("enclosing function: `{function_label}`"),
            format!(
                "module `{module_path}` does not match any \
                 `paradigms.FL.invariant_owner_paths` pattern"
            ),
            format!(
                "callee matches silent-discard pattern `{silent_pattern}` in \
                 `paradigms.FL.silent_discard_callees` — converts a `Result` \
                 into a value or `Option` without propagating the error"
            ),
        ],
        suggested_fix: Some(format!(
            "propagate the error with `?` and let the caller decide, or \
             explicitly handle the `Err` branch — `let value = result.{}()` \
             discards the failure lineage. If `{module_path}` is a legitimate \
             invariant owner (supervisor, test-support module), add it to \
             `paradigms.FL.invariant_owner_paths`. For a one-off intentional \
             discard, suppress with `// locus: allow FL003 reason=\"…\" \
             expires=\"YYYY-MM-DD\"`",
            cs.callee,
        )),
    }
}

pub struct Fl003Rule;
pub static FL003_RULE: Fl003Rule = Fl003Rule;

const FL003_ID: RuleId = RuleId::new("FL003");
const FL003_PARADIGM: ParadigmId = ParadigmId::new("FL");

impl RuleDefinition for Fl003Rule {
    fn id(&self) -> RuleId {
        FL003_ID
    }
    fn paradigm(&self) -> ParadigmId {
        FL003_PARADIGM
    }
    fn title(&self) -> &'static str {
        "silent error discard"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::super::lockfile_schema::FlSection;
        let section: FlSection = ctx.lockfile.paradigm_section("FL").unwrap_or_default();
        fl003(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(FL003_ID),
                rule_id: Some(FL003_ID),
                paradigm_id: Some(FL003_PARADIGM),
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
