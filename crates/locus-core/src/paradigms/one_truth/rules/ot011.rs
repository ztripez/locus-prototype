//! OT011 — shadow newtype / value object.
//!
//! Fires for each single-field struct (a "newtype") whose **name** matches
//! an accepted canonical (by short name) but whose symbol isn't accepted.
//! Common shape: `pub struct UserId(pub String);` defined in two places.
//!
//! Confidence 0.80 — name-match is a strong signal; the field-type check
//! keeps us off generic `Wrapper<T>`-style structs.

use std::collections::BTreeMap;

use locus_air::{AirItem, AirWorkspace, TypeKind};

use super::super::lockfile_schema::OtSection;
use crate::diagnostics::{CheckMode, Severity};
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{FindingIdMinter, ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

pub struct Ot011Rule;

pub static OT011_RULE: Ot011Rule = Ot011Rule;

const OT011_ID: RuleId = RuleId::new("OT011");
const OT_PARADIGM: ParadigmId = ParadigmId::new("OT");

impl RuleDefinition for Ot011Rule {
    fn id(&self) -> RuleId {
        OT011_ID
    }
    fn paradigm(&self) -> ParadigmId {
        OT_PARADIGM
    }
    fn title(&self) -> &'static str {
        "shadow newtype / value object"
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
    let mut canonical_short: BTreeMap<String, (String, String)> = BTreeMap::new(); // short → (concept, full)
    for (concept_id, entry) in &section.concepts {
        let symbol = &entry.canonical.symbol;
        if let Some(short) = symbol.rsplit("::").next() {
            canonical_short.insert(short.to_string(), (concept_id.clone(), symbol.clone()));
        }
    }
    if canonical_short.is_empty() {
        return Vec::new();
    }
    let confidence = 0.80;
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
                if ty.kind != TypeKind::Struct || ty.fields.len() != 1 {
                    continue;
                }
                if section.role_of(&ty.symbol).is_some() {
                    continue;
                }
                let Some((concept_id, canonical_symbol)) = canonical_short.get(ty.name.as_str())
                else {
                    continue;
                };
                if &ty.symbol == canonical_symbol {
                    continue; // canonical itself, just not accepted under that concept yet
                }
                out.push(ot011_finding(
                    ty,
                    concept_id,
                    canonical_symbol,
                    confidence,
                    severity,
                    finding_ids,
                ));
            }
        }
    }
    out
}

fn ot011_finding(
    ty: &locus_air::AirType,
    concept_id: &str,
    canonical_symbol: &str,
    confidence: f32,
    severity: Severity,
    finding_ids: &FindingIdMinter,
) -> RuleFinding {
    RuleFinding {
        id: finding_ids.next(),
        source: FindingSource::RegisteredRule(OT011_ID),
        rule_id: Some(OT011_ID),
        paradigm_id: Some(OT_PARADIGM),
        default_severity: severity,
        span: Some(ty.span.clone()),
        concept: Some(concept_id.to_string()),
        message: format!(
            "newtype `{}` shadows accepted canonical `{canonical_symbol}` \
             (concept `{concept_id}`)",
            ty.symbol
        ),
        evidence: vec![],
        why: vec![
            format!("single-field struct named `{}`", ty.name),
            format!("canonical for `{concept_id}`: `{canonical_symbol}`"),
            format!("inference confidence: {confidence:.2}"),
        ],
        suggested_fix: Some(format!(
            "remove `{}` and import `{canonical_symbol}` instead; if this \
             really is a parallel boundary representation, accept it via \
             `// locus: ot boundary {concept_id} <name>` then rerun `locus init`",
            ty.symbol
        )),
        diagnostic_code: None,
    }
}
