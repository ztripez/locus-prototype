//! PA rule implementations.
//!
//! Implemented:
//! - [`pa001`]: trait declared and immediately implemented in the same file
//!   (co-located port and adapter — the port wasn't actually abstracted).
//! - [`pa002`]: application/domain file imports a concrete adapter framework
//!   (`reqwest::*`, `sqlx::*`, …) — that's an adapter detail, not domain
//!   concern.
//! - [`pa003`]: application function performs an external-IO call directly
//!   instead of going through a declared port.
//! - [`pa004`]: an adapter type is constructed outside any composition
//!   root / bootstrap / composition module.

use std::collections::BTreeMap;

use locus_air::{
    ActionKind, AirFact, AirImplBlock, AirItem, AirSpan, AirWorkspace, FactKind, FactTarget,
    TypeKind,
};

use super::lockfile_schema::{PaSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

fn pa001_diagnostic(ty: &locus_air::AirType, imp: &AirImplBlock, mode: CheckMode) -> Diagnostic {
    Diagnostic {
        rule_id: "PA001".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: ty.span.clone(),
        concept: None,
        message: format!(
            "trait `{}` and its only impl (`{}`) share file `{}`",
            ty.name, imp.target_type, ty.span.file
        ),
        why: vec![
            format!("trait `{}` declared in `{}`", ty.symbol, ty.span.file),
            format!(
                "sole impl is `impl {} for {}` in the same file",
                ty.name, imp.target_type
            ),
            "no `accepted_colocated_traits` pattern matched".into(),
        ],
        suggested_fix: Some(format!(
            "move `{}` to a ports module (typically `application::ports::*`) and the \
             impl for `{}` to an adapter/infrastructure module; if this trait is a \
             genuine utility helper rather than a port, accept it via \
             `paradigms.PA.accepted_colocated_traits` in `locus.lock`",
            ty.name, imp.target_type
        )),
    }
}

/// PA001 — port and its sole impl in the same file.
///
/// A trait declared and immediately implemented in the same file is the
/// classic "I made a port to abstract this thing, but I never actually
/// abstracted anything" smell. Ports belong in `application::ports::*`,
/// adapters in `infrastructure::*` or boundary modules — physical separation
/// is the whole point of the port/adapter split.
///
/// Algorithm:
/// - For every `AirItem::Type` with `kind: TypeKind::Trait`, find its impls
///   by short name (last `::` segment of `trait_path`).
/// - If exactly one impl exists AND that impl's `span.file` equals the
///   trait's `span.file`, fire PA001.
/// - Skip if zero impls (intentionally-uninhabited trait — that's AB's
///   problem) or 2+ impls (already cross-file split, by definition).
/// - Skip if the trait's symbol or short name matches any pattern in
///   `accepted_colocated_traits`.
///
/// Severity: Warning by default; elevated to Fatal under `--agent-strict`.
/// Check a single trait type for PA001 (co-located port+adapter).
/// Returns `Some(Diagnostic)` when the rule fires.
fn pa001_check_trait<'a>(
    ty: &'a locus_air::AirType,
    trait_to_impls: &'a BTreeMap<&str, Vec<&'a AirImplBlock>>,
    section: &PaSection,
    mode: CheckMode,
) -> Option<Diagnostic> {
    if ty.kind != TypeKind::Trait {
        return None;
    }
    let impls = trait_to_impls.get(ty.name.as_str())?;
    if impls.len() != 1 {
        return None;
    }
    let imp = impls[0];
    if imp.span.file != ty.span.file {
        return None;
    }
    let exempted = section
        .accepted_colocated_traits
        .iter()
        .any(|pat| matches_pattern(pat, &ty.symbol) || matches_pattern(pat, &ty.name));
    if exempted {
        return None;
    }
    Some(pa001_diagnostic(ty, imp, mode))
}

pub fn pa001(air: &AirWorkspace, section: &PaSection, mode: CheckMode) -> Vec<Diagnostic> {
    let trait_to_impls = build_trait_to_impls(air);
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Type(ty) = item else { continue };
                if let Some(d) = pa001_check_trait(ty, &trait_to_impls, section, mode) {
                    out.push(d);
                }
            }
        }
    }
    out
}

/// Index every `AirItem::Impl` with a `trait_path` by the trait's short name
/// (last `::` segment). Inherent impls (`trait_path: None`) are excluded —
/// they aren't port implementations.
fn build_trait_to_impls(air: &AirWorkspace) -> BTreeMap<&str, Vec<&AirImplBlock>> {
    let mut out: BTreeMap<&str, Vec<&AirImplBlock>> = BTreeMap::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Impl(imp) = item else {
                    continue;
                };
                let Some(tp) = imp.interface.as_deref() else {
                    continue;
                };
                let short = tp.rsplit("::").next().unwrap_or(tp);
                out.entry(short).or_default().push(imp);
            }
        }
    }
    out
}

fn pa002_diagnostic(
    module_path: &str,
    imp: &locus_air::AirImport,
    application_pattern: &str,
    adapter_pattern: &str,
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
        rule_id: "PA002".to_string(),
        severity: mode.elevate(Severity::Fatal),
        span: imp.span.clone(),
        concept: None,
        message: format!(
            "application/domain module `{module_path}` imports concrete adapter `{}`",
            imp.path
        ),
        why: vec![
            format!(
                "module `{module_path}` matches application_paths \
                 pattern `{application_pattern}`"
            ),
            format!(
                "import `{}` matches concrete_adapter_patterns \
                 pattern `{adapter_pattern}`",
                imp.path
            ),
            "application/domain code must depend on ports (traits), \
             not concrete adapters; the adapter belongs at the \
             boundary"
                .into(),
        ],
        suggested_fix: Some(format!(
            "introduce a port (trait) the application depends on, \
             move the `{}` usage into an infrastructure adapter \
             that implements the port; if the import is genuinely \
             a non-adapter utility, narrow \
             `paradigms.PA.concrete_adapter_patterns` in `locus.lock`",
            imp.path
        )),
    }
}

/// PA002 — concrete adapter import in application/domain layer.
///
/// For each `AirItem::Import` in a file whose `module_path` matches a pattern
/// in `application_paths`, fire when the import's `path` matches a pattern in
/// `concrete_adapter_patterns`.
///
/// Severity: Fatal — application/domain code reaching directly into a
/// concrete adapter (`reqwest::Client`, `sqlx::PgPool`, …) breaks the
/// port/adapter split that PA defends. Same justification as BO001/DG001
/// for forbidden edges.
///
/// Silent until BOTH `application_paths` and `concrete_adapter_patterns`
/// are populated.
pub fn pa002(air: &AirWorkspace, section: &PaSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.application_paths.is_empty() || section.concrete_adapter_patterns.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(application_pattern) = section
                .application_paths
                .iter()
                .find(|pat| matches_pattern(pat, module_path))
            else {
                continue;
            };
            for item in &file.items {
                let AirItem::Import(imp) = item else {
                    continue;
                };
                let Some(adapter_pattern) = section
                    .concrete_adapter_patterns
                    .iter()
                    .find(|pat| matches_pattern(pat, &imp.path))
                else {
                    continue;
                };
                out.push(pa002_diagnostic(
                    module_path,
                    imp,
                    application_pattern,
                    adapter_pattern,
                    mode,
                ));
            }
        }
    }
    out
}

/// PA003 — application function performs an external-IO call without
/// going through a declared port.
///
/// For every `FactKind::ExternalIo` fact whose `target` is a `Function`,
/// resolve the function's enclosing module path and fire when that path
/// matches one of `application_paths`. The `std-rt` loader emits these
/// facts for `std::process::Command::*`, `std::net::*`, and similar
/// outbound primitives. Application code reaching directly into those
/// primitives bypasses the port layer entirely — same posture as PA002,
/// just enforced against runtime call-site evidence rather than imports.
///
/// Severity: Fatal — structural; agent-strict already elevates anything.
///
/// Silent when `application_paths` is empty: same opt-in UX as PA002.
///
/// Module-path resolution: the file's `module_path` is checked first; if
/// no match, the function symbol itself is matched against the same
/// patterns. This lets an `*::tests::*` carve-out cover inline `mod tests`
/// blocks that live at a deeper symbol path than the file.
pub fn pa003(air: &AirWorkspace, section: &PaSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.application_paths.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for fact in &air.facts {
        if fact.kind != FactKind::ExternalIo {
            continue;
        }
        let FactTarget::Function { symbol } = &fact.target else {
            continue;
        };
        let Some((module_path, fn_span)) = pa_lookup_function(air, symbol) else {
            continue;
        };
        let matched_pattern = section
            .application_paths
            .iter()
            .find(|pat| matches_pattern(pat, module_path) || matches_pattern(pat, symbol));
        let Some(matched_pattern) = matched_pattern else {
            continue;
        };
        out.push(pa003_diagnostic(
            fact,
            symbol,
            module_path,
            fn_span,
            matched_pattern,
            mode,
        ));
    }
    out
}

/// Resolve `symbol` against AIR. Returns the enclosing file's module_path
/// and the function's span. Mirrors `runtime_work::rules::lookup_function`.
fn pa_lookup_function<'a>(air: &'a AirWorkspace, symbol: &str) -> Option<(&'a str, AirSpan)> {
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                if let AirItem::Function(f) = item
                    && f.symbol == symbol
                {
                    let module = file.module_path.as_deref()?;
                    return Some((module, f.span.clone()));
                }
            }
        }
    }
    None
}

fn pa003_diagnostic(
    fact: &AirFact,
    symbol: &str,
    module_path: &str,
    fn_span: AirSpan,
    matched_pattern: &str,
    mode: CheckMode,
) -> Diagnostic {
    let span = match &fact.target {
        FactTarget::Span(s) => s.clone(),
        FactTarget::Function { .. } | FactTarget::File { .. } => fn_span,
    };
    let evidence = fact.evidence.as_deref().unwrap_or("?");
    let mut why = vec![
        format!("module `{module_path}` matches application_paths pattern `{matched_pattern}`"),
        format!("external-IO evidence: `{evidence}`"),
    ];
    for r in &fact.reasons {
        why.push(r.clone());
    }
    why.push(
        "external IO must be abstracted behind a port (trait) and \
         implemented in an adapter, not called directly from application code"
            .into(),
    );
    Diagnostic {
        rule_id: "PA003".to_string(),
        severity: mode.elevate(Severity::Fatal),
        span,
        concept: None,
        message: format!(
            "application function `{symbol}` performs external IO `{evidence}` \
             without going through a declared port"
        ),
        why,
        suggested_fix: Some(format!(
            "introduce a port trait for the IO concept (e.g. `HttpClient`, \
             `ProcessRunner`, `Network`), implement it in an adapter module, \
             and inject the adapter through the composition root. The \
             application function `{symbol}` should depend on the trait, not \
             reach for `{evidence}` directly. If `{module_path}` is not \
             actually application code, narrow `paradigms.PA.application_paths` \
             in `locus.lock`."
        )),
    }
}

fn pa004_diagnostic(
    a: &locus_air::AirTruthAction,
    module_label: &str,
    function_label: &str,
    adapter_pattern: &str,
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
        rule_id: "PA004".to_string(),
        severity: mode.elevate(Severity::Fatal),
        span: a.span.clone(),
        concept: None,
        message: format!(
            "adapter `{}` constructed in module `{module_label}` \
             outside any accepted construction path",
            a.target
        ),
        why: vec![
            format!(
                "target `{}` matches adapter_type_patterns pattern `{adapter_pattern}`",
                a.target
            ),
            format!(
                "module `{module_label}` matches none of the \
                 `accepted_construction_paths` patterns"
            ),
            format!("enclosing function: `{function_label}`"),
        ],
        suggested_fix: Some(format!(
            "move the construction of `{}` into a composition \
             root (e.g. `main`, a `bootstrap` module, or a \
             declared `composition::*` module); if `{module_label}` \
             is itself a legitimate composition site, add it to \
             `paradigms.PA.accepted_construction_paths` in \
             `locus.lock`",
            a.target
        )),
    }
}

/// PA004 — adapter construction outside composition root.
///
/// For each `AirItem::TruthAction { action: Construct, target }`, fire when
/// `target` matches a pattern in `adapter_type_patterns` AND the action's
/// enclosing file (`AirFile.module_path`) does NOT match any pattern in
/// `accepted_construction_paths`.
///
/// Severity: Fatal — adapters constructed outside the composition root
/// undermine the whole point of having one.
///
/// Silent when `adapter_type_patterns` is empty. Defaults populate
/// `accepted_construction_paths` so the user only needs to opt in by listing
/// adapter types.
/// Check a single file's Construct actions for PA004. Appends diagnostics.
fn pa004_check_file(
    file: &locus_air::AirFile,
    module_path: &str,
    section: &PaSection,
    mode: CheckMode,
    out: &mut Vec<Diagnostic>,
) {
    let module_label = if module_path.is_empty() {
        "(unknown module)"
    } else {
        module_path
    };
    for item in &file.items {
        let AirItem::TruthAction(a) = item else {
            continue;
        };
        if a.action != ActionKind::Construct {
            continue;
        }
        let Some(adapter_pattern) = section
            .adapter_type_patterns
            .iter()
            .find(|pat| matches_pattern(pat, &a.target))
        else {
            continue;
        };
        let function_label = a
            .function
            .as_deref()
            .unwrap_or("(no enclosing function recorded)");
        out.push(pa004_diagnostic(
            a,
            module_label,
            function_label,
            adapter_pattern,
            mode,
        ));
    }
}

pub fn pa004(air: &AirWorkspace, section: &PaSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.adapter_type_patterns.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let module_path = file.module_path.as_deref().unwrap_or("");
            if section
                .accepted_construction_paths
                .iter()
                .any(|pat| matches_pattern(pat, module_path))
            {
                continue; // file is itself an accepted construction path
            }
            pa004_check_file(file, module_path, section, mode, &mut out);
        }
    }
    out
}

// ── RuleDefinition impls (governance spine migration, epic #71) ──────────────

use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

const PA_PARADIGM: ParadigmId = ParadigmId::new("PA");
const PA001_ID: RuleId = RuleId::new("PA001");
const PA002_ID: RuleId = RuleId::new("PA002");
const PA003_ID: RuleId = RuleId::new("PA003");
const PA004_ID: RuleId = RuleId::new("PA004");

pub struct Pa001Rule;
pub static PA001_RULE: Pa001Rule = Pa001Rule;

impl RuleDefinition for Pa001Rule {
    fn id(&self) -> RuleId {
        PA001_ID
    }
    fn paradigm(&self) -> ParadigmId {
        PA_PARADIGM
    }
    fn title(&self) -> &'static str {
        "port and its sole impl co-located in same file"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::lockfile_schema::PaSection;
        let section: PaSection = ctx.lockfile.paradigm_section("PA").unwrap_or_default();
        pa001(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(PA001_ID),
                rule_id: Some(PA001_ID),
                paradigm_id: Some(PA_PARADIGM),
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

pub struct Pa002Rule;
pub static PA002_RULE: Pa002Rule = Pa002Rule;

impl RuleDefinition for Pa002Rule {
    fn id(&self) -> RuleId {
        PA002_ID
    }
    fn paradigm(&self) -> ParadigmId {
        PA_PARADIGM
    }
    fn title(&self) -> &'static str {
        "concrete adapter import in application/domain layer"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Fatal
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::lockfile_schema::PaSection;
        let section: PaSection = ctx.lockfile.paradigm_section("PA").unwrap_or_default();
        pa002(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(PA002_ID),
                rule_id: Some(PA002_ID),
                paradigm_id: Some(PA_PARADIGM),
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

pub struct Pa003Rule;
pub static PA003_RULE: Pa003Rule = Pa003Rule;

impl RuleDefinition for Pa003Rule {
    fn id(&self) -> RuleId {
        PA003_ID
    }
    fn paradigm(&self) -> ParadigmId {
        PA_PARADIGM
    }
    fn title(&self) -> &'static str {
        "external IO in application without port"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Fatal
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::lockfile_schema::PaSection;
        let section: PaSection = ctx.lockfile.paradigm_section("PA").unwrap_or_default();
        pa003(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(PA003_ID),
                rule_id: Some(PA003_ID),
                paradigm_id: Some(PA_PARADIGM),
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

pub struct Pa004Rule;
pub static PA004_RULE: Pa004Rule = Pa004Rule;

impl RuleDefinition for Pa004Rule {
    fn id(&self) -> RuleId {
        PA004_ID
    }
    fn paradigm(&self) -> ParadigmId {
        PA_PARADIGM
    }
    fn title(&self) -> &'static str {
        "adapter construction outside composition root"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Fatal
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::lockfile_schema::PaSection;
        let section: PaSection = ctx.lockfile.paradigm_section("PA").unwrap_or_default();
        pa004(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(PA004_ID),
                rule_id: Some(PA004_ID),
                paradigm_id: Some(PA_PARADIGM),
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

#[cfg(test)]
#[path = "rules_tests.rs"]
mod rules_tests;
