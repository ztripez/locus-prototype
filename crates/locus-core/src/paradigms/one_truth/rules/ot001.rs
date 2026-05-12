//! OT001 — duplicate canonical concept.
//!
//! Fires when two or more cluster members are tagged Canonical for the same
//! concept. Two ways this happens:
//! - multiple `// locus: ot canonical` annotations across types in the same stem
//!   bucket;
//! - a hint and a lockfile acceptance disagreeing — the lockfile wins for the
//!   role lookup, but the *other* annotated type still presents as Canonical
//!   via its hint, producing a duplicate within the cluster.
//!
//! Always Fatal: a concept can only have one canonical representation. There
//! is no "warning" path here — it's a structural contradiction.

use super::super::infer::{ConceptCluster, InferredRole};
use crate::diagnostics::Severity;
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{FindingIdMinter, ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

pub struct Ot001Rule;

pub static OT001_RULE: Ot001Rule = Ot001Rule;

const OT001_ID: RuleId = RuleId::new("OT001");
const OT_PARADIGM: ParadigmId = ParadigmId::new("OT");

impl RuleDefinition for Ot001Rule {
    fn id(&self) -> RuleId {
        OT001_ID
    }
    fn paradigm(&self) -> ParadigmId {
        OT_PARADIGM
    }
    fn title(&self) -> &'static str {
        "duplicate canonical concept"
    }
    fn default_severity(&self) -> Severity {
        Severity::Fatal
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::super::lockfile_schema::OtSection;
        let section: OtSection = ctx.lockfile.paradigm_section("OT").unwrap_or_default();
        let clusters = super::super::infer::cluster_concepts_with_lockfile(ctx.air, &section);
        produce_findings_from_clusters(&clusters, ctx.finding_ids)
    }
}

// locus: allow CX001 — rule finding helper; inherently spans >50 lines due to full RuleFinding construction per cluster member
/// Cluster-level helper exposed for tests. Walks pre-computed clusters
/// and emits findings — same logic as `Ot001Rule::observe` minus the
/// AIR-to-clusters step.
pub(crate) fn produce_findings_from_clusters(
    clusters: &[ConceptCluster],
    finding_ids: &FindingIdMinter,
) -> Vec<RuleFinding> {
    let mut out = Vec::new();
    for cluster in clusters {
        let canonicals: Vec<_> = cluster
            .members
            .iter()
            .filter(|m| m.role == InferredRole::Canonical)
            .collect();
        if canonicals.len() < 2 {
            continue;
        }

        // Finding per *extra* canonical — pin the first as the "incumbent"
        // and report each additional one. This makes the fixes obvious: drop
        // the redundant `// locus: ot canonical` annotation or rename the type.
        let primary = canonicals[0];
        for extra in &canonicals[1..] {
            out.push(RuleFinding {
                id: finding_ids.next(),
                source: FindingSource::RegisteredRule(OT001_ID),
                rule_id: Some(OT001_ID),
                paradigm_id: Some(OT_PARADIGM),
                default_severity: Severity::Fatal,
                span: Some(extra.span.clone()),
                concept: Some(cluster.concept_id.clone()),
                message: format!(
                    "`{}` is a second canonical for concept `{}`; \
                     `{}` is already canonical",
                    extra.symbol, cluster.concept_id, primary.symbol
                ),
                evidence: vec![],
                why: vec![
                    format!(
                        "both members carry Canonical role for stem `{}`",
                        cluster.stem
                    ),
                    format!("incumbent canonical: `{}`", primary.symbol),
                ],
                suggested_fix: Some(format!(
                    "drop the `// locus: ot canonical` annotation on `{}` and either \
                     re-annotate it as `// locus: ot boundary {} <name>` or rename the type",
                    extra.name, cluster.concept_id
                )),
                diagnostic_code: None,
            });
        }
    }
    out
}
