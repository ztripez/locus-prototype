//! LOCUS005 fires: bypass observations dedupe across repeated
//! findings, unknown concept ids emit one Advisory, and finding-source
//! mismatch (unregistered policy id) produces a bypass.

use super::super::RuleFinding;
use super::{governance_code_concept, policy_concept, policy_finding_with_code, run_with};
use crate::diagnostics::Severity;
use crate::governance::arch::{
    ArchDeclaration, ArchLoadOutcome, ConceptDeclaration, ConceptEnforcement,
};
use crate::governance::finding::FindingStore;
use crate::governance::registry::{ParadigmRegistry, PolicyRegistry, RuleRegistry};

#[test]
fn fires_once_per_unique_unregistered_governance_code() {
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: Vec::new(),
        concepts: vec![governance_code_concept()],
    });
    let mut store = FindingStore::new();
    // Two findings, same unregistered code → one LOCUS005 (dedupe).
    store.insert(policy_finding_with_code(
        0,
        "registry-integrity",
        "LOCUS999",
    ));
    store.insert(policy_finding_with_code(
        1,
        "registry-integrity",
        "LOCUS999",
    ));
    let out = run_with(
        &arch,
        store,
        &RuleRegistry::standard(),
        &ParadigmRegistry::standard(),
        &PolicyRegistry::standard(),
    );
    let locus005: Vec<&RuleFinding> = out
        .new_findings
        .iter()
        .filter(|f| f.diagnostic_code.as_deref() == Some("LOCUS005"))
        .collect();
    assert_eq!(
        locus005.len(),
        1,
        "two findings with same unregistered code → one dedupe'd LOCUS005; got {:?}",
        locus005
            .iter()
            .map(|f| f.message.as_str())
            .collect::<Vec<_>>()
    );
    let f = locus005[0];
    assert!(
        f.why
            .iter()
            .any(|w| w.contains("GovernanceDiagnosticRegistry")),
        "why[] should reference the declared SoT; got {:?}",
        f.why
    );
    assert_eq!(f.concept.as_deref(), Some("governance-code"));
}

#[test]
fn fires_for_finding_from_unregistered_policy() {
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: Vec::new(),
        concepts: vec![policy_concept()],
    });
    let mut store = FindingStore::new();
    // Synthetic finding from a policy that's NOT in the registry. The
    // diagnostic_code is registered (LOCUS003), so the only bypass is
    // the policy id itself.
    store.insert(policy_finding_with_code(
        0,
        "nonexistent-policy",
        "LOCUS003",
    ));
    let out = run_with(
        &arch,
        store,
        &RuleRegistry::standard(),
        &ParadigmRegistry::standard(),
        &PolicyRegistry::standard(),
    );
    let locus005: Vec<&RuleFinding> = out
        .new_findings
        .iter()
        .filter(|f| f.diagnostic_code.as_deref() == Some("LOCUS005"))
        .collect();
    assert_eq!(locus005.len(), 1);
    assert!(locus005[0].message.contains("policy"));
    assert!(
        locus005[0]
            .why
            .iter()
            .any(|w| w.contains("nonexistent-policy"))
            || locus005[0].why.iter().any(|w| w.contains("policy"))
    );
}

#[test]
fn unknown_concept_id_emits_one_advisory() {
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: Vec::new(),
        concepts: vec![ConceptDeclaration {
            id: "unknown-thing".into(),
            source_of_truth: "X".into(),
            registry: "Y".into(),
            enforcement: ConceptEnforcement::Advisory,
        }],
    });
    let out = run_with(
        &arch,
        FindingStore::new(),
        &RuleRegistry::standard(),
        &ParadigmRegistry::standard(),
        &PolicyRegistry::standard(),
    );
    let locus005: Vec<&RuleFinding> = out
        .new_findings
        .iter()
        .filter(|f| f.diagnostic_code.as_deref() == Some("LOCUS005"))
        .collect();
    assert_eq!(locus005.len(), 1);
    assert!(locus005[0].message.contains("unknown-thing"));
    assert_eq!(
        locus005[0].default_severity,
        Severity::Advisory,
        "LOCUS005 is always Advisory"
    );
}
