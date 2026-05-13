//! LOCUS005 should NOT fire: registered identifiers, empty arch
//! declarations, legacy diagnostics.

use super::{
    governance_code_concept, legacy_finding, policy_finding_with_code, registered_rule_finding,
    rule_concept, run_with,
};
use crate::governance::arch::{ArchDeclaration, ArchLoadOutcome, ConceptDeclaration};
use crate::governance::finding::FindingStore;
use crate::governance::ids::ParadigmId;
use crate::governance::registry::{ParadigmRegistry, PolicyRegistry, RuleRegistry};

#[test]
fn silent_when_arch_missing() {
    let out = run_with(
        &ArchLoadOutcome::Missing,
        FindingStore::new(),
        &RuleRegistry::standard(),
        &ParadigmRegistry::standard(),
        &PolicyRegistry::standard(),
    );
    assert!(out.new_findings.is_empty());
    assert!(out.decisions.is_empty());
}

#[test]
fn silent_when_concepts_empty() {
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: vec!["registry-integrity".into()],
        concepts: Vec::new(),
    });
    let out = run_with(
        &arch,
        FindingStore::new(),
        &RuleRegistry::standard(),
        &ParadigmRegistry::standard(),
        &PolicyRegistry::standard(),
    );
    assert!(out.new_findings.is_empty());
}

#[test]
fn legacy_diagnostic_skipped_for_rule_concept() {
    // CRITICAL: legacy diagnostics belong to LOCUS003 alone. LOCUS005
    // must not fire for them, even if their rule_code happens to be
    // unregistered (which is the whole point of LOCUS003 -- migration
    // debt is already tracked).
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: Vec::new(),
        concepts: vec![rule_concept()],
    });
    let mut store = FindingStore::new();
    store.insert(legacy_finding(0, "ZZ999"));
    let out = run_with(
        &arch,
        store,
        &RuleRegistry::standard(),
        &ParadigmRegistry::standard(),
        &PolicyRegistry::standard(),
    );
    assert!(
        out.new_findings.is_empty(),
        "legacy diagnostics must not trigger LOCUS005; got {:?}",
        out.new_findings
            .iter()
            .map(|f| f.message.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn governance_code_concept_skips_locus_codes_that_are_registered() {
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: Vec::new(),
        concepts: vec![governance_code_concept()],
    });
    let mut store = FindingStore::new();
    // LOCUS003 IS in the registry — must not trigger LOCUS005.
    store.insert(policy_finding_with_code(
        0,
        "registry-integrity",
        "LOCUS003",
    ));
    let out = run_with(
        &arch,
        store,
        &RuleRegistry::standard(),
        &ParadigmRegistry::standard(),
        &PolicyRegistry::standard(),
    );
    assert!(
        out.new_findings.is_empty(),
        "registered LOCUS003 must not trigger LOCUS005"
    );
}

#[test]
fn rule_concept_silent_for_registered_rule() {
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: Vec::new(),
        concepts: vec![rule_concept()],
    });
    let mut store = FindingStore::new();
    // CX001 IS in standard rule registry.
    store.insert(registered_rule_finding(0, "CX001"));
    let out = run_with(
        &arch,
        store,
        &RuleRegistry::standard(),
        &ParadigmRegistry::standard(),
        &PolicyRegistry::standard(),
    );
    assert!(
        out.new_findings.is_empty(),
        "registered rule must not trigger LOCUS005"
    );
}

#[test]
fn paradigm_concept_silent_for_registered_paradigm() {
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: Vec::new(),
        concepts: vec![ConceptDeclaration {
            id: "paradigm".into(),
            source_of_truth: "ParadigmDefinition".into(),
            registry: "ParadigmRegistry".into(),
            enforcement: crate::governance::arch::ConceptEnforcement::Advisory,
        }],
    });
    // CX paradigm IS in the standard registry — must not trigger LOCUS005.
    let mut store = FindingStore::new();
    let mut f = registered_rule_finding(0, "CX001");
    f.paradigm_id = Some(ParadigmId::new("CX"));
    store.insert(f);
    let out = run_with(
        &arch,
        store,
        &RuleRegistry::standard(),
        &ParadigmRegistry::standard(),
        &PolicyRegistry::standard(),
    );
    assert!(out.new_findings.is_empty());
}
