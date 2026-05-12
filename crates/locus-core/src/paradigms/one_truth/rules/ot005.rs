//! OT005 — missing converter for an accepted boundary.
//!
//! Fires when a concept has accepted boundaries but no accepted converter
//! mentions a given boundary (in either direction). The spec eventually wants
//! this to track inbound vs outbound directions; for Phase 2 we only require
//! at least one converter touching the boundary.
//!
//! Always Fatal: a boundary with no converter is a dead end — boundary data
//! either can't reach the canonical or can't leave it.

use locus_air::{AirSpan, AirWorkspace};

use super::super::lockfile_schema::OtSection;
use super::helpers::{short_name, span_of_symbol};
use crate::diagnostics::{CheckMode, Severity};
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{FindingIdMinter, ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

pub struct Ot005Rule;

pub static OT005_RULE: Ot005Rule = Ot005Rule;

const OT005_ID: RuleId = RuleId::new("OT005");
const OT_PARADIGM: ParadigmId = ParadigmId::new("OT");

impl RuleDefinition for Ot005Rule {
    fn id(&self) -> RuleId {
        OT005_ID
    }
    fn paradigm(&self) -> ParadigmId {
        OT_PARADIGM
    }
    fn title(&self) -> &'static str {
        "missing converter for accepted boundary"
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
    let mut out = Vec::new();
    for (concept_id, entry) in &section.concepts {
        for boundary in &entry.boundaries {
            let short = short_name(&boundary.symbol);
            let has_converter = entry
                .converters
                .iter()
                .any(|c| short_name(&c.from) == short || short_name(&c.to) == short);
            if has_converter {
                continue;
            }
            let span = span_of_symbol(air, &boundary.symbol)
                .unwrap_or_else(|| AirSpan::new(boundary.symbol.clone(), 1, 1));
            out.push(RuleFinding {
                id: finding_ids.next(),
                source: FindingSource::RegisteredRule(OT005_ID),
                rule_id: Some(OT005_ID),
                paradigm_id: Some(OT_PARADIGM),
                default_severity: mode.elevate(Severity::Fatal),
                span: Some(span),
                concept: Some(concept_id.clone()),
                message: format!(
                    "boundary `{}` (concept `{concept_id}`) has no accepted converter \
                     to/from the canonical",
                    boundary.symbol
                ),
                evidence: vec![],
                why: vec![
                    format!("canonical: `{}`", entry.canonical.symbol),
                    format!(
                        "no entry under `paradigms.OT.concepts.{concept_id}.converters` \
                         mentions `{short}` on either side"
                    ),
                ],
                suggested_fix: Some(format!(
                    "add an `impl TryFrom<{short}> for {}` (or its inverse) and rerun \
                     `locus init`; alternatively remove the boundary acceptance if it's \
                     no longer needed",
                    short_name(&entry.canonical.symbol),
                )),
                diagnostic_code: None,
            });
        }
    }
    out
}
