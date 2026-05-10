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
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

pub fn ot010(air: &AirWorkspace, section: &OtSection, mode: CheckMode) -> Vec<Diagnostic> {
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
                    out.push(Diagnostic {
                        rule_id: "OT010".to_string(),
                        severity,
                        span: ty.span.clone(),
                        concept: Some(concept_id.clone()),
                        message: format!(
                            "enum `{}` overlaps {:.0}% with accepted canonical `{canonical_symbol}` \
                             but is not accepted as canonical or boundary",
                            ty.symbol,
                            overlap * 100.0
                        ),
                        why: vec![
                            format!("variants: {{{}}}", join_sorted(&candidate_variants)),
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
                    });
                    break;
                }
            }
        }
    }
    out
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
