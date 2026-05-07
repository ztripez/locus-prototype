//! OT rules. Phase 2 ships OT002 only; OT001/003-007 follow once `locus init`
//! produces a populated lockfile to compare against.

use super::infer::{ConceptCluster, FIELD_OVERLAP_THRESHOLD, InferredRole};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// OT002 — undeclared concept-shaped type.
///
/// Fires when a cluster contains:
/// - at least one Canonical member (annotated `// ot: canonical`), and
/// - one or more Unknown members whose field overlap with the canonical
///   meets [`FIELD_OVERLAP_THRESHOLD`].
///
/// The Unknown members get a Warning by default; under `--agent-strict` they
/// are elevated to Fatal so agent-introduced shadow types can't sneak in.
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

            let suggested_fix = format!(
                "annotate as boundary: `// ot: boundary {} <boundary-name>` above `{}`, \
                 or remove and use `{}` directly",
                cluster.concept_id, member.name, canonical.symbol
            );

            out.push(Diagnostic {
                rule_id: "OT002".to_string(),
                severity: mode.elevate(Severity::Warning),
                span: member.span.clone(),
                concept: Some(cluster.concept_id.clone()),
                message: format!(
                    "`{}` is concept-shaped but not accepted as canonical or boundary",
                    member.symbol
                ),
                why,
                suggested_fix: Some(suggested_fix),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::infer::{ClusterMember, ConceptCluster, InferredRole};
    use super::*;
    use locus_air::AirSpan;

    fn member(
        name: &str,
        symbol: &str,
        role: InferredRole,
        overlap: f32,
        reasons: Vec<String>,
    ) -> ClusterMember {
        ClusterMember {
            symbol: symbol.into(),
            name: name.into(),
            role,
            span: AirSpan::new("t.rs", 1, 1),
            file_path: "t.rs".into(),
            field_overlap: overlap,
            fields: vec!["id".into(), "email".into()],
            reasons,
        }
    }

    #[test]
    fn fires_on_unknown_with_canonical_present() {
        let cluster = ConceptCluster {
            concept_id: "user".into(),
            stem: "User".into(),
            members: vec![
                member("User", "crate::User", InferredRole::Canonical, 1.0, vec![]),
                member(
                    "UserModel",
                    "crate::dto::UserModel",
                    InferredRole::Unknown,
                    1.0,
                    vec!["name suffix `Model`".into()],
                ),
            ],
        };
        let diags = ot002(&[cluster], CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "OT002");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert_eq!(diags[0].concept.as_deref(), Some("user"));
    }

    #[test]
    fn does_not_fire_when_no_canonical() {
        let cluster = ConceptCluster {
            concept_id: "user".into(),
            stem: "User".into(),
            members: vec![
                member(
                    "UserDto",
                    "crate::UserDto",
                    InferredRole::Boundary,
                    1.0,
                    vec![],
                ),
                member(
                    "UserModel",
                    "crate::UserModel",
                    InferredRole::Unknown,
                    1.0,
                    vec![],
                ),
            ],
        };
        let diags = ot002(&[cluster], CheckMode::Human);
        assert!(diags.is_empty(), "no canonical anchor → no OT002");
    }

    #[test]
    fn does_not_fire_on_accepted_boundary() {
        let cluster = ConceptCluster {
            concept_id: "user".into(),
            stem: "User".into(),
            members: vec![
                member("User", "crate::User", InferredRole::Canonical, 1.0, vec![]),
                member(
                    "UserDto",
                    "crate::UserDto",
                    InferredRole::Boundary,
                    1.0,
                    vec![],
                ),
            ],
        };
        assert!(ot002(&[cluster], CheckMode::Human).is_empty());
    }

    #[test]
    fn agent_strict_elevates_to_fatal() {
        let cluster = ConceptCluster {
            concept_id: "user".into(),
            stem: "User".into(),
            members: vec![
                member("User", "crate::User", InferredRole::Canonical, 1.0, vec![]),
                member(
                    "UserModel",
                    "crate::UserModel",
                    InferredRole::Unknown,
                    1.0,
                    vec![],
                ),
            ],
        };
        let diags = ot002(&[cluster], CheckMode::AgentStrict);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn weak_overlap_below_threshold_is_dropped() {
        let cluster = ConceptCluster {
            concept_id: "user".into(),
            stem: "User".into(),
            members: vec![
                member("User", "crate::User", InferredRole::Canonical, 1.0, vec![]),
                member(
                    "UserModel",
                    "crate::UserModel",
                    InferredRole::Unknown,
                    0.2,
                    vec![],
                ),
            ],
        };
        assert!(ot002(&[cluster], CheckMode::Human).is_empty());
    }
}
