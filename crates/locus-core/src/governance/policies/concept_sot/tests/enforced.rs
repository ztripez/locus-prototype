//! Enforced-mode semantics: Warning under Human, Fatal under
//! AgentStrict, triggering-finding span propagated to LOCUS005, and
//! per-concept mode mixing produces independent severities in the
//! same run. Legacy diagnostics stay LOCUS003's territory even under
//! Enforced.

use super::{
    enforced_rule_concept, legacy_finding, paradigm_concept, registered_rule_finding, run_with,
    run_with_mode,
};
use crate::diagnostics::{CheckMode, Severity};
use crate::governance::arch::{ArchDeclaration, ArchLoadOutcome};
use crate::governance::decision::{DecisionStatus, SeverityChange};
use crate::governance::finding::FindingStore;
use crate::governance::ids::ParadigmId;
use crate::governance::registry::{ParadigmRegistry, PolicyRegistry, RuleRegistry};

#[test]
fn enforced_concept_emits_warning_under_human_mode() {
    // Rule concept declared Enforced — an unregistered RuleId
    // finding should produce a LOCUS005 with Warning severity + an
    // Active decision (no longer pure-guide Advisory).
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: Vec::new(),
        concepts: vec![enforced_rule_concept()],
    });
    let mut store = FindingStore::new();
    store.insert(registered_rule_finding(0, "ZZ999")); // not registered
    let out = run_with(
        &arch,
        store,
        &RuleRegistry::standard(),
        &ParadigmRegistry::standard(),
        &PolicyRegistry::standard(),
    );
    assert_eq!(out.new_findings.len(), 1);
    assert_eq!(out.new_findings[0].default_severity, Severity::Warning);
    assert_eq!(out.decisions.len(), 1);
    assert_eq!(out.decisions[0].severity, Severity::Warning);
    assert_eq!(out.decisions[0].status, DecisionStatus::Active);
    // severity_change stays Unchanged — LOCUS005 is the policy
    // making the decision, not a downstream policy mutating one.
    assert_eq!(out.decisions[0].severity_change, SeverityChange::Unchanged);
}

#[test]
fn enforced_concept_preserves_triggering_finding_span() {
    // Codex review of #101: enforced LOCUS005 findings without a span
    // get a synthetic `<governance>` path in the pipeline, which
    // `locus check --changed` filters out before fatal-exit
    // evaluation. Verify the triggering finding's span is propagated
    // so changed-only CI gates still catch enforced bypasses.
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: Vec::new(),
        concepts: vec![enforced_rule_concept()],
    });
    let span = locus_air::AirSpan::new("src/widget.rs", 42, 50);
    let mut trig = registered_rule_finding(0, "ZZ999");
    trig.span = Some(span.clone());
    let mut store = FindingStore::new();
    store.insert(trig);
    let out = run_with(
        &arch,
        store,
        &RuleRegistry::standard(),
        &ParadigmRegistry::standard(),
        &PolicyRegistry::standard(),
    );
    assert_eq!(out.new_findings.len(), 1);
    assert_eq!(
        out.new_findings[0].span,
        Some(span),
        "LOCUS005 must carry the triggering finding's span so \
         changed-only filters can route it correctly"
    );
}

#[test]
fn enforced_concept_elevates_to_fatal_under_agent_strict() {
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: Vec::new(),
        concepts: vec![enforced_rule_concept()],
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
    assert_eq!(out.new_findings[0].default_severity, Severity::Fatal);
    assert_eq!(out.decisions[0].severity, Severity::Fatal);
    assert_eq!(out.decisions[0].status, DecisionStatus::Active);
}

#[test]
fn mode_mixing_per_concept() {
    // arch.json declares `rule` Enforced and `paradigm` Advisory.
    // Emitting one bypass per concept should yield two LOCUS005s
    // with different severity/status pairs in the same run.
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: Vec::new(),
        concepts: vec![enforced_rule_concept(), paradigm_concept()],
    });
    let mut store = FindingStore::new();
    // Unregistered rule id → emits via rule concept (Enforced).
    store.insert(registered_rule_finding(0, "ZZ999"));
    // Unregistered paradigm id → emits via paradigm concept (Advisory).
    let mut paradigm_f = registered_rule_finding(1, "CX001");
    paradigm_f.paradigm_id = Some(ParadigmId::new("ZZ"));
    store.insert(paradigm_f);

    let out = run_with(
        &arch,
        store,
        &RuleRegistry::standard(),
        &ParadigmRegistry::standard(),
        &PolicyRegistry::standard(),
    );
    let by_concept: std::collections::BTreeMap<_, _> = out
        .new_findings
        .iter()
        .zip(out.decisions.iter())
        .map(|(f, d)| (f.concept.clone().unwrap_or_default(), (f, d)))
        .collect();
    let (rule_f, rule_d) = by_concept
        .get("rule")
        .expect("expected one LOCUS005 for rule concept");
    let (par_f, par_d) = by_concept
        .get("paradigm")
        .expect("expected one LOCUS005 for paradigm concept");
    assert_eq!(rule_f.default_severity, Severity::Warning);
    assert_eq!(rule_d.severity, Severity::Warning);
    assert_eq!(rule_d.status, DecisionStatus::Active);
    assert_eq!(par_f.default_severity, Severity::Advisory);
    assert_eq!(par_d.severity, Severity::Advisory);
    assert_eq!(par_d.status, DecisionStatus::Advisory);
}

#[test]
fn legacy_diagnostic_still_skipped_in_enforced_mode() {
    // Legacy diagnostics belong to LOCUS003 alone. Declaring rule
    // Enforced must NOT cause LOCUS005 to fire for legacy entries.
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: Vec::new(),
        concepts: vec![enforced_rule_concept()],
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
        "legacy diagnostics must not trigger LOCUS005 even in Enforced mode; got {:?}",
        out.new_findings
            .iter()
            .map(|f| f.message.as_str())
            .collect::<Vec<_>>()
    );
}
