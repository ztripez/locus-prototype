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
use crate::diagnostics::{CheckMode, Severity};
use crate::governance::evidence::{Confidence, Evidence};
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

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
        let section: OtSection = ctx.lockfile.paradigm_section("OT").unwrap_or_default();
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
            out.push(make_finding_inner(
                cluster,
                canonical,
                member,
                mode,
                finding_ids,
            ));
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
        let findings = run_observe(&air, &lf, CheckMode::Human);

        assert_eq!(
            findings.len(),
            1,
            "expected one OT002 finding, got {findings:?}"
        );
        assert_finding_shape(&findings[0]);
        assert_high_confidence_overlap(&findings[0]);
    }

    /// Build a stand-alone `RuleContext` for `Ot002Rule::observe` and return
    /// the findings. Tests use this instead of inlining the context dance.
    fn run_observe(air: &AirWorkspace, lf: &Lockfile, mode: CheckMode) -> Vec<RuleFinding> {
        let rules = RuleRegistry::standard();
        let paradigms = ParadigmRegistry::empty();
        let minter = FindingIdMinter::new();
        let ctx = RuleContext {
            air,
            lockfile: lf,
            mode,
            rule_registry: &rules,
            paradigm_registry: &paradigms,
            finding_ids: &minter,
        };
        Ot002Rule.observe(&ctx)
    }

    /// Assert the finding has the registered-rule shape (source, ids,
    /// severity, message stem).
    fn assert_finding_shape(f: &RuleFinding) {
        assert_eq!(f.source, FindingSource::RegisteredRule(OT002_ID));
        assert_eq!(f.rule_id, Some(OT002_ID));
        assert_eq!(f.paradigm_id, Some(OT_PARADIGM));
        assert_eq!(f.default_severity, Severity::Warning);
        assert!(
            f.message.contains("concept-shaped but not accepted"),
            "expected legacy-compatible message, got `{}`",
            f.message
        );
    }

    /// Assert the typed evidence is `InferenceConfidence::High` with an
    /// "overlaps" signal (1.0 field overlap → High tier).
    fn assert_high_confidence_overlap(f: &RuleFinding) {
        assert_eq!(f.evidence.len(), 1);
        match &f.evidence[0] {
            Evidence::InferenceConfidence { score, signals } => {
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

    /// Workspace with a canonical-annotated `User` type and an unannotated
    /// `UserResponse` sibling sharing 100% of fields and the `User` stem.
    /// Drives the OT002 cluster builder + rule end-to-end.
    fn workspace_with_canonical_and_sibling() -> AirWorkspace {
        let canonical = user_type("User", "demo::user::User", 5);
        let sibling = user_type("UserResponse", "demo::user::UserResponse", 12);
        AirWorkspace::new(vec![AirPackage {
            name: "demo".into(),
            version: "0.0.1".into(),
            root_dir: "/tmp/demo".into(),
            files: vec![AirFile {
                path: "src/user.rs".into(),
                module_path: Some("demo::user".into()),
                items: vec![AirItem::Type(canonical), AirItem::Type(sibling)],
                hints: vec![canonical_hint_at_line(5)],
                parse_error: None,
                line_count: 20,
            }],
        }])
    }

    /// Build a `User`-shaped struct (fields `id`, `name`) with the given
    /// name/symbol/span. Used to construct both the canonical and the
    /// shadow sibling — they share field shape so `field_overlap` is 1.0.
    fn user_type(name: &str, symbol: &str, start_line: u32) -> AirType {
        // locus: allow OT004 — test fixture; constructs AIR canonical for the rule under test
        AirType {
            kind: TypeKind::Struct,
            name: name.into(),
            symbol: symbol.into(),
            symbol_segments: Vec::new(),
            visibility: Visibility::Public,
            fields: vec![user_field("id", "u32"), user_field("name", "String")],
            variants: Vec::new(),
            decorators: Vec::new(),
            span: AirSpan::new("src/user.rs", start_line, start_line + 3),
            doc: None,
        }
    }

    fn user_field(name: &str, ty: &str) -> AirField {
        // locus: allow OT004 — test fixture; constructs AIR canonical for the rule under test
        AirField {
            name: name.into(),
            type_text: ty.into(),
            visibility: Visibility::Public,
        }
    }

    /// Build a `// locus: ot canonical` hint whose `target_span` matches the
    /// canonical type's first line, so the inference associates the hint
    /// with the right item.
    fn canonical_hint_at_line(target_line: u32) -> AirHint {
        // locus: allow OT004 — test fixture; constructs AIR canonical hint for the rule under test
        AirHint {
            kind: HintKind::Canonical,
            raw: "// locus: ot canonical".into(),
            span: AirSpan::new("src/user.rs", target_line - 1, target_line - 1),
            target_span: Some(AirSpan::new("src/user.rs", target_line, target_line)),
        }
    }
}
