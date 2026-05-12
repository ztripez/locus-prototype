//! TA rule implementations.
//!
//! Implemented:
//! - [`ta001`]: test module defines a public domain-shaped type. Test
//!   fixtures duplicating domain concepts as public types is the "we made our
//!   own `User` struct in tests" pattern the spec calls out — domain truth
//!   should live on the canonical path, not in a test-local public clone.
//! - [`ta002`]: test type whose name overlaps an accepted canonical concept.
//!   The user lists their accepted canonical type names in
//!   `canonical_name_patterns`; any type defined inside `test_paths` whose
//!   name matches is flagged regardless of visibility.
//! - [`ta003`]: test struct whose name *and* field-name set both echo a
//!   canonical concept. Cross-checks `canonical_name_patterns` (looser
//!   "contains" match) with `canonical_field_sets` Jaccard overlap >= 0.5.
//! - [`ta004`]: a port-trait `impl` landing inside test code outside the
//!   declared `accepted_test_adapter_paths` — agent-introduced fake
//!   adapters that bypass the project's accepted test-adapter home.
//!
//! Mirrors UT001 in shape (lockfile-driven module pattern match, fires on
//! public types) but with a different fix narrative: demote to non-`pub`,
//! lift to a real production module, or accept as a shared fixture surface
//! (future TA mechanism).

use std::collections::BTreeSet;

use locus_air::{AirItem, AirWorkspace, TypeKind, Visibility};

use super::lockfile_schema::{TaSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

fn ta001_diagnostic(
    ty: &locus_air::AirType,
    module_path: &str,
    pattern: &str,
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
        rule_id: "TA001".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: ty.span.clone(),
        concept: None,
        message: format!(
            "test module `{module_path}` defines public type `{}` \
             (matched test pattern `{pattern}`)",
            ty.name
        ),
        why: vec![
            format!("module `{module_path}` matches test pattern `{pattern}`"),
            format!(
                "public type `{}` (`{}`, visibility `{:?}`)",
                ty.name, ty.symbol, ty.visibility
            ),
            "test modules must not create new domain truth; a public type \
             in test code is typically a shadow of a domain concept that \
             should live on the canonical production path"
                .into(),
        ],
        suggested_fix: Some(format!(
            "demote `{}` to non-`pub` if it's only used inside this test \
             module; or move it out of the test module if it's actually \
             shared production code; or accept this test module as a \
             legitimate public-fixture surface (future TA mechanism)",
            ty.name
        )),
    }
}

/// TA001 — test module defines a public domain-shaped type.
///
/// For every `AirFile` whose `module_path` matches any pattern in
/// `test_paths`, fire one diagnostic per public `AirItem::Type`.
///
/// Severity: Warning by default; Fatal under `--agent-strict`. Test modules
/// can legitimately hold private fixture types, so the structural fail-fast
/// tier isn't a fit — a public type is the heuristic signal that domain
/// concepts are being shadowed in test code.
pub fn ta001(air: &AirWorkspace, section: &TaSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.test_paths.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(pattern) = section
                .test_paths
                .iter()
                .find(|pat| matches_pattern(pat, module_path))
            else {
                continue;
            };
            for item in &file.items {
                let AirItem::Type(ty) = item else {
                    continue;
                };
                if ty.visibility != Visibility::Public {
                    continue;
                }
                out.push(ta001_diagnostic(ty, module_path, pattern, mode));
            }
        }
    }
    out
}

fn ta002_diagnostic(
    ty: &locus_air::AirType,
    module_path: &str,
    test_pattern: &str,
    name_pattern: &str,
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
        rule_id: "TA002".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: ty.span.clone(),
        concept: None,
        message: format!(
            "test module `{module_path}` defines type `{}` whose name \
             matches accepted canonical pattern `{name_pattern}`",
            ty.name
        ),
        why: vec![
            format!("module `{module_path}` matches test pattern `{test_pattern}`"),
            format!(
                "type name `{}` matches `paradigms.TA.canonical_name_patterns` \
                 entry `{name_pattern}`",
                ty.name
            ),
            "test types that re-use canonical names shadow the production \
             concept; even private duplicates drift over time and obscure \
             where the real definition lives"
                .into(),
        ],
        suggested_fix: Some(format!(
            "rename `{}` to a test-scoped identifier (e.g. `Test{0}` or \
             `{0}Fixture`), import the canonical type instead of redefining \
             it, or — if this name is genuinely unrelated to the domain \
             concept — narrow `paradigms.TA.canonical_name_patterns` so it \
             no longer matches",
            ty.name
        )),
    }
}

/// TA002 — test type whose name overlaps an accepted canonical concept.
///
/// For every `AirItem::Type` whose enclosing file's `module_path` matches a
/// pattern in `test_paths`, fire when the type's name matches any pattern
/// in `canonical_name_patterns`. Name match uses the same wildcard syntax
/// as the path matcher — `User`, `*User`, `User*`, `*User*` are all valid
/// — but the typical authoring shape is the bare canonical name (`User`,
/// `Email`, `Order`).
///
/// Visibility is intentionally not gated: a *private* test struct named
/// `User` is still a domain shadow worth flagging, even though TA001
/// would skip it. The two rules complement: TA001 is the public-surface
/// signal, TA002 is the named-shadow signal.
///
/// Severity: Warning by default; Fatal under `--agent-strict`.
pub fn ta002(air: &AirWorkspace, section: &TaSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.test_paths.is_empty() || section.canonical_name_patterns.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(test_pattern) = section
                .test_paths
                .iter()
                .find(|pat| matches_pattern(pat, module_path))
            else {
                continue;
            };
            for item in &file.items {
                let AirItem::Type(ty) = item else {
                    continue;
                };
                let Some(name_pattern) = section
                    .canonical_name_patterns
                    .iter()
                    .find(|pat| name_matches(pat, &ty.name))
                else {
                    continue;
                };
                out.push(ta002_diagnostic(
                    ty,
                    module_path,
                    test_pattern,
                    name_pattern,
                    mode,
                ));
            }
        }
    }
    out
}

fn ta003_diagnostic(
    ty: &locus_air::AirType,
    module_path: &str,
    test_pattern: &str,
    name_pattern: &str,
    overlap: f32,
    canonical_set: &[String],
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
        rule_id: "TA003".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: ty.span.clone(),
        concept: None,
        message: format!(
            "test struct `{}` in `{module_path}` shadows a canonical \
             concept (name overlap with pattern `{name_pattern}`, field \
             Jaccard {overlap:.2} against canonical field set)",
            ty.name,
        ),
        why: vec![
            format!("module `{module_path}` matches test pattern `{test_pattern}`"),
            format!(
                "struct name `{}` contains canonical pattern `{name_pattern}`",
                ty.name
            ),
            format!(
                "field-set Jaccard overlap {overlap:.2} >= 0.5 against canonical \
                 field set `{canonical_set:?}`",
            ),
            "test structs that mirror canonical names *and* canonical \
             field shapes are the spec's shape-shadow anti-pattern: \
             agents recreate domain truth in test code rather than \
             using the real type"
                .into(),
        ],
        suggested_fix: Some(format!(
            "import the canonical struct and construct it directly in this \
             test, or, if this fixture is genuinely a different concept \
             that just happens to share a few field names, rename it to \
             break the name overlap (e.g. `{}_TestStub`)",
            ty.name
        )),
    }
}

/// TA003 — test struct whose name and field shape both echo a canonical concept.
///
/// For every `AirItem::Type` with `kind == Struct` inside `test_paths`,
/// fire when:
/// - The type's name *contains* the stripped form of any pattern in
///   `canonical_name_patterns` (looser than TA002's exact-name match — a
///   test struct called `TestUser` or `UserFixture` is a candidate).
/// - The type's field-name set has Jaccard overlap >= 0.5 with any entry
///   in `canonical_field_sets`.
///
/// Both gates must trip; either alone is too noisy. TA003 is the
/// shape-shadow signal — even renamed (TA002 wouldn't fire on `TestUser`),
/// a struct that mirrors the canonical's field set is still duplicating
/// domain truth.
///
/// Severity: Warning by default; Fatal under `--agent-strict`.
/// Check one struct type for the TA003 shape-shadow signal.
/// Returns `Some(Diagnostic)` when both name and field-shape gates trip.
fn ta003_check_type(
    ty: &locus_air::AirType,
    module_path: &str,
    test_pattern: &str,
    section: &TaSection,
    mode: CheckMode,
) -> Option<Diagnostic> {
    if ty.kind != TypeKind::Struct {
        return None;
    }
    let name_pattern = section
        .canonical_name_patterns
        .iter()
        .find(|pat| name_contains(pat, &ty.name))?;
    let test_field_names: BTreeSet<&str> = ty.fields.iter().map(|f| f.name.as_str()).collect();
    if test_field_names.is_empty() {
        return None;
    }
    let (canonical_set, overlap) =
        best_jaccard_match(&test_field_names, &section.canonical_field_sets)?;
    if overlap < 0.5 {
        return None;
    }
    Some(ta003_diagnostic(
        ty,
        module_path,
        test_pattern,
        name_pattern,
        overlap,
        canonical_set,
        mode,
    ))
}

pub fn ta003(air: &AirWorkspace, section: &TaSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.test_paths.is_empty()
        || section.canonical_name_patterns.is_empty()
        || section.canonical_field_sets.is_empty()
    {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(test_pattern) = section
                .test_paths
                .iter()
                .find(|pat| matches_pattern(pat, module_path))
            else {
                continue;
            };
            for item in &file.items {
                let AirItem::Type(ty) = item else { continue };
                if let Some(d) = ta003_check_type(ty, module_path, test_pattern, section, mode) {
                    out.push(d);
                }
            }
        }
    }
    out
}

fn ta004_diagnostic(
    imp: &locus_air::AirImplBlock,
    module_path: &str,
    test_pattern: &str,
    trait_path: &str,
    port_pattern: &str,
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
        rule_id: "TA004".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: imp.span.clone(),
        concept: None,
        message: format!(
            "port impl `impl {trait_path} for {}` in test module `{module_path}` \
             lives outside any `paradigms.TA.accepted_test_adapter_paths`",
            imp.target_type,
        ),
        why: vec![
            format!("module `{module_path}` matches test pattern `{test_pattern}`"),
            format!("trait path `{trait_path}` matches port pattern `{port_pattern}`"),
            format!(
                "module `{module_path}` matches no \
                 `paradigms.TA.accepted_test_adapter_paths` pattern"
            ),
            "test adapters belong on a declared adapter path (e.g. \
             `tests::support::*`); inline port impls inside test files \
             drift from the production adapter contract"
                .into(),
        ],
        suggested_fix: Some(format!(
            "move `impl {trait_path} for {}` to a dedicated test-adapter \
             module (and add that module to \
             `paradigms.TA.accepted_test_adapter_paths` in `.locus/lock.json`), \
             or — if this trait isn't really a port — narrow \
             `paradigms.TA.port_trait_patterns` so it no longer matches",
            imp.target_type,
        )),
    }
}

/// TA004 — port impl in a test file that isn't an accepted test-adapter home.
///
/// For every `AirItem::Impl` with `Some(trait_path)`, fire when:
/// - The impl's enclosing file's `module_path` matches a `test_paths`
///   pattern.
/// - The trait path matches any pattern in `port_trait_patterns`.
/// - The file's `module_path` does NOT match any pattern in
///   `accepted_test_adapter_paths`.
///
/// Catches the "agent stitched in an in-memory `UserRepository` adapter
/// inside the test module" smell. Test adapters are legitimate, but they
/// belong on a declared adapter path (`tests::support::*`,
/// `*::test_adapters::*`), not in the same file as the production tests
/// they support — that's the path that drifts.
///
/// Severity: Warning by default; Fatal under `--agent-strict`.
/// Check a single impl block to see if it matches TA004 criteria.
fn ta004_check_impl<'a>(
    imp: &'a locus_air::AirImplBlock,
    module_path: &str,
    test_pattern: &str,
    section: &'a TaSection,
    mode: CheckMode,
) -> Option<Diagnostic> {
    let trait_path = imp.interface.as_deref()?;
    let trait_short = trait_path.rsplit("::").next().unwrap_or(trait_path);
    let port_pattern = section.port_trait_patterns.iter().find(|pat| {
        matches_pattern(pat, trait_path)
            || matches_pattern(pat, trait_short)
            || name_matches(pat, trait_path)
            || name_matches(pat, trait_short)
    })?;
    Some(ta004_diagnostic(
        imp,
        module_path,
        test_pattern,
        trait_path,
        port_pattern,
        mode,
    ))
}

pub fn ta004(air: &AirWorkspace, section: &TaSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.test_paths.is_empty() || section.port_trait_patterns.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(test_pattern) = section
                .test_paths
                .iter()
                .find(|pat| matches_pattern(pat, module_path))
            else {
                continue;
            };
            if section
                .accepted_test_adapter_paths
                .iter()
                .any(|pat| matches_pattern(pat, module_path))
            {
                continue;
            }
            for item in &file.items {
                let AirItem::Impl(imp) = item else { continue };
                if let Some(d) = ta004_check_impl(imp, module_path, test_pattern, section, mode) {
                    out.push(d);
                }
            }
        }
    }
    out
}

/// Wildcard-aware name match. Reuses [`matches_pattern`] when the pattern
/// contains `::` separators (so users can write `pkg::module::User`); for
/// bare names, supports leading/trailing `*` glob.
fn name_matches(pattern: &str, name: &str) -> bool {
    if pattern.contains("::") {
        return matches_pattern(pattern, name);
    }
    let leading = pattern.starts_with('*');
    let trailing = pattern.ends_with('*') && pattern.len() > 1;
    let stripped = match (leading, trailing) {
        (true, true) => &pattern[1..pattern.len() - 1],
        (true, false) => &pattern[1..],
        (false, true) => &pattern[..pattern.len() - 1],
        (false, false) => pattern,
    };
    if stripped.is_empty() {
        return pattern == "*";
    }
    match (leading, trailing) {
        (true, true) => name.contains(stripped),
        (true, false) => name.ends_with(stripped),
        (false, true) => name.starts_with(stripped),
        (false, false) => pattern == name,
    }
}

/// Looser variant of [`name_matches`] used by TA003: strips any leading/
/// trailing `*` and tests for a substring containment. `User`, `*User`,
/// `User*`, `*User*` all collapse to "name contains `User`".
fn name_contains(pattern: &str, name: &str) -> bool {
    let trimmed = pattern.trim_matches('*');
    if trimmed.is_empty() {
        return false;
    }
    name.contains(trimmed)
}

/// Compute Jaccard overlap between `test_fields` and each entry in
/// `canonical_sets`; return the best-matching canonical set together with
/// its overlap. `None` when `canonical_sets` is empty or every entry is
/// empty.
fn best_jaccard_match<'a>(
    test_fields: &BTreeSet<&str>,
    canonical_sets: &'a [Vec<String>],
) -> Option<(&'a [String], f32)> {
    let mut best: Option<(&'a [String], f32)> = None;
    for canonical in canonical_sets {
        if canonical.is_empty() {
            continue;
        }
        let canonical_set: BTreeSet<&str> = canonical.iter().map(String::as_str).collect();
        let intersection = test_fields.intersection(&canonical_set).count() as f32;
        let union = test_fields.union(&canonical_set).count() as f32;
        if union == 0.0 {
            continue;
        }
        let jaccard = intersection / union;
        match best {
            Some((_, b)) if jaccard <= b => {}
            _ => best = Some((canonical.as_slice(), jaccard)),
        }
    }
    best
}

// ── RuleDefinition impls (governance spine migration, epic #71) ──────────────

use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

const TA_PARADIGM: ParadigmId = ParadigmId::new("TA");
const TA001_ID: RuleId = RuleId::new("TA001");
const TA002_ID: RuleId = RuleId::new("TA002");
const TA003_ID: RuleId = RuleId::new("TA003");
const TA004_ID: RuleId = RuleId::new("TA004");

pub struct Ta001Rule;
pub static TA001_RULE: Ta001Rule = Ta001Rule;

impl RuleDefinition for Ta001Rule {
    fn id(&self) -> RuleId {
        TA001_ID
    }
    fn paradigm(&self) -> ParadigmId {
        TA_PARADIGM
    }
    fn title(&self) -> &'static str {
        "test module defines a public domain-shaped type"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        let section: TaSection = ctx.lockfile.paradigm_section("TA").unwrap_or_default();
        ta001(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(TA001_ID),
                rule_id: Some(TA001_ID),
                paradigm_id: Some(TA_PARADIGM),
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

pub struct Ta002Rule;
pub static TA002_RULE: Ta002Rule = Ta002Rule;

impl RuleDefinition for Ta002Rule {
    fn id(&self) -> RuleId {
        TA002_ID
    }
    fn paradigm(&self) -> ParadigmId {
        TA_PARADIGM
    }
    fn title(&self) -> &'static str {
        "test type whose name overlaps an accepted canonical concept"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        let section: TaSection = ctx.lockfile.paradigm_section("TA").unwrap_or_default();
        ta002(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(TA002_ID),
                rule_id: Some(TA002_ID),
                paradigm_id: Some(TA_PARADIGM),
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

pub struct Ta003Rule;
pub static TA003_RULE: Ta003Rule = Ta003Rule;

impl RuleDefinition for Ta003Rule {
    fn id(&self) -> RuleId {
        TA003_ID
    }
    fn paradigm(&self) -> ParadigmId {
        TA_PARADIGM
    }
    fn title(&self) -> &'static str {
        "test struct whose name and field shape both echo a canonical concept"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        let section: TaSection = ctx.lockfile.paradigm_section("TA").unwrap_or_default();
        ta003(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(TA003_ID),
                rule_id: Some(TA003_ID),
                paradigm_id: Some(TA_PARADIGM),
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

pub struct Ta004Rule;
pub static TA004_RULE: Ta004Rule = Ta004Rule;

impl RuleDefinition for Ta004Rule {
    fn id(&self) -> RuleId {
        TA004_ID
    }
    fn paradigm(&self) -> ParadigmId {
        TA_PARADIGM
    }
    fn title(&self) -> &'static str {
        "port impl in a test file outside accepted test-adapter modules"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        let section: TaSection = ctx.lockfile.paradigm_section("TA").unwrap_or_default();
        ta004(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(TA004_ID),
                rule_id: Some(TA004_ID),
                paradigm_id: Some(TA_PARADIGM),
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
