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

use locus_air::{AirSpan, AirWorkspace, HintKind};

use super::super::lockfile_schema::OtSection;
use super::helpers::{prefer_higher_provenance, short_name};
use crate::diagnostics::{CheckMode, Severity};
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{FindingIdMinter, ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

pub struct Ot007Rule;

pub static OT007_RULE: Ot007Rule = Ot007Rule;

const OT007_ID: RuleId = RuleId::new("OT007");
const OT_PARADIGM: ParadigmId = ParadigmId::new("OT");

impl RuleDefinition for Ot007Rule {
    fn id(&self) -> RuleId {
        OT007_ID
    }
    fn paradigm(&self) -> ParadigmId {
        OT_PARADIGM
    }
    fn title(&self) -> &'static str {
        "adapter-to-adapter conversion"
    }
    fn default_severity(&self) -> Severity {
        Severity::Fatal
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
            // Dedup by provenance — semantic-resolved wins (see #111).
            for c in prefer_higher_provenance(&file.items) {
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

                out.push(ot007_finding(
                    c,
                    from_short,
                    to_short,
                    from_concept,
                    to_concept,
                    mode,
                    finding_ids,
                ));
            }
        }
    }
    out
}

fn ot007_finding(
    c: &locus_air::AirConversion,
    from_short: &str,
    to_short: &str,
    from_concept: &str,
    to_concept: &str,
    mode: CheckMode,
    finding_ids: &FindingIdMinter,
) -> RuleFinding {
    let cross_label = if from_concept == to_concept {
        "within the same concept".to_string()
    } else {
        format!("across concepts (`{from_concept}` → `{to_concept}`)")
    };
    RuleFinding {
        id: finding_ids.next(),
        source: FindingSource::RegisteredRule(OT007_ID),
        rule_id: Some(OT007_ID),
        paradigm_id: Some(OT_PARADIGM),
        default_severity: mode.elevate(Severity::Fatal),
        span: Some(c.span.clone()),
        concept: Some(from_concept.to_string()),
        message: format!(
            "adapter-to-adapter conversion `{}` ({} → {}) — both endpoints \
             are accepted boundaries",
            c.symbol, c.from, c.to
        ),
        evidence: vec![],
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
        diagnostic_code: None,
    }
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
