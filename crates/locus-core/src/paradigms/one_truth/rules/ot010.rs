//! OT010 — shadow enum.
//!
//! Fires for each enum that:
//! 1. Is not lockfile-accepted (canonical or boundary), and
//! 2. Shares ≥ 50% of its variant names with an accepted canonical enum.
//!
//! 50% is the same Jaccard threshold OT002 uses for struct field overlap
//! (`FIELD_OVERLAP_THRESHOLD`). Confidence is 0.85 — variant-name overlap is
//! a fairly specific signal but not bullet-proof (`Active`/`Inactive` shows
//! up everywhere).

use std::collections::BTreeSet;

use locus_air::{AirItem, AirWorkspace, TypeKind};

use super::super::infer::FIELD_OVERLAP_THRESHOLD;
use super::super::lockfile_schema::OtSection;
use crate::diagnostics::{CheckMode, Severity};
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{FindingIdMinter, ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

pub struct Ot010Rule;

pub static OT010_RULE: Ot010Rule = Ot010Rule;

const OT010_ID: RuleId = RuleId::new("OT010");
const OT_PARADIGM: ParadigmId = ParadigmId::new("OT");

impl RuleDefinition for Ot010Rule {
    fn id(&self) -> RuleId {
        OT010_ID
    }
    fn paradigm(&self) -> ParadigmId {
        OT_PARADIGM
    }
    fn title(&self) -> &'static str {
        "shadow enum"
    }
    fn default_severity(&self) -> Severity {
        Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        let section: OtSection = ctx.lockfile.paradigm_section("OT").unwrap_or_default();
        produce_findings(ctx.air, &section, ctx.mode, ctx.finding_ids)
    }
}

pub(crate) fn produce_findings(
    air: &AirWorkspace,
    section: &OtSection,
    mode: CheckMode,
    finding_ids: &FindingIdMinter,
) -> Vec<RuleFinding> {
    // Collect every accepted canonical enum's variant set.
    let mut canonical_enums: Vec<(String, String, BTreeSet<String>)> = Vec::new(); // (concept, symbol, variants)
    for (concept_id, entry) in &section.concepts {
        let symbol = &entry.canonical.symbol;
        let Some((variants, kind)) = type_variants_and_kind(air, symbol) else {
            continue;
        };
        if kind != TypeKind::Enum {
            continue;
        }
        canonical_enums.push((concept_id.clone(), symbol.clone(), variants));
    }
    if canonical_enums.is_empty() {
        return Vec::new();
    }
    let confidence = 0.85;
    let Some(severity) = Severity::from_confidence(confidence, mode) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Type(ty) = item else {
                    continue;
                };
                if ty.kind != TypeKind::Enum {
                    continue;
                }
                if section.role_of(&ty.symbol).is_some() {
                    continue; // already accepted
                }
                let candidate_variants: BTreeSet<String> =
                    ty.variants.iter().map(|v| v.name.clone()).collect();
                if candidate_variants.is_empty() {
                    continue;
                }
                for (concept_id, canonical_symbol, canonical_variants) in &canonical_enums {
                    if &ty.symbol == canonical_symbol {
                        continue;
                    }
                    let overlap = jaccard_str(&candidate_variants, canonical_variants);
                    if overlap < FIELD_OVERLAP_THRESHOLD {
                        continue;
                    }
                    out.push(ot010_finding(
                        ty,
                        &candidate_variants,
                        canonical_symbol,
                        canonical_variants,
                        concept_id,
                        overlap,
                        confidence,
                        severity,
                        finding_ids,
                    ));
                    break;
                }
            }
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn ot010_finding(
    ty: &locus_air::AirType,
    candidate_variants: &BTreeSet<String>,
    canonical_symbol: &str,
    canonical_variants: &BTreeSet<String>,
    concept_id: &str,
    overlap: f32,
    confidence: f32,
    severity: Severity,
    finding_ids: &FindingIdMinter,
) -> RuleFinding {
    RuleFinding {
        id: finding_ids.next(),
        source: FindingSource::RegisteredRule(OT010_ID),
        rule_id: Some(OT010_ID),
        paradigm_id: Some(OT_PARADIGM),
        default_severity: severity,
        span: Some(ty.span.clone()),
        concept: Some(concept_id.to_string()),
        message: format!(
            "enum `{}` overlaps {:.0}% with accepted canonical `{canonical_symbol}` \
             but is not accepted as canonical or boundary",
            ty.symbol,
            overlap * 100.0
        ),
        evidence: vec![],
        why: vec![
            format!("variants: {{{}}}", join_sorted(candidate_variants)),
            format!(
                "canonical `{canonical_symbol}` variants: {{{}}}",
                join_sorted(canonical_variants)
            ),
            format!("Jaccard overlap: {:.2}", overlap),
            format!("inference confidence: {confidence:.2}"),
        ],
        suggested_fix: Some(format!(
            "remove `{}` and use `{canonical_symbol}` directly, or accept \
             this enum as a boundary for `{concept_id}` via \
             `// locus: ot boundary {concept_id} <name>` then rerun `locus init`",
            ty.name
        )),
        diagnostic_code: None,
    }
}

/// `(variants, kind)` for the type whose `symbol` matches `target`.
fn type_variants_and_kind(
    air: &AirWorkspace,
    target: &str,
) -> Option<(BTreeSet<String>, TypeKind)> {
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                if let AirItem::Type(ty) = item
                    && ty.symbol == target
                {
                    return Some((
                        ty.variants.iter().map(|v| v.name.clone()).collect(),
                        ty.kind,
                    ));
                }
            }
        }
    }
    None
}

fn jaccard_str(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 {
        0.0
    } else {
        inter as f32 / union as f32
    }
}

fn join_sorted(set: &BTreeSet<String>) -> String {
    set.iter().cloned().collect::<Vec<_>>().join(", ")
}
