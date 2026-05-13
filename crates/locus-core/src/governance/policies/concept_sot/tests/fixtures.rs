//! Shared fixture helpers for the `concept_sot` unit test suite.

use super::super::policy::ConceptSourceOfTruthPolicy;
use crate::diagnostics::Severity;
use crate::governance::arch::{ArchLoadOutcome, ConceptDeclaration, ConceptEnforcement};
use crate::governance::finding::{FindingSource, FindingStore, RuleFinding};
use crate::governance::ids::{FindingId, FindingIdMinter, PolicyId, RuleId};
use crate::governance::policy::{PolicyContext, PolicyDefinition, PolicyOutput};
use crate::governance::registry::{ParadigmRegistry, PolicyRegistry, RuleRegistry};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub(crate) fn registered_rule_finding(id_raw: u64, rule_code: &'static str) -> RuleFinding {
    RuleFinding {
        id: FindingId::from_raw_for_test(id_raw),
        source: FindingSource::RegisteredRule(RuleId::new(rule_code)),
        rule_id: Some(RuleId::new(rule_code)),
        paradigm_id: None,
        default_severity: Severity::Warning,
        span: None,
        concept: None,
        message: "msg".into(),
        evidence: Vec::new(),
        why: Vec::new(),
        suggested_fix: None,
        diagnostic_code: None,
    }
}

pub(crate) fn legacy_finding(id_raw: u64, rule_code: &str) -> RuleFinding {
    RuleFinding {
        id: FindingId::from_raw_for_test(id_raw),
        source: FindingSource::LegacyDiagnostic {
            rule_code: rule_code.into(),
            paradigm: None,
        },
        rule_id: None,
        paradigm_id: None,
        default_severity: Severity::Warning,
        span: None,
        concept: None,
        message: "legacy".into(),
        evidence: Vec::new(),
        why: Vec::new(),
        suggested_fix: None,
        diagnostic_code: None,
    }
}

pub(crate) fn policy_finding_with_code(id: u64, owner: &'static str, code: &str) -> RuleFinding {
    RuleFinding {
        id: FindingId::from_raw_for_test(id),
        source: FindingSource::Policy(PolicyId::new(owner)),
        rule_id: None,
        paradigm_id: None,
        default_severity: Severity::Advisory,
        span: None,
        concept: None,
        message: "m".into(),
        evidence: Vec::new(),
        why: Vec::new(),
        suggested_fix: None,
        diagnostic_code: Some(code.into()),
    }
}

pub(crate) fn run_with(
    arch: &ArchLoadOutcome,
    store: FindingStore,
    rules: &RuleRegistry,
    paradigms: &ParadigmRegistry,
    policies: &PolicyRegistry,
) -> PolicyOutput {
    run_with_mode(
        arch,
        store,
        rules,
        paradigms,
        policies,
        crate::diagnostics::CheckMode::Human,
    )
}

pub(crate) fn run_with_mode(
    arch: &ArchLoadOutcome,
    store: FindingStore,
    rules: &RuleRegistry,
    paradigms: &ParadigmRegistry,
    policies: &PolicyRegistry,
    mode: crate::diagnostics::CheckMode,
) -> PolicyOutput {
    let air = AirWorkspace::new(Vec::new());
    let lf = Lockfile::empty();
    let minter = FindingIdMinter::new();
    let ctx = PolicyContext {
        air: &air,
        lockfile: &lf,
        mode,
        rule_registry: rules,
        paradigm_registry: paradigms,
        policy_registry: policies,
        findings: &store,
        prior_decisions: &[],
        finding_ids: &minter,
        arch,
    };
    ConceptSourceOfTruthPolicy.decide(&ctx)
}

pub(crate) fn rule_concept() -> ConceptDeclaration {
    ConceptDeclaration {
        id: "rule".into(),
        source_of_truth: "RuleDefinition".into(),
        registry: "RuleRegistry".into(),
        enforcement: ConceptEnforcement::Advisory,
    }
}

pub(crate) fn policy_concept() -> ConceptDeclaration {
    ConceptDeclaration {
        id: "policy".into(),
        source_of_truth: "PolicyDefinition".into(),
        registry: "PolicyRegistry".into(),
        enforcement: ConceptEnforcement::Advisory,
    }
}

pub(crate) fn governance_code_concept() -> ConceptDeclaration {
    ConceptDeclaration {
        id: "governance-code".into(),
        source_of_truth: "GovernanceDiagnosticRegistry".into(),
        registry: "GovernanceDiagnosticRegistry".into(),
        enforcement: ConceptEnforcement::Advisory,
    }
}

pub(crate) fn paradigm_concept() -> ConceptDeclaration {
    ConceptDeclaration {
        id: "paradigm".into(),
        source_of_truth: "ParadigmDefinition".into(),
        registry: "ParadigmRegistry".into(),
        enforcement: ConceptEnforcement::Advisory,
    }
}

pub(crate) fn enforced_rule_concept() -> ConceptDeclaration {
    ConceptDeclaration {
        id: "rule".into(),
        source_of_truth: "RuleDefinition".into(),
        registry: "RuleRegistry".into(),
        enforcement: ConceptEnforcement::Enforced,
    }
}
