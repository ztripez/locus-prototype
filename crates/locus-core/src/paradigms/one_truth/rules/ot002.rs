//! OT002 — undeclared concept-shaped type.
//!
//! Fires when a cluster contains:
//! - at least one Canonical member (annotated `// locus: ot canonical`), and
//! - one or more Unknown members whose field overlap with the canonical
//!   meets [`FIELD_OVERLAP_THRESHOLD`].
//!
//! The Unknown members get a Warning by default; under `--agent-strict` they
//! are elevated to Fatal so agent-introduced shadow types can't sneak in.

use super::super::infer::{ConceptCluster, FIELD_OVERLAP_THRESHOLD, InferredRole};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};
use crate::governance::evidence::{Confidence, Evidence};
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

pub fn ot002(clusters: &[ConceptCluster], mode: CheckMode) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for cluster in clusters {
        let canonical = cluster
            .members
            .iter()
            .find(|m| m.role == InferredRole::Canonical);
        let Some(canonical) = canonical else {
            continue; // no anchor → can't tell which is the shadow
        };

        for member in &cluster.members {
            if member.role != InferredRole::Unknown {
                continue;
            }
            if member.field_overlap < FIELD_OVERLAP_THRESHOLD {
                continue;
            }
            out.push(ot002_diagnostic(cluster, canonical, member, mode));
        }
    }
    out
}

fn ot002_diagnostic(
    cluster: &super::super::infer::ConceptCluster,
    canonical: &super::super::infer::ClusterMember,
    member: &super::super::infer::ClusterMember,
    mode: CheckMode,
) -> Diagnostic {
    let mut why = vec![
        format!(
            "overlaps {:.0}% with `{}` (canonical for `{}`)",
            member.field_overlap * 100.0,
            canonical.name,
            cluster.concept_id
        ),
        format!("name shares stem `{}`", cluster.stem),
    ];
    why.extend(member.reasons.iter().cloned());
    Diagnostic {
        rule_id: "OT002".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: member.span.clone(),
        concept: Some(cluster.concept_id.clone()),
        message: format!(
            "`{}` is concept-shaped but not accepted as canonical or boundary",
            member.symbol
        ),
        why,
        suggested_fix: Some(format!(
            "annotate as boundary: `// locus: ot boundary {} <boundary-name>` above `{}`, \
             or remove and use `{}` directly",
            cluster.concept_id, member.name, canonical.symbol
        )),
    }
}

pub struct Ot002Rule;

pub static OT002_RULE: Ot002Rule = Ot002Rule;

const OT002_ID: RuleId = RuleId::new("OT002");
const OT_PARADIGM: ParadigmId = ParadigmId::new("OT");

impl RuleDefinition for Ot002Rule {
    fn id(&self) -> RuleId {
        OT002_ID
    }
    fn paradigm(&self) -> ParadigmId {
        OT_PARADIGM
    }
    fn title(&self) -> &'static str {
        "undeclared concept-shaped type"
    }
    fn default_severity(&self) -> Severity {
        Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::super::lockfile_schema::OtSection;
        let section: OtSection = ctx
            .lockfile
            .paradigm_section("OT")
            .unwrap_or_default();
        let clusters = super::super::infer::cluster_concepts_with_lockfile(ctx.air, &section);
        produce_findings_from_clusters(&clusters, ctx.mode, ctx.finding_ids)
    }
}

/// Cluster-level helper exposed for tests. Walks pre-computed clusters
/// and emits findings — same logic as `Ot002Rule::observe` minus the
/// AIR-to-clusters step.
pub(crate) fn produce_findings_from_clusters(
    clusters: &[ConceptCluster],
    mode: CheckMode,
    finding_ids: &crate::governance::ids::FindingIdMinter,
) -> Vec<RuleFinding> {
    let mut out = Vec::new();
    for cluster in clusters {
        let canonical = cluster
            .members
            .iter()
            .find(|m| m.role == InferredRole::Canonical);
        let Some(canonical) = canonical else {
            continue;
        };
        for member in &cluster.members {
            if member.role != InferredRole::Unknown {
                continue;
            }
            if member.field_overlap < FIELD_OVERLAP_THRESHOLD {
                continue;
            }
            out.push(make_finding_inner(cluster, canonical, member, mode, finding_ids));
        }
    }
    out
}

fn make_finding_inner(
    cluster: &ConceptCluster,
    canonical: &super::super::infer::ClusterMember,
    member: &super::super::infer::ClusterMember,
    mode: CheckMode,
    finding_ids: &crate::governance::ids::FindingIdMinter,
) -> RuleFinding {
    let mut signals = vec![
        format!(
            "overlaps {:.0}% with `{}` (canonical for `{}`)",
            member.field_overlap * 100.0,
            canonical.name,
            cluster.concept_id
        ),
        format!("name shares stem `{}`", cluster.stem),
    ];
    signals.extend(member.reasons.iter().cloned());

    let severity = mode.elevate(Severity::Warning);
    let why = signals.clone();
    RuleFinding {
        id: finding_ids.next(),
        source: FindingSource::RegisteredRule(OT002_ID),
        rule_id: Some(OT002_ID),
        paradigm_id: Some(OT_PARADIGM),
        default_severity: severity,
        span: Some(member.span.clone()),
        concept: Some(cluster.concept_id.clone()),
        message: format!(
            "`{}` is concept-shaped but not accepted as canonical or boundary",
            member.symbol
        ),
        evidence: vec![Evidence::InferenceConfidence {
            score: confidence_from_overlap(member.field_overlap),
            signals,
        }],
        why,
        suggested_fix: Some(format!(
            "annotate as boundary: `// locus: ot boundary {} <boundary-name>` above `{}`, \
             or remove and use `{}` directly",
            cluster.concept_id, member.name, canonical.symbol
        )),
        diagnostic_code: None,
    }
}

/// Discretize the inference's `field_overlap` (0.0..=1.0) into a
/// `Confidence` tier. Mirrors the spec's 0.50/0.70/0.90 confidence
/// ladder (`Severity::from_confidence` in `diagnostics.rs`). Since the
/// rule's overlap gate is `FIELD_OVERLAP_THRESHOLD = 0.50`, an overlap
/// of < 0.70 maps to Low, [0.70, 0.90) to Medium, and ≥ 0.90 to High.
fn confidence_from_overlap(overlap: f32) -> Confidence {
    if overlap >= 0.90 {
        Confidence::High
    } else if overlap >= 0.70 {
        Confidence::Medium
    } else {
        Confidence::Low
    }
}

#[cfg(test)]
mod ot002_rule_tests {
    use super::*;
    use crate::diagnostics::CheckMode;
    use crate::governance::ids::FindingIdMinter;
    use crate::governance::registry::{ParadigmRegistry, RuleRegistry};
    use crate::lockfile::Lockfile;
    use locus_air::{
        AirField, AirFile, AirHint, AirItem, AirPackage, AirSpan, AirType, AirWorkspace, HintKind,
        TypeKind, Visibility,
    };

    /// Build a workspace with two types sharing stem `User` and overlapping
    /// fields: one is `// locus: ot canonical`-annotated, the other has
    /// no hint. The migrated rule should emit one OT002 finding on the
    /// undeclared sibling.
    #[test]
    fn fires_on_concept_shaped_sibling_without_annotation() {
        let air = workspace_with_canonical_and_sibling();
        let lf = Lockfile::default();
        let rules = RuleRegistry::standard();
        let paradigms = ParadigmRegistry::empty();
        let minter = FindingIdMinter::new();
        let ctx = RuleContext {
            air: &air,
            lockfile: &lf,
            mode: CheckMode::Human,
            rule_registry: &rules,
            paradigm_registry: &paradigms,
            finding_ids: &minter,
        };

        let findings = Ot002Rule.observe(&ctx);
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one OT002 finding, got {findings:?}"
        );
        let f = &findings[0];
        assert_eq!(f.source, FindingSource::RegisteredRule(OT002_ID));
        assert_eq!(f.rule_id, Some(OT002_ID));
        assert_eq!(f.paradigm_id, Some(OT_PARADIGM));
        assert_eq!(f.default_severity, Severity::Warning);
        assert!(
            f.message.contains("concept-shaped but not accepted"),
            "expected legacy-compatible message, got `{}`",
            f.message
        );

        // Typed evidence.
        assert_eq!(f.evidence.len(), 1);
        match &f.evidence[0] {
            Evidence::InferenceConfidence { score, signals } => {
                // Two fields overlap fully (id, name) → field_overlap is
                // 1.0 → Confidence::High.
                assert_eq!(*score, Confidence::High);
                assert!(
                    signals.iter().any(|s| s.contains("overlaps")),
                    "expected overlap signal in {signals:?}"
                );
            }
            other => panic!("expected InferenceConfidence evidence, got {other:?}"),
        }
    }

    #[test]
    fn confidence_ladder_matches_spec_thresholds() {
        assert_eq!(confidence_from_overlap(1.00), Confidence::High);
        assert_eq!(confidence_from_overlap(0.95), Confidence::High);
        assert_eq!(confidence_from_overlap(0.90), Confidence::High);
        assert_eq!(confidence_from_overlap(0.89), Confidence::Medium);
        assert_eq!(confidence_from_overlap(0.70), Confidence::Medium);
        assert_eq!(confidence_from_overlap(0.69), Confidence::Low);
        assert_eq!(confidence_from_overlap(0.50), Confidence::Low);
    }

    fn workspace_with_canonical_and_sibling() -> AirWorkspace {
        let canonical = AirType {
            kind: TypeKind::Struct,
            name: "User".into(),
            symbol: "demo::user::User".into(),
            symbol_segments: Vec::new(),
            visibility: Visibility::Public,
            fields: vec![
                AirField {
                    name: "id".into(),
                    type_text: "u32".into(),
                    visibility: Visibility::Public,
                },
                AirField {
                    name: "name".into(),
                    type_text: "String".into(),
                    visibility: Visibility::Public,
                },
            ],
            variants: Vec::new(),
            decorators: Vec::new(),
            span: AirSpan::new("src/user.rs", 5, 8),
            doc: None,
        };
        let sibling = AirType {
            kind: TypeKind::Struct,
            name: "UserResponse".into(),
            symbol: "demo::user::UserResponse".into(),
            symbol_segments: Vec::new(),
            visibility: Visibility::Public,
            fields: vec![
                AirField {
                    name: "id".into(),
                    type_text: "u32".into(),
                    visibility: Visibility::Public,
                },
                AirField {
                    name: "name".into(),
                    type_text: "String".into(),
                    visibility: Visibility::Public,
                },
            ],
            variants: Vec::new(),
            decorators: Vec::new(),
            span: AirSpan::new("src/user.rs", 12, 15),
            doc: None,
        };
        // The canonical hint sits one line above the canonical type's
        // span (line 4 hints, line 5 type) — target_span points at line 5
        // so the inference matches the hint to the User struct.
        let hint = AirHint {
            kind: HintKind::Canonical,
            raw: "// locus: ot canonical".into(),
            span: AirSpan::new("src/user.rs", 4, 4),
            target_span: Some(AirSpan::new("src/user.rs", 5, 5)),
        };
        AirWorkspace::new(vec![AirPackage {
            name: "demo".into(),
            version: "0.0.1".into(),
            root_dir: "/tmp/demo".into(),
            files: vec![AirFile {
                path: "src/user.rs".into(),
                module_path: Some("demo::user".into()),
                items: vec![AirItem::Type(canonical), AirItem::Type(sibling)],
                hints: vec![hint],
                parse_error: None,
                line_count: 20,
            }],
        }])
    }
}
