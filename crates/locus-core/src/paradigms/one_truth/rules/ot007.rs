//! OT007 — adapter-to-adapter conversion.
//!
//! Fires on every `AirConversion` whose endpoints are both lockfile-accepted
//! boundaries (in any concept). Adapter-to-adapter conversions bypass the
//! canonical and create a hidden translation path; the preferred shape is
//! `adapter → canonical → adapter`.
//!
//! Suppressed when a `// locus: ot protocol-translation reason="…"` hint binds to
//! the conversion's span — the explicit "yes I really mean this" escape hatch
//! from the spec.
//!
//! Always Fatal otherwise.

use std::collections::BTreeMap;

use locus_air::{AirItem, AirSpan, AirWorkspace, HintKind};

use super::super::lockfile_schema::OtSection;
use super::helpers::short_name;
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

pub fn ot007(air: &AirWorkspace, section: &OtSection, mode: CheckMode) -> Vec<Diagnostic> {
    let mut boundary_to_concept: BTreeMap<String, String> = BTreeMap::new();
    for (concept_id, entry) in &section.concepts {
        for b in &entry.boundaries {
            boundary_to_concept.insert(short_name(&b.symbol).to_string(), concept_id.clone());
        }
    }
    if boundary_to_concept.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Conversion(c) = item else {
                    continue;
                };
                let from_short = short_name(&c.from);
                let to_short = short_name(&c.to);
                let Some(from_concept) = boundary_to_concept.get(from_short) else {
                    continue;
                };
                let Some(to_concept) = boundary_to_concept.get(to_short) else {
                    continue;
                };

                if conversion_has_protocol_translation_hint(&file.hints, &c.span) {
                    continue;
                }

                let cross_label = if from_concept == to_concept {
                    "within the same concept".to_string()
                } else {
                    format!("across concepts (`{from_concept}` → `{to_concept}`)")
                };
                out.push(Diagnostic {
                    rule_id: "OT007".to_string(),
                    severity: mode.elevate(Severity::Fatal),
                    span: c.span.clone(),
                    concept: Some(from_concept.clone()),
                    message: format!(
                        "adapter-to-adapter conversion `{}` ({} → {}) — both endpoints \
                         are accepted boundaries",
                        c.symbol, c.from, c.to
                    ),
                    why: vec![
                        format!("`{from_short}` is a boundary for `{from_concept}`"),
                        format!("`{to_short}` is a boundary for `{to_concept}`"),
                        format!("conversion routes {cross_label}"),
                        "preferred path: adapter → canonical → adapter".into(),
                    ],
                    suggested_fix: Some(
                        "go through the canonical (e.g. `Canonical::try_from(from)?` then \
                         `Other::from(canonical)`), or annotate the conversion with \
                         `// locus: ot protocol-translation reason=\"...\"` if it's an \
                         intentional shortcut"
                            .into(),
                    ),
                });
            }
        }
    }
    out
}

/// True if any `// locus: ot protocol-translation` hint in the file has a
/// `target_span` that lands within the conversion's span.
fn conversion_has_protocol_translation_hint(hints: &[locus_air::AirHint], span: &AirSpan) -> bool {
    hints.iter().any(|h| {
        matches!(h.kind, HintKind::ProtocolTranslation { .. })
            && h.target_span
                .as_ref()
                .is_some_and(|t| t.line_start >= span.line_start && t.line_start <= span.line_end)
    })
}
