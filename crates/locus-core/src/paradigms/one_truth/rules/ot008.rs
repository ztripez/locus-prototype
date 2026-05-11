//! OT008 — domain logic on a boundary adapter.
//!
//! Fires when an inherent `impl AcceptedBoundary { ... }` declares a method
//! whose name is *not* in the boundary-shape allowlist (`from`, `try_from`,
//! `into`, `serialize`, `fmt`, …). Domain queries / behaviours
//! (`is_active`, `validate`, `apply_to`, `total_price`, …) belong on the
//! canonical, not the wire/storage shape.
//!
//! Confidence 0.85 — name-only heuristic; the method body could be a pure
//! projection and we can't tell from AIR. Per the spec's severity table
//! (`docs/PARADIGMS.md` §"Severity tiers"), this is warning by default and
//! fatal under `--agent-strict`. [`Severity::from_confidence`] does the
//! mapping.

use std::collections::BTreeMap;

use locus_air::{AirItem, AirWorkspace};

use super::super::lockfile_schema::OtSection;
use super::helpers::short_name;
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

pub fn ot008(air: &AirWorkspace, section: &OtSection, mode: CheckMode) -> Vec<Diagnostic> {
    let mut boundary_short_to_concept: BTreeMap<String, String> = BTreeMap::new();
    for (concept_id, entry) in &section.concepts {
        for b in &entry.boundaries {
            boundary_short_to_concept.insert(short_name(&b.symbol).to_string(), concept_id.clone());
        }
    }
    if boundary_short_to_concept.is_empty() {
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
                let AirItem::Impl(im) = item else {
                    continue;
                };
                if im.interface.is_some() {
                    // Trait impls (`impl From<X> for Y`, `impl Display for Y`,
                    // serde derives, etc.) are projection by construction —
                    // they're how boundary types translate, not domain logic.
                    continue;
                }
                let self_short = short_name(&im.target_type);
                let Some(concept_id) = boundary_short_to_concept.get(self_short) else {
                    continue;
                };
                for method in &im.method_names {
                    if is_boundary_shape_method(method) {
                        continue;
                    }
                    out.push(ot008_diagnostic(
                        im,
                        self_short,
                        method,
                        concept_id,
                        confidence,
                        severity,
                    ));
                }
            }
        }
    }
    out
}

fn ot008_diagnostic(
    im: &locus_air::AirImplBlock,
    self_short: &str,
    method: &str,
    concept_id: &str,
    confidence: f32,
    severity: Severity,
) -> Diagnostic {
    Diagnostic {
        rule_id: "OT008".to_string(),
        severity,
        span: im.span.clone(),
        concept: Some(concept_id.to_string()),
        message: format!(
            "boundary `{self_short}` carries domain-shaped method \
             `{method}` — boundary adapters should only translate, \
             not reason about, the concept"
        ),
        why: vec![
            format!("`{self_short}` is the accepted boundary for `{concept_id}`"),
            format!(
                "`{method}` is not in the boundary-shape allowlist \
                 (from/try_from/into/as_*/to_*/serialize/deserialize/fmt/new/default/builder)"
            ),
            format!("inference confidence: {confidence:.2}"),
        ],
        suggested_fix: Some(format!(
            "move `{method}` onto the canonical for `{concept_id}` \
             (where domain behaviour lives), or rename it into the \
             boundary-shape allowlist if it really is pure translation"
        )),
    }
}

/// True for method names that are part of the *translation* surface of a
/// boundary adapter (and so allowed by OT008). The list is conservative —
/// when in doubt prefer false-positive (a flag) over false-negative
/// (a missed domain leak), then expand the allowlist if the user pushes back.
fn is_boundary_shape_method(name: &str) -> bool {
    // Exact-match conversions, accessors, factories, and stdlib trait shims.
    const EXACT: &[&str] = &[
        "from",
        "try_from",
        "into",
        "try_into",
        "serialize",
        "deserialize",
        "fmt",
        "display",
        "new",
        "default",
        "builder",
        "build",
        "clone",
        "as_ref",
        "as_mut",
        "as_str",
        "as_bytes",
        "into_inner",
        "inner",
        "len",
        "is_empty",
    ];
    if EXACT.contains(&name) {
        return true;
    }
    // Conventional translation prefixes.
    name.starts_with("as_")
        || name.starts_with("to_")
        || name.starts_with("into_")
        || name.starts_with("from_")
        || name.starts_with("try_")
        || name.starts_with("with_")
}
