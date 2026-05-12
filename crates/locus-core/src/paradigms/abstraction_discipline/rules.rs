//! AB rule implementations.
//!
//! Implemented:
//! - [`ab001`]: trait declared in the workspace with exactly one impl. The
//!   "manager / processor / DataHandler" pattern from the spec — a trait
//!   added "in case other implementations exist someday" but in fact only
//!   ever points at one concrete type. Speculative abstraction.
//! - [`ab002`]: type (trait or struct) named after a generic role
//!   (`*Manager`, `*Service`, `*Processor`, …) without an accepted
//!   single-impl trait or accepted-abstraction-name mapping.
//!
//! Counting rules (AB001):
//! - 0 impls: skip (likely a library API surface, an unimplemented port, or
//!   an associated-type-only trait). The signal is too weak to fire on.
//! - 1 impl: fire AB001 unless the trait is exempted by
//!   `accepted_single_impl_traits`.
//! - 2+ impls: justified, skip.
//!
//! Trait/impl matching:
//! - A trait is identified by its [`AirType`] symbol (`crate::foo::Trait`).
//! - An impl's `trait_path` is rendered with the same clean type-text
//!   formatting as a type symbol (per AIR docs), so we match an impl to a
//!   declared trait when the impl's `trait_path` either equals the trait's
//!   full symbol or ends with `::<trait_short_name>`. The suffix fallback
//!   covers the common case where the trait is referenced inside an impl by
//!   a re-exported / shorter path than its declaration symbol.

use std::collections::HashMap;

use locus_air::{AirItem, AirSpan, AirWorkspace, TypeKind};

use super::lockfile_schema::{AbSection, matches_name_pattern, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

struct TraitDecl {
    symbol: String,
    name: String,
    span: AirSpan,
}

#[derive(Default)]
struct ImplCount {
    count: u32,
    first_self_ty: Option<String>,
}

/// Collect every declared trait in workspace iteration order.
fn collect_trait_decls(air: &AirWorkspace) -> Vec<TraitDecl> {
    let mut traits = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                if let AirItem::Type(ty) = item
                    && ty.kind == TypeKind::Trait
                {
                    traits.push(TraitDecl {
                        symbol: ty.symbol.clone(),
                        name: ty.name.clone(),
                        span: ty.span.clone(),
                    });
                }
            }
        }
    }
    traits
}

/// Count impl blocks per trait, indexed by both full symbol and short name.
/// Returns `(by_symbol, by_short, ambiguous_shorts)`.
fn count_trait_impls<'a>(
    air: &AirWorkspace,
    traits: &'a [TraitDecl],
) -> (
    HashMap<&'a str, ImplCount>,
    HashMap<&'a str, ImplCount>,
    std::collections::HashSet<&'a str>,
) {
    let (mut by_symbol, mut by_short, ambiguous_shorts) = init_impl_tables(traits);
    populate_impl_counts(air, &mut by_symbol, &mut by_short, &ambiguous_shorts);
    (by_symbol, by_short, ambiguous_shorts)
}

/// Build the initial (empty) count tables and ambiguous-short-name set.
fn init_impl_tables(
    traits: &[TraitDecl],
) -> (
    HashMap<&str, ImplCount>,
    HashMap<&str, ImplCount>,
    std::collections::HashSet<&str>,
) {
    let mut by_symbol: HashMap<&str, ImplCount> = HashMap::new();
    let mut by_short: HashMap<&str, ImplCount> = HashMap::new();
    let mut short_name_seen: HashMap<&str, u32> = HashMap::new();
    for t in traits {
        by_symbol.insert(t.symbol.as_str(), ImplCount::default());
        by_short.entry(t.name.as_str()).or_default();
        *short_name_seen.entry(t.name.as_str()).or_insert(0) += 1;
    }
    let ambiguous_shorts: std::collections::HashSet<&str> = short_name_seen
        .into_iter()
        .filter_map(|(k, v)| (v > 1).then_some(k))
        .collect();
    (by_symbol, by_short, ambiguous_shorts)
}

/// Walk every impl block in the workspace and increment the per-trait counts.
fn populate_impl_counts<'a>(
    air: &AirWorkspace,
    by_symbol: &mut HashMap<&'a str, ImplCount>,
    by_short: &mut HashMap<&'a str, ImplCount>,
    ambiguous_shorts: &std::collections::HashSet<&'a str>,
) {
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Impl(im) = item else { continue };
                let Some(trait_path) = im.interface.as_deref() else {
                    continue;
                };
                if let Some(slot) = by_symbol.get_mut(trait_path) {
                    slot.count += 1;
                    if slot.first_self_ty.is_none() {
                        slot.first_self_ty = Some(im.target_type.clone());
                    }
                    continue;
                }
                let short = trait_path.rsplit("::").next().unwrap_or(trait_path);
                if ambiguous_shorts.contains(short) {
                    continue;
                }
                if let Some(slot) = by_short.get_mut(short) {
                    slot.count += 1;
                    if slot.first_self_ty.is_none() {
                        slot.first_self_ty = Some(im.target_type.clone());
                    }
                }
            }
        }
    }
}

fn ab001_diagnostic(t: &TraitDecl, lone: &str, mode: CheckMode) -> Diagnostic {
    Diagnostic {
        rule_id: "AB001".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: t.span.clone(),
        concept: None,
        message: format!(
            "trait `{}` has exactly one impl (`{}`) — likely speculative abstraction",
            t.symbol, lone
        ),
        why: vec![
            format!("trait symbol `{}`", t.symbol),
            "impl count: 1".into(),
            format!("only impl is for `{lone}`"),
            "single-impl traits with no boundary role are usually \
             speculative abstraction (the spec's manager / processor / \
             DataHandler pattern)"
                .into(),
        ],
        suggested_fix: Some(format!(
            "remove the trait and use `{lone}` directly, or — if this is a \
             genuine port awaiting a second impl (e.g. a test double in a \
             separate environment) — add `{}` to \
             `paradigms.AB.accepted_single_impl_traits` in `locus.lock`",
            t.symbol
        )),
    }
}

/// AB001 — trait declared in the workspace has exactly one impl.
///
/// Severity: Warning by default; Fatal under `--agent-strict` via
/// [`CheckMode::elevate`]. Spec lists this as a heuristic: a single-impl
/// trait might still be a real port awaiting its second environment, so
/// blocking by default would be too aggressive. The escape hatch is the
/// `accepted_single_impl_traits` list.
///
/// Unlike MO/UT, AB001 fires even on a fully default section: the spec is
/// explicit that speculative abstraction should be flagged eagerly so users
/// have to examine each occurrence and explicitly accept legitimate ports.
pub fn ab001(air: &AirWorkspace, section: &AbSection, mode: CheckMode) -> Vec<Diagnostic> {
    let traits = collect_trait_decls(air);
    if traits.is_empty() {
        return Vec::new();
    }
    let (by_symbol, by_short, ambiguous_shorts) = count_trait_impls(air, &traits);

    let mut out = Vec::new();
    for t in &traits {
        let by_sym = by_symbol.get(t.symbol.as_str());
        let by_sht = if ambiguous_shorts.contains(t.name.as_str()) {
            None
        } else {
            by_short.get(t.name.as_str())
        };
        let (count, lone_self_ty) = match (by_sym, by_sht) {
            (Some(s), _) if s.count > 0 => (s.count, s.first_self_ty.clone()),
            (_, Some(h)) => (h.count, h.first_self_ty.clone()),
            (Some(s), None) => (s.count, s.first_self_ty.clone()),
            (None, None) => (0, None),
        };
        if count != 1 {
            continue;
        }
        let exempted = section
            .accepted_single_impl_traits
            .iter()
            .any(|pat| matches_pattern(pat, &t.symbol) || matches_pattern(pat, &t.name));
        if exempted {
            continue;
        }
        let lone = lone_self_ty.unwrap_or_else(|| "<unknown>".to_string());
        out.push(ab001_diagnostic(t, &lone, mode));
    }
    out
}

fn ab002_diagnostic(
    ty: &locus_air::AirType,
    kind_word: &str,
    matched_pattern: &str,
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
        rule_id: "AB002".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: ty.span.clone(),
        concept: None,
        message: format!(
            "{kind_word} `{}` is named after a generic role (`{matched_pattern}`) \
             without an accepted abstraction record — likely \
             manager/processor/coordinator anti-pattern",
            ty.symbol
        ),
        why: vec![
            format!("{kind_word} symbol `{}`", ty.symbol),
            format!(
                "short name `{}` matches suspect pattern `{matched_pattern}`",
                ty.name
            ),
            "neither `accepted_single_impl_traits` nor `accepted_abstraction_names` \
             covers this symbol"
                .into(),
        ],
        suggested_fix: Some(format!(
            "rename the type after the *domain concept it owns* (e.g. `UserManager` \
             → `UserDirectory`), or — if this name is genuinely the right one — \
             add `{}` to `paradigms.AB.accepted_abstraction_names` in `locus.lock`",
            ty.symbol
        )),
    }
}

/// AB002 — manager/processor abstraction without accepted role.
///
/// For each `AirItem::Type` with `TypeKind::Trait` or `TypeKind::Struct`,
/// fire when the type's short name matches any pattern in
/// `section.suspect_abstraction_patterns` AND the type is NOT exempted by
/// either:
/// - `section.accepted_single_impl_traits` — the existing AB001 acceptance
///   list, reused so a port trait already accepted as legitimately
///   single-impl (`Clock`, etc.) doesn't re-fire under AB002, OR
/// - `section.accepted_abstraction_names` — the new AB002-specific
///   acceptance list, for cases where a `*Service` / `*Manager` name is
///   genuinely the right domain term (rare, but real).
///
/// Match semantics: each acceptance pattern is checked against both the
/// type's full symbol (`crate::core::UserManager`) and its short name
/// (`UserManager`), mirroring AB001's exemption shape.
///
/// Severity: Warning by default; Fatal under `--agent-strict`.
///
/// Lockfile-driven defaults: the section ships with a non-empty
/// `suspect_abstraction_patterns` (the spec's seeded role-name list), so
/// AB002 fires immediately on un-onboarded code. Users curate the
/// patterns or accept individual names rather than starting from silence
/// — abstraction discipline is the spec's "examine and decide
/// deliberately" paradigm, not a paradigm where un-configured means
/// silent.
/// Return `true` when `ty` is exempt from AB002 via any acceptance list.
fn ab002_is_exempt(ty: &locus_air::AirType, section: &AbSection) -> bool {
    let sym = ty.symbol.as_str();
    let name = ty.name.as_str();
    let pat_matches = |pat: &String| matches_pattern(pat, sym) || matches_pattern(pat, name);
    section.accepted_single_impl_traits.iter().any(pat_matches)
        || section.accepted_abstraction_names.iter().any(pat_matches)
}

pub fn ab002(air: &AirWorkspace, section: &AbSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.suspect_abstraction_patterns.is_empty() {
        // User explicitly emptied the list → silent. Same convention as
        // disabling a paradigm via empty configuration.
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Type(ty) = item else { continue };
                if !matches!(ty.kind, TypeKind::Trait | TypeKind::Struct) {
                    continue;
                }
                let Some(matched_pattern) = section
                    .suspect_abstraction_patterns
                    .iter()
                    .find(|pat| matches_name_pattern(pat, &ty.name))
                else {
                    continue;
                };
                if ab002_is_exempt(ty, section) {
                    continue;
                }
                let kind_word = match ty.kind {
                    TypeKind::Trait => "trait",
                    TypeKind::Struct => "struct",
                    _ => unreachable!(),
                };
                out.push(ab002_diagnostic(ty, kind_word, matched_pattern, mode));
            }
        }
    }
    out
}

// ── RuleDefinition impls (governance spine migration, epic #71) ──────────────

use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

const AB_PARADIGM: ParadigmId = ParadigmId::new("AB");
const AB001_ID: RuleId = RuleId::new("AB001");
const AB002_ID: RuleId = RuleId::new("AB002");

pub struct Ab001Rule;
pub static AB001_RULE: Ab001Rule = Ab001Rule;

impl RuleDefinition for Ab001Rule {
    fn id(&self) -> RuleId {
        AB001_ID
    }
    fn paradigm(&self) -> ParadigmId {
        AB_PARADIGM
    }
    fn title(&self) -> &'static str {
        "single-impl trait (speculative abstraction)"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::lockfile_schema::AbSection;
        let section: AbSection = ctx.lockfile.paradigm_section("AB").unwrap_or_default();
        ab001(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(AB001_ID),
                rule_id: Some(AB001_ID),
                paradigm_id: Some(AB_PARADIGM),
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

pub struct Ab002Rule;
pub static AB002_RULE: Ab002Rule = Ab002Rule;

impl RuleDefinition for Ab002Rule {
    fn id(&self) -> RuleId {
        AB002_ID
    }
    fn paradigm(&self) -> ParadigmId {
        AB_PARADIGM
    }
    fn title(&self) -> &'static str {
        "type named after generic role"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::lockfile_schema::AbSection;
        let section: AbSection = ctx.lockfile.paradigm_section("AB").unwrap_or_default();
        ab002(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(AB002_ID),
                rule_id: Some(AB002_ID),
                paradigm_id: Some(AB_PARADIGM),
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
