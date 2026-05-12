//! BO rules.
//!
//! Implemented:
//! - [`bo001`]: domain/application file imports a transport- or
//!   persistence-style dependency. Conceptually adjacent to DG001 but uses
//!   BO's own lockfile shape (`domain_paths` × `forbidden_in_domain`) and is
//!   dedicated to the boundary-vs-domain split.
//! - [`bo002`]: function in a domain file exposes a persistence-shaped type
//!   in its parameter or return signature (`persistence_type_patterns`).
//! - [`bo004`]: canonical type carries a forbidden derive (e.g.
//!   `Serialize`/`Deserialize`) — domain types should not be coupled to
//!   serialization/schema frameworks.
//! - [`bo005`]: domain function performs a persistence write
//!   (`FactKind::PersistenceWrite`) — domain code must not write to storage
//!   directly; invert the dependency through a port.

use locus_air::{AirItem, AirSpan, AirWorkspace, FactKind, FactTarget};

use super::lockfile_schema::{BoSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

fn bo001_diagnostic(
    module_path: &str,
    import_path: &str,
    domain_pattern: &str,
    forbidden_pattern: &str,
    span: locus_air::AirSpan,
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
        rule_id: "BO001".to_string(),
        severity: mode.elevate(Severity::Fatal),
        span,
        concept: None,
        message: format!(
            "domain module `{module_path}` imports forbidden \
             transport/persistence path `{import_path}`"
        ),
        why: vec![
            format!(
                "importer `{module_path}` matches domain_paths pattern \
                 `{domain_pattern}`"
            ),
            format!(
                "import `{import_path}` matches forbidden_in_domain pattern \
                 `{forbidden_pattern}`"
            ),
            "domain/application code must not depend directly on transport, \
             persistence, or serialization frameworks; those concerns belong \
             at the boundary"
                .into(),
        ],
        suggested_fix: Some(
            "convert at the boundary (introduce a port/adapter, or move the \
             conversion into an application-layer service that calls the \
             framework on the domain's behalf); if the import is a \
             domain-friendly utility, narrow the `paradigms.BO.forbidden_in_domain` \
             pattern in `locus.lock` so it no longer matches"
                .into(),
        ),
    }
}

/// BO001 — domain/application file imports a forbidden transport/persistence
/// dependency.
///
/// For every `AirFile` whose `module_path` matches any pattern in
/// `domain_paths`, walk its `AirImport` items. Fire when the import path
/// matches any pattern in `forbidden_in_domain`.
///
/// Always Fatal: domain leakage of transport/persistence breaks the layered
/// architecture the user has declared via the lockfile — same justification
/// as DG001's forbidden edges.
pub fn bo001(air: &AirWorkspace, section: &BoSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.domain_paths.is_empty() || section.forbidden_in_domain.is_empty() {
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
                let AirItem::Import(imp) = item else {
                    continue;
                };
                let Some(forbidden_pattern) = section
                    .forbidden_in_domain
                    .iter()
                    .find(|pat| matches_pattern(pat, &imp.path))
                else {
                    continue;
                };
                out.push(bo001_diagnostic(
                    module_path,
                    &imp.path,
                    domain_pattern,
                    forbidden_pattern,
                    imp.span.clone(),
                    mode,
                ));
            }
        }
    }
    out
}

fn bo002_diagnostic(
    func: &locus_air::AirFunction,
    module_path: &str,
    domain_pattern: &str,
    position: &str,
    type_text: &str,
    persistence_pattern: &str,
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
        rule_id: "BO002".to_string(),
        severity: mode.elevate(Severity::Fatal),
        span: func.span.clone(),
        concept: None,
        message: format!(
            "domain function `{}` exposes persistence-shaped type \
             `{type_text}` in {position}",
            func.symbol
        ),
        why: vec![
            format!(
                "module `{module_path}` matches domain_paths pattern \
                 `{domain_pattern}`"
            ),
            format!(
                "{position} type `{type_text}` matches \
                 persistence_type_patterns pattern \
                 `{persistence_pattern}`"
            ),
            "domain functions must speak in domain types; \
             persistence-shaped values belong on the boundary, \
             translated by an adapter or repository"
                .into(),
        ],
        suggested_fix: Some(format!(
            "introduce a domain type and a converter at the \
             boundary; if `{type_text}` is genuinely a domain \
             concept (rare), narrow \
             `paradigms.BO.persistence_type_patterns` in `locus.lock` \
             so `{persistence_pattern}` no longer matches"
        )),
    }
}

/// BO002 — persistence type leaking into a domain function signature.
///
/// For every `AirFunction` whose containing `AirFile.module_path` matches any
/// pattern in `domain_paths`, fire when one of its parameter types or its
/// return type matches any pattern in `persistence_type_patterns` (textual
/// match against the rendered `type_text`).
///
/// Severity: Fatal — same justification as BO001. A `sqlx::PgRow` parameter
/// in a domain function couples the domain to the persistence framework just
/// as surely as importing it would; the import-site check (BO001) wouldn't
/// catch the case where a re-export brings the type in under a different
/// path. This rule is the signature-level companion.
///
/// Silent when either `domain_paths` or `persistence_type_patterns` is empty.
pub fn bo002(air: &AirWorkspace, section: &BoSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.domain_paths.is_empty() || section.persistence_type_patterns.is_empty() {
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
                // Check parameters first, then return type — first hit wins.
                let mut hit: Option<(String, String, String)> = None;
                for (pname, ptype) in &func.params {
                    if let Some(p) = section
                        .persistence_type_patterns
                        .iter()
                        .find(|pat| type_text_matches(pat, ptype))
                    {
                        hit = Some((format!("parameter `{pname}`"), ptype.clone(), p.clone()));
                        break;
                    }
                }
                if hit.is_none()
                    && let Some(ret) = func.return_type.as_deref()
                    && let Some(p) = section
                        .persistence_type_patterns
                        .iter()
                        .find(|pat| type_text_matches(pat, ret))
                {
                    hit = Some(("return type".to_string(), ret.to_string(), p.clone()));
                }
                let Some((position, type_text, persistence_pattern)) = hit else {
                    continue;
                };
                out.push(bo002_diagnostic(
                    func,
                    module_path,
                    domain_pattern,
                    &position,
                    &type_text,
                    &persistence_pattern,
                    mode,
                ));
            }
        }
    }
    out
}

/// Match a `persistence_type_patterns` entry against an `AirFunction`
/// `type_text`. The rendered `type_text` may include borrows, generics,
/// commas, paths, etc. (e.g. `&sqlx::PgRow`, `Vec<sea_orm::DbErr>`,
/// `Result<Foo, diesel::result::Error>`). We use a substring-aware match
/// over the path-shaped portions: any contiguous path-like fragment in
/// `type_text` is fed through [`matches_pattern`] against the pattern.
fn type_text_matches(pattern: &str, type_text: &str) -> bool {
    // Fast path: exact whole-text match (covers patterns without wildcards
    // and bare type texts like `sqlx::PgRow`).
    if matches_pattern(pattern, type_text) {
        return true;
    }
    // Tokenize on characters that can't appear inside a Rust path. The
    // remaining chunks are candidate path-shaped fragments.
    for fragment in type_text.split(|c: char| !(c.is_alphanumeric() || c == ':' || c == '_')) {
        if fragment.is_empty() {
            continue;
        }
        if matches_pattern(pattern, fragment) {
            return true;
        }
    }
    false
}

fn bo004_diagnostic(
    ty: &locus_air::AirType,
    module_path: &str,
    canonical_pattern: &str,
    derive: &str,
    forbidden: &str,
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
        rule_id: "BO004".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: ty.span.clone(),
        concept: None,
        message: format!(
            "canonical type `{}` carries forbidden derive `{derive}`",
            ty.symbol
        ),
        why: vec![
            format!(
                "module `{module_path}` matches canonical_paths \
                 pattern `{canonical_pattern}`"
            ),
            format!(
                "derive `{derive}` matches \
                 forbidden_canonical_derives entry `{forbidden}`"
            ),
            "canonical domain types must not depend on \
             serialization/schema frameworks; serialization \
             belongs on a boundary DTO"
                .into(),
        ],
        suggested_fix: Some(format!(
            "remove `{derive}` from `{}` and introduce a \
             boundary DTO that does carry the derive plus a \
             converter; if the derive is genuinely needed on \
             the canonical (e.g. fixture/config), accept it \
             via `paradigms.BO.forbidden_canonical_derives` in \
             `locus.lock`",
            ty.name
        )),
    }
}

/// BO004 — accepted canonical type carries a forbidden derive.
///
/// For every `AirItem::Type` whose containing `AirFile.module_path` matches a
/// `canonical_paths` pattern, fire when any of its `derives` matches a name
/// in `forbidden_canonical_derives`. The point: canonical domain types
/// shouldn't depend on serialization/schema frameworks (`Serialize`,
/// `Deserialize`, `ToSchema`, etc.) — those concerns belong at the boundary,
/// where DTO types do the marshalling.
///
/// Match semantics: derive entries in `forbidden_canonical_derives` are
/// matched as **trait short names**. We compare against both the literal
/// derive token (e.g. `serde::Serialize`) and its last `::` segment
/// (`Serialize`) so a configuration of `["Serialize"]` works whether the
/// derive was authored qualified or unqualified.
///
/// Severity: Warning — having `Serialize` on a canonical type is sloppy but
/// not a hard structural break. Elevated to Fatal under `--agent-strict`.
///
/// Silent when `canonical_paths` is empty (no types are nominated as
/// canonical, so there's nothing to enforce).
pub fn bo004(air: &AirWorkspace, section: &BoSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.canonical_paths.is_empty() || section.forbidden_canonical_derives.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(canonical_pattern) = section
                .canonical_paths
                .iter()
                .find(|pat| matches_pattern(pat, module_path))
            else {
                continue;
            };
            for item in &file.items {
                let AirItem::Type(ty) = item else {
                    continue;
                };
                // BO004 is narrow to Derive decorators (Rust-only).
                for decorator in ty
                    .decorators
                    .iter()
                    .filter(|d| matches!(d.source, locus_air::DecoratorSource::Derive))
                {
                    let derive = &decorator.name;
                    let short = derive.rsplit("::").next().unwrap_or(derive.as_str());
                    let Some(forbidden) = section
                        .forbidden_canonical_derives
                        .iter()
                        .find(|d| d.as_str() == derive.as_str() || d.as_str() == short)
                    else {
                        continue;
                    };
                    out.push(bo004_diagnostic(
                        ty,
                        module_path,
                        canonical_pattern,
                        derive,
                        forbidden,
                        mode,
                    ));
                    break; // one diagnostic per type
                }
            }
        }
    }
    out
}

fn bo005_diagnostic(
    fact: &locus_air::AirFact,
    symbol: &str,
    module_path: &str,
    fn_span: AirSpan,
    domain_pattern: &str,
    mode: CheckMode,
) -> Diagnostic {
    let evidence = fact.evidence.as_deref().unwrap_or("persistence write");
    let span = match &fact.target {
        FactTarget::Span(s) => s.clone(),
        FactTarget::Function { .. } | FactTarget::File { .. } => fn_span,
    };
    let mut why = vec![format!(
        "module `{module_path}` (or function `{symbol}`) matches \
         domain_paths pattern `{domain_pattern}`"
    )];
    if fact.reasons.is_empty() {
        why.push("loader detected persistence-write-shaped call".to_string());
    } else {
        for r in &fact.reasons {
            why.push(r.clone());
        }
    }
    if let Some(ev) = fact.evidence.as_deref() {
        why.push(format!("evidence: `{ev}`"));
    }
    why.push(
        "domain code must not write to storage directly; persistence \
         belongs at the boundary, behind a port (Repository/Storage \
         trait) implemented by an adapter"
            .to_string(),
    );
    Diagnostic {
        rule_id: "BO005".to_string(),
        severity: mode.elevate(Severity::Fatal),
        span,
        concept: None,
        message: format!(
            "domain function `{symbol}` performs persistence write \
             `{evidence}` — domain code must not write to storage \
             directly"
        ),
        why,
        suggested_fix: Some(
            "invert the dependency: define a port (e.g. a `Repository` \
             or `Storage` trait) in the domain layer, implement it in \
             an adapter, and inject the adapter from the composition \
             root. The domain function then calls `repo.save(...)` \
             instead of touching storage directly. If this module is \
             actually outside the domain, narrow \
             `paradigms.BO.domain_paths` in `locus.lock`."
                .to_string(),
        ),
    }
}

/// BO005 — persistence write inside a domain function.
///
/// For every `FactKind::PersistenceWrite` fact whose target is a function
/// symbol, look up the function's enclosing file and fire when **either**
/// the file's `module_path` **or** the function symbol matches any pattern
/// in `domain_paths`. The function-symbol check catches inline
/// `mod tests {}` blocks: their symbols sit at a deeper path than the file
/// (`pkg::mod::tests::case`), so a `*::tests::*` carve-out wouldn't reach
/// them via the file-only check.
///
/// Severity: Fatal — same posture as BO001/BO002. This is a structural
/// domain leak: the std-rt loader emits these for `std::fs::write`,
/// `std::fs::create_dir*`, `std::fs::remove_*`, etc., and any of them
/// inside a domain function couples the domain to the storage substrate.
///
/// Silent when `domain_paths` is empty (BO is opt-in by lockfile, like
/// the rest of the paradigm). Non-`PersistenceWrite` facts and facts
/// targeting files/spans (rather than function symbols) are skipped.
pub fn bo005(air: &AirWorkspace, section: &BoSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.domain_paths.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for fact in &air.facts {
        if fact.kind != FactKind::PersistenceWrite {
            continue;
        }
        let FactTarget::Function { symbol } = &fact.target else {
            continue;
        };
        let Some((module_path, fn_span)) = lookup_function(air, symbol) else {
            continue;
        };
        let Some(domain_pattern) = section
            .domain_paths
            .iter()
            .find(|pat| matches_pattern(pat, module_path) || matches_pattern(pat, symbol))
        else {
            continue;
        };
        out.push(bo005_diagnostic(
            fact,
            symbol,
            module_path,
            fn_span,
            domain_pattern,
            mode,
        ));
    }
    out
}

/// Walk every package/file/item, returning the enclosing file's
/// `module_path` and the function's span for the first `AirFunction`
/// whose `symbol` matches. Mirrors the helper in
/// `runtime_work/rules.rs::lookup_function`; duplicated here so paradigms
/// don't import each other.
fn lookup_function<'a>(air: &'a AirWorkspace, symbol: &str) -> Option<(&'a str, AirSpan)> {
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

// ── RuleDefinition impls (governance spine migration, epic #71) ──────────────

use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

const BO_PARADIGM: ParadigmId = ParadigmId::new("BO");
const BO001_ID: RuleId = RuleId::new("BO001");
const BO002_ID: RuleId = RuleId::new("BO002");
const BO004_ID: RuleId = RuleId::new("BO004");
const BO005_ID: RuleId = RuleId::new("BO005");

pub struct Bo001Rule;
pub static BO001_RULE: Bo001Rule = Bo001Rule;

impl RuleDefinition for Bo001Rule {
    fn id(&self) -> RuleId {
        BO001_ID
    }
    fn paradigm(&self) -> ParadigmId {
        BO_PARADIGM
    }
    fn title(&self) -> &'static str {
        "domain type leaks through boundary signature"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Fatal
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::lockfile_schema::BoSection;
        let section: BoSection = ctx.lockfile.paradigm_section("BO").unwrap_or_default();
        bo001(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(BO001_ID),
                rule_id: Some(BO001_ID),
                paradigm_id: Some(BO_PARADIGM),
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

pub struct Bo002Rule;
pub static BO002_RULE: Bo002Rule = Bo002Rule;

impl RuleDefinition for Bo002Rule {
    fn id(&self) -> RuleId {
        BO002_ID
    }
    fn paradigm(&self) -> ParadigmId {
        BO_PARADIGM
    }
    fn title(&self) -> &'static str {
        "boundary type used as canonical"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Fatal
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::lockfile_schema::BoSection;
        let section: BoSection = ctx.lockfile.paradigm_section("BO").unwrap_or_default();
        bo002(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(BO002_ID),
                rule_id: Some(BO002_ID),
                paradigm_id: Some(BO_PARADIGM),
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

pub struct Bo004Rule;
pub static BO004_RULE: Bo004Rule = Bo004Rule;

impl RuleDefinition for Bo004Rule {
    fn id(&self) -> RuleId {
        BO004_ID
    }
    fn paradigm(&self) -> ParadigmId {
        BO_PARADIGM
    }
    fn title(&self) -> &'static str {
        "boundary type co-defined with canonical concept"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::lockfile_schema::BoSection;
        let section: BoSection = ctx.lockfile.paradigm_section("BO").unwrap_or_default();
        bo004(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(BO004_ID),
                rule_id: Some(BO004_ID),
                paradigm_id: Some(BO_PARADIGM),
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

pub struct Bo005Rule;
pub static BO005_RULE: Bo005Rule = Bo005Rule;

impl RuleDefinition for Bo005Rule {
    fn id(&self) -> RuleId {
        BO005_ID
    }
    fn paradigm(&self) -> ParadigmId {
        BO_PARADIGM
    }
    fn title(&self) -> &'static str {
        "persistence call in domain layer"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Fatal
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::lockfile_schema::BoSection;
        let section: BoSection = ctx.lockfile.paradigm_section("BO").unwrap_or_default();
        bo005(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(BO005_ID),
                rule_id: Some(BO005_ID),
                paradigm_id: Some(BO_PARADIGM),
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
