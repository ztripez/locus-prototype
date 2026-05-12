//! Advisory-mode semantics: Advisory severity + Advisory status,
//! preserved even under AgentStrict. Unknown-concept stays Advisory
//! even when the typo'd declaration spelled `enforced`.

use super::{
    governance_code_concept, policy_concept, policy_finding_with_code, registered_rule_finding,
    rule_concept, run_with, run_with_mode,
};
use crate::diagnostics::{CheckMode, Severity};
use crate::governance::arch::{
    ArchDeclaration, ArchLoadOutcome, ConceptDeclaration, ConceptEnforcement,
};
use crate::governance::decision::DecisionStatus;
use crate::governance::finding::FindingStore;
use crate::governance::registry::{ParadigmRegistry, PolicyRegistry, RuleRegistry};

#[test]
fn advisory_mode_emits_advisory_severity_and_status() {
    // All test concept helpers default to Advisory enforcement, so a
    // non-trivial finding-set should produce Advisory severity +
    // Advisory status across the board.
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: Vec::new(),
        concepts: vec![rule_concept(), policy_concept(), governance_code_concept()],
    });
    let mut store = FindingStore::new();
    store.insert(policy_finding_with_code(0, "ghost-policy", "LOCUS999"));
    let out = run_with(
        &arch,
        store,
        &RuleRegistry::standard(),
        &ParadigmRegistry::standard(),
        &PolicyRegistry::standard(),
    );
    for f in &out.new_findings {
        assert_eq!(f.default_severity, Severity::Advisory);
    }
    for d in &out.decisions {
        assert_eq!(d.severity, Severity::Advisory);
        assert_eq!(d.status, DecisionStatus::Advisory);
    }
}

#[test]
fn advisory_concept_stays_advisory_under_agent_strict() {
    // Even under --agent-strict, an Advisory-mode concept must not
    // elevate. Pure-guide-signal contract is preserved.
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: Vec::new(),
        concepts: vec![rule_concept()],
    });
    let mut store = FindingStore::new();
    store.insert(registered_rule_finding(0, "ZZ999"));
    let out = run_with_mode(
        &arch,
        store,
        &RuleRegistry::standard(),
        &ParadigmRegistry::standard(),
        &PolicyRegistry::standard(),
        CheckMode::AgentStrict,
    );
    assert_eq!(out.new_findings.len(), 1);
    assert_eq!(out.new_findings[0].default_severity, Severity::Advisory);
    assert_eq!(out.decisions[0].severity, Severity::Advisory);
    assert_eq!(out.decisions[0].status, DecisionStatus::Advisory);
}

#[test]
fn unknown_concept_id_stays_advisory_even_if_enforcement_field_present() {
    // A typo in arch.json (`unknown-thing` is not a recognised
    // concept id) is a config-quality issue, not an SoT bypass.
    // Even with `enforcement: enforced` declared, the resulting
    // "unknown concept" LOCUS005 must stay Advisory+Advisory.
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: Vec::new(),
        concepts: vec![ConceptDeclaration {
            id: "unknown-thing".into(),
            source_of_truth: "X".into(),
            registry: "Y".into(),
            enforcement: ConceptEnforcement::Enforced,
        }],
    });
    let out = run_with_mode(
        &arch,
        FindingStore::new(),
        &RuleRegistry::standard(),
        &ParadigmRegistry::standard(),
        &PolicyRegistry::standard(),
        CheckMode::AgentStrict,
    );
    assert_eq!(out.new_findings.len(), 1);
    assert_eq!(out.new_findings[0].default_severity, Severity::Advisory);
    assert_eq!(out.decisions[0].severity, Severity::Advisory);
    assert_eq!(out.decisions[0].status, DecisionStatus::Advisory);
}
