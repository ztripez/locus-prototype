//! FL001 — boundary error leaks into a domain function signature.
//!
//! For every `AirFile` whose `module_path` matches any pattern in
//! `domain_paths`, inspect each `AirItem::Function`. If the function's
//! `return_type` parses as `Result<T, E>` (top level — generics inside T are
//! skipped over) and `E` matches any pattern in `boundary_error_patterns`,
//! fire one diagnostic.
//!
//! Severity: **Fatal** in both modes. Boundary errors leaking into domain
//! signatures is a structural failure: the layer edge that should have
//! wrapped the error in a domain error type didn't, and the failure has
//! already lost its owner by the time the function is called. Unlike the
//! mostly-heuristic FL futures, this one is deterministic — driven entirely
//! by signature-shape and explicit lockfile patterns — so the strict tier is
//! the right default. `CheckMode::elevate` is still applied for symmetry,
//! even though it's a no-op on Fatal.

use locus_air::{AirItem, AirWorkspace};

use super::super::lockfile_schema::{FlSection, matches_pattern};
use super::helpers::extract_result_error_type;
use crate::diagnostics::{CheckMode, Diagnostic, Severity};
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

pub fn fl001(air: &AirWorkspace, section: &FlSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.domain_paths.is_empty() || section.boundary_error_patterns.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(domain_pattern) = section
                .domain_paths
                .iter()
                .find(|pat| matches_pattern(pat, module_path))
            else {
                continue;
            };
            for item in &file.items {
                let AirItem::Function(func) = item else {
                    continue;
                };
                let Some(ret) = func.return_type.as_deref() else {
                    continue;
                };
                let Some(err_ty) = extract_result_error_type(ret) else {
                    continue;
                };
                let Some(boundary_pattern) = section
                    .boundary_error_patterns
                    .iter()
                    .find(|pat| matches_pattern(pat, err_ty))
                else {
                    continue;
                };
                out.push(fl001_diagnostic(
                    func,
                    module_path,
                    ret,
                    err_ty,
                    domain_pattern,
                    boundary_pattern,
                    mode,
                ));
            }
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn fl001_diagnostic(
    func: &locus_air::AirFunction,
    module_path: &str,
    ret: &str,
    err_ty: &str,
    domain_pattern: &str,
    boundary_pattern: &str,
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
        rule_id: "FL001".to_string(),
        severity: mode.elevate(Severity::Fatal),
        span: func.span.clone(),
        concept: None,
        message: format!(
            "domain function `{}` returns boundary error type `{}` \
             (matched domain pattern `{}`, boundary pattern `{}`)",
            func.name, err_ty, domain_pattern, boundary_pattern,
        ),
        why: vec![
            format!("module `{module_path}` matches domain pattern `{domain_pattern}`"),
            format!("function `{}` (`{}`)", func.name, func.symbol),
            format!("return type `{ret}`"),
            format!(
                "extracted error type `{err_ty}` matches boundary pattern \
                 `{boundary_pattern}`"
            ),
            "domain function signatures must speak the domain's error \
             vocabulary; transport / boundary errors leak the failure lineage \
             past the layer that should have wrapped them"
                .into(),
        ],
        suggested_fix: Some(format!(
            "wrap `{err_ty}` in a domain error type at the layer's edge — \
             either `impl From<{err_ty}> for <DomainError>` or an explicit \
             `map_err` at the boundary — so `{}` returns the domain error \
             instead",
            func.name,
        )),
    }
}

pub struct Fl001Rule;
pub static FL001_RULE: Fl001Rule = Fl001Rule;

const FL001_ID: RuleId = RuleId::new("FL001");
const FL001_PARADIGM: ParadigmId = ParadigmId::new("FL");

impl RuleDefinition for Fl001Rule {
    fn id(&self) -> RuleId {
        FL001_ID
    }
    fn paradigm(&self) -> ParadigmId {
        FL001_PARADIGM
    }
    fn title(&self) -> &'static str {
        "boundary error in domain function signature"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Fatal
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::super::lockfile_schema::FlSection;
        let section: FlSection = ctx.lockfile.paradigm_section("FL").unwrap_or_default();
        fl001(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(FL001_ID),
                rule_id: Some(FL001_ID),
                paradigm_id: Some(FL001_PARADIGM),
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
