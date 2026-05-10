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
use crate::diagnostics::{Diagnostic, Severity};

pub fn ot001(clusters: &[ConceptCluster], _mode: crate::diagnostics::CheckMode) -> Vec<Diagnostic> {
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

        // Diagnostic per *extra* canonical — pin the first as the "incumbent"
        // and report each additional one. This makes the fixes obvious: drop
        // the redundant `// locus: ot canonical` annotation or rename the type.
        let primary = canonicals[0];
        for extra in &canonicals[1..] {
            out.push(Diagnostic {
                rule_id: "OT001".to_string(),
                severity: Severity::Fatal,
                span: extra.span.clone(),
                concept: Some(cluster.concept_id.clone()),
                message: format!(
                    "`{}` is a second canonical for concept `{}`; \
                     `{}` is already canonical",
                    extra.symbol, cluster.concept_id, primary.symbol
                ),
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
            });
        }
    }
    out
}
