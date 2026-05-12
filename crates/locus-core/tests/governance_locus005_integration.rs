//! Integration tests for LOCUS005 architecture-intent emission.
//!
//! Verifies that `ConceptSourceOfTruthPolicy` emits guide-shaped LOCUS005
//! advisories when observed state bypasses the declared source of truth,
//! and — crucially — that legacy diagnostics stay in LOCUS003's lane.
//!
//! These tests run the policy in isolation against a synthetic
//! `FindingStore` so we can inject precise bypass shapes that the
//! standard pipeline wouldn't produce on its own (e.g. a finding from a
//! policy that's not in the registry).

use locus_air::AirWorkspace;
use locus_core::governance::finding::RuleFinding;
use locus_core::governance::ids::{FindingIdMinter, ParadigmId, PolicyId, RuleId};
use locus_core::governance::policy::PolicyContext;
use locus_core::governance::{
    self, ArchDeclaration, ArchLoadOutcome, ConceptDeclaration, ConceptEnforcement,
    ConceptSourceOfTruthPolicy, FindingSource, FindingStore, ParadigmRegistry, PolicyDefinition,
    PolicyRegistry, RuleRegistry,
};
use locus_core::{CheckMode, Lockfile, Severity};

fn rule_concept() -> ConceptDeclaration {
    ConceptDeclaration {
        id: "rule".into(),
        source_of_truth: "RuleDefinition".into(),
        registry: "RuleRegistry".into(),
        enforcement: ConceptEnforcement::Advisory,
    }
}

fn paradigm_concept() -> ConceptDeclaration {
    ConceptDeclaration {
        id: "paradigm".into(),
        source_of_truth: "ParadigmDefinition".into(),
        registry: "ParadigmRegistry".into(),
        enforcement: ConceptEnforcement::Advisory,
    }
}

fn policy_concept() -> ConceptDeclaration {
    ConceptDeclaration {
        id: "policy".into(),
        source_of_truth: "PolicyDefinition".into(),
        registry: "PolicyRegistry".into(),
        enforcement: ConceptEnforcement::Advisory,
    }
}

fn governance_code_concept() -> ConceptDeclaration {
    ConceptDeclaration {
        id: "governance-code".into(),
        source_of_truth: "GovernanceDiagnosticRegistry".into(),
        registry: "GovernanceDiagnosticRegistry".into(),
        enforcement: ConceptEnforcement::Advisory,
    }
}

fn enforced_rule_concept() -> ConceptDeclaration {
    ConceptDeclaration {
        id: "rule".into(),
        source_of_truth: "RuleDefinition".into(),
        registry: "RuleRegistry".into(),
        enforcement: ConceptEnforcement::Enforced,
    }
}

fn locus_standard_concepts() -> Vec<ConceptDeclaration> {
    vec![
        rule_concept(),
        paradigm_concept(),
        policy_concept(),
        governance_code_concept(),
    ]
}

/// Test scaffold that builds a fresh `FindingStore` from a list of
/// (factory) closures so each finding gets a unique id from the supplied
/// `FindingIdMinter`. Returns the LOCUS005 `RuleFinding`s emitted by
/// `ConceptSourceOfTruthPolicy`.
fn run_isolated<F>(arch: ArchLoadOutcome, populate: F) -> Vec<RuleFinding>
where
    F: FnOnce(&FindingIdMinter, &mut FindingStore),
{
    run_isolated_with_mode(arch, CheckMode::Human, populate).new_findings
}

/// Returns the full policy output (findings + decisions) under the
/// supplied mode. Used by the enforced-mode tests to pin the rendered
/// severity that the pipeline's materializer would emit verbatim onto
/// the resulting `Diagnostic`.
fn run_isolated_with_mode<F>(
    arch: ArchLoadOutcome,
    mode: CheckMode,
    populate: F,
) -> locus_core::governance::PolicyOutput
where
    F: FnOnce(&FindingIdMinter, &mut FindingStore),
{
    let air = AirWorkspace::new(Vec::new());
    let lf = Lockfile::empty();
    let rules = RuleRegistry::standard();
    let paradigms = ParadigmRegistry::standard();
    let policies = PolicyRegistry::standard();
    let minter = FindingIdMinter::new();
    let mut store = FindingStore::new();
    populate(&minter, &mut store);
    let ctx = PolicyContext {
        air: &air,
        lockfile: &lf,
        mode,
        rule_registry: &rules,
        paradigm_registry: &paradigms,
        policy_registry: &policies,
        findings: &store,
        prior_decisions: &[],
        finding_ids: &minter,
        arch: &arch,
    };
    ConceptSourceOfTruthPolicy.decide(&ctx)
}

fn finding_with_diagnostic_code(
    minter: &FindingIdMinter,
    source: FindingSource,
    diagnostic_code: &str,
) -> RuleFinding {
    RuleFinding {
        id: minter.next(),
        source,
        rule_id: None,
        paradigm_id: None,
        default_severity: Severity::Advisory,
        span: None,
        concept: None,
        message: "synthetic".into(),
        evidence: Vec::new(),
        why: Vec::new(),
        suggested_fix: None,
        diagnostic_code: Some(diagnostic_code.into()),
    }
}

#[test]
fn silent_when_locus_has_valid_declarations() {
    // Locus's own standard registries match the declared concepts. Run
    // through the full pipeline — LOCUS005 must be silent.
    let air = AirWorkspace::new(Vec::new());
    let lf = Lockfile::empty();
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: vec![
            "registry-integrity".into(),
            "registry-coherence".into(),
            "concept-source-of-truth".into(),
            "default-pass-through".into(),
        ],
        concepts: locus_standard_concepts(),
    });
    let out = governance::run_with_arch(&air, &lf, CheckMode::Human, &arch);
    let locus005: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "LOCUS005")
        .collect();
    assert!(
        locus005.is_empty(),
        "valid Locus declarations must be LOCUS005-silent; got: {:?}",
        locus005
            .iter()
            .map(|d| d.message.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn silent_when_concepts_is_empty() {
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: vec!["registry-integrity".into()],
        concepts: Vec::new(),
    });
    let findings = run_isolated(arch, |_minter, _store| {});
    assert!(
        findings.is_empty(),
        "empty concepts → no LOCUS005; got {:?}",
        findings
            .iter()
            .map(|f| f.message.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn silent_when_arch_missing() {
    let findings = run_isolated(ArchLoadOutcome::Missing, |_minter, _store| {});
    assert!(findings.is_empty());
}

#[test]
fn fires_for_unregistered_governance_code() {
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: Vec::new(),
        concepts: vec![governance_code_concept()],
    });
    let findings = run_isolated(arch, |minter, store| {
        store.insert(finding_with_diagnostic_code(
            minter,
            FindingSource::Policy(PolicyId::new("registry-integrity")),
            "LOCUS999",
        ));
    });
    let locus005: Vec<_> = findings
        .iter()
        .filter(|f| f.diagnostic_code.as_deref() == Some("LOCUS005"))
        .collect();
    assert_eq!(
        locus005.len(),
        1,
        "expected one LOCUS005 for unregistered LOCUS999; got {:?}",
        locus005
            .iter()
            .map(|f| f.message.as_str())
            .collect::<Vec<_>>()
    );
    let f = locus005[0];
    assert_eq!(f.concept.as_deref(), Some("governance-code"));
    assert_eq!(f.default_severity, Severity::Advisory);
    assert!(
        f.suggested_fix.is_some(),
        "LOCUS005 must carry a suggested_fix"
    );
    assert!(
        f.why
            .iter()
            .any(|w| w.contains("GovernanceDiagnosticRegistry")),
        "why[] should reference the declared SoT; got {:?}",
        f.why
    );
    assert!(
        f.why.iter().any(|w| w.contains("LOCUS999")),
        "why[] should name the observed bypass; got {:?}",
        f.why
    );
}

#[test]
fn fires_for_finding_from_unregistered_policy() {
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: Vec::new(),
        concepts: vec![policy_concept()],
    });
    let findings = run_isolated(arch, |minter, store| {
        // Carry a registered governance code so the only thing
        // unregistered is the policy itself.
        store.insert(finding_with_diagnostic_code(
            minter,
            FindingSource::Policy(PolicyId::new("nonexistent-policy")),
            "LOCUS003",
        ));
    });
    let locus005: Vec<_> = findings
        .iter()
        .filter(|f| f.diagnostic_code.as_deref() == Some("LOCUS005"))
        .collect();
    assert_eq!(locus005.len(), 1);
    assert_eq!(locus005[0].concept.as_deref(), Some("policy"));
    assert!(
        locus005[0]
            .why
            .iter()
            .any(|w| w.contains("nonexistent-policy")),
        "why[] must mention the observed unregistered policy; got {:?}",
        locus005[0].why
    );
}

#[test]
fn legacy_diagnostic_stays_locus003_not_locus005() {
    // CRITICAL: legacy diagnostics are LOCUS003's territory. LOCUS005
    // must skip them by construction so the new policy never duplicates
    // migration-debt signal.
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: Vec::new(),
        concepts: locus_standard_concepts(),
    });
    let findings = run_isolated(arch, |minter, store| {
        store.insert(RuleFinding {
            id: minter.next(),
            source: FindingSource::LegacyDiagnostic {
                rule_code: "CX999".into(),
                paradigm: None,
            },
            rule_id: None,
            paradigm_id: None,
            default_severity: Severity::Warning,
            span: None,
            concept: None,
            message: "legacy finding".into(),
            evidence: Vec::new(),
            why: Vec::new(),
            suggested_fix: None,
            diagnostic_code: None,
        });
    });
    assert!(
        findings.is_empty(),
        "legacy diagnostics must NOT trigger LOCUS005; got {:?}",
        findings
            .iter()
            .map(|f| f.message.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn locus003_still_fires_through_pipeline_for_legacy_diagnostics() {
    // Companion to the legacy-skip test above: through the full pipeline,
    // legacy diagnostics still produce LOCUS003 entries — proving that
    // LOCUS005's legacy-skip doesn't accidentally suppress LOCUS003.
    use locus_air::{AirFile, AirImport, AirItem, AirPackage, AirSpan, Visibility};
    let air = AirWorkspace::new(vec![AirPackage {
        name: "pkg".into(),
        version: "0.0.1".into(),
        root_dir: "/tmp/pkg".into(),
        files: vec![AirFile {
            path: "src/feature_a/handler.rs".into(),
            module_path: Some("pkg::feature_a::handler".into()),
            items: vec![AirItem::Import(AirImport {
                path: "pkg::feature_b::internal".into(),
                path_segments: Vec::new(),
                visibility: Visibility::Module,
                span: AirSpan::new("src/feature_a/handler.rs", 1, 1),
            })],
            hints: Vec::new(),
            parse_error: None,
            line_count: 5,
        }],
    }]);
    let lf = Lockfile::empty();
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: vec![
            "registry-integrity".into(),
            "registry-coherence".into(),
            "concept-source-of-truth".into(),
            "default-pass-through".into(),
        ],
        concepts: locus_standard_concepts(),
    });
    let out = governance::run_with_arch(&air, &lf, CheckMode::Human, &arch);

    // Any LOCUS005 finding should NOT reference legacy rule codes
    // (CX, OT, etc.) in its why[] — that would prove LOCUS005 leaked
    // into LOCUS003's territory.
    let locus005_for_legacy: Vec<_> = out
        .findings
        .iter()
        .filter(|f| f.diagnostic_code.as_deref() == Some("LOCUS005"))
        .filter(|f| {
            f.why
                .iter()
                .any(|w| w.contains("CX999") || w.contains("OT999"))
        })
        .collect();
    assert!(
        locus005_for_legacy.is_empty(),
        "LOCUS005 must not fire on legacy rule_codes; got {:?}",
        locus005_for_legacy
            .iter()
            .map(|f| f.message.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn unknown_concept_id_emits_one_locus005() {
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: Vec::new(),
        concepts: vec![ConceptDeclaration {
            id: "unknown-thing".into(),
            source_of_truth: "MysteryTrait".into(),
            registry: "MysteryRegistry".into(),
            enforcement: ConceptEnforcement::Advisory,
        }],
    });
    let findings = run_isolated(arch, |_minter, _store| {});
    let locus005: Vec<_> = findings
        .iter()
        .filter(|f| f.diagnostic_code.as_deref() == Some("LOCUS005"))
        .collect();
    assert_eq!(
        locus005.len(),
        1,
        "expected exactly one LOCUS005 for unknown concept id"
    );
    assert!(locus005[0].message.contains("unknown-thing"));
    assert!(
        locus005[0].suggested_fix.as_ref().unwrap().contains("rule"),
        "suggested_fix should list supported concept ids; got {:?}",
        locus005[0].suggested_fix
    );
}

#[test]
fn dedupes_repeated_bypass_observations() {
    // Three findings, same unregistered code → one LOCUS005.
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: Vec::new(),
        concepts: vec![governance_code_concept()],
    });
    let findings = run_isolated(arch, |minter, store| {
        for _ in 0..3 {
            store.insert(finding_with_diagnostic_code(
                minter,
                FindingSource::Policy(PolicyId::new("registry-integrity")),
                "LOCUS999",
            ));
        }
    });
    let locus005: Vec<_> = findings
        .iter()
        .filter(|f| f.diagnostic_code.as_deref() == Some("LOCUS005"))
        .collect();
    assert_eq!(locus005.len(), 1, "expected one dedupe'd LOCUS005");
}

#[test]
fn locus005_advisory_concept_stays_advisory_under_agent_strict() {
    // Even in --agent-strict mode, a concept declared with Advisory
    // enforcement (the default since #100) keeps LOCUS005 at Advisory
    // severity. Pure-guide-signal contract preserved.
    let air = AirWorkspace::new(Vec::new());
    let lf = Lockfile::empty();
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: vec![
            "registry-integrity".into(),
            "registry-coherence".into(),
            "concept-source-of-truth".into(),
            "default-pass-through".into(),
        ],
        concepts: vec![ConceptDeclaration {
            id: "unknown-thing".into(),
            source_of_truth: "X".into(),
            registry: "Y".into(),
            enforcement: ConceptEnforcement::Advisory,
        }],
    });
    let out = governance::run_with_arch(&air, &lf, CheckMode::AgentStrict, &arch);
    for d in out.diagnostics.iter().filter(|d| d.rule_id == "LOCUS005") {
        assert_eq!(
            d.severity,
            Severity::Advisory,
            "LOCUS005 must stay Advisory under --agent-strict for an Advisory concept; got {:?}",
            d.severity
        );
    }
}

#[test]
fn end_to_end_enforced_concept_under_agent_strict() {
    // Declare `rule` Enforced + inject a synthetic bypass (an
    // unregistered RuleId finding). Under --agent-strict the resulting
    // LOCUS005 must render at Fatal severity with an Active status —
    // the rendered diagnostic carries the decision's severity
    // verbatim (see `pipeline::materialize`), so pinning the decision
    // pins the user-facing output.
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: Vec::new(),
        concepts: vec![enforced_rule_concept()],
    });
    let out = run_isolated_with_mode(arch, CheckMode::AgentStrict, |minter, store| {
        store.insert(RuleFinding {
            id: minter.next(),
            source: FindingSource::RegisteredRule(RuleId::new("ZZ999")),
            rule_id: Some(RuleId::new("ZZ999")),
            paradigm_id: None,
            default_severity: Severity::Warning,
            span: None,
            concept: None,
            message: "m".into(),
            evidence: Vec::new(),
            why: Vec::new(),
            suggested_fix: None,
            diagnostic_code: None,
        });
    });
    let locus005: Vec<_> = out
        .new_findings
        .iter()
        .filter(|f| f.diagnostic_code.as_deref() == Some("LOCUS005"))
        .collect();
    assert_eq!(locus005.len(), 1, "expected one LOCUS005 for ZZ999 bypass");
    assert_eq!(
        locus005[0].default_severity,
        Severity::Fatal,
        "Enforced + AgentStrict must render as Fatal; got {:?}",
        locus005[0].default_severity
    );
    let decision = out
        .decisions
        .iter()
        .find(|d| d.finding_id == locus005[0].id)
        .expect("decision paired with finding");
    assert_eq!(decision.severity, Severity::Fatal);
    assert_eq!(
        decision.status,
        locus_core::governance::DecisionStatus::Active,
        "Enforced bypass should be Active, not Advisory"
    );
}

#[test]
fn end_to_end_advisory_concept_does_not_block() {
    // Mirror of the enforced test: declare `rule` Advisory (default)
    // + inject the same synthetic bypass. Even under --agent-strict,
    // the rendered LOCUS005 stays Advisory, so the run wouldn't
    // contribute to a non-zero error count.
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: Vec::new(),
        concepts: vec![rule_concept()], // Advisory enforcement
    });
    let out = run_isolated_with_mode(arch, CheckMode::AgentStrict, |minter, store| {
        store.insert(RuleFinding {
            id: minter.next(),
            source: FindingSource::RegisteredRule(RuleId::new("ZZ999")),
            rule_id: Some(RuleId::new("ZZ999")),
            paradigm_id: None,
            default_severity: Severity::Warning,
            span: None,
            concept: None,
            message: "m".into(),
            evidence: Vec::new(),
            why: Vec::new(),
            suggested_fix: None,
            diagnostic_code: None,
        });
    });
    let locus005: Vec<_> = out
        .new_findings
        .iter()
        .filter(|f| f.diagnostic_code.as_deref() == Some("LOCUS005"))
        .collect();
    assert_eq!(locus005.len(), 1);
    assert_eq!(locus005[0].default_severity, Severity::Advisory);
    let decision = out
        .decisions
        .iter()
        .find(|d| d.finding_id == locus005[0].id)
        .expect("decision paired with finding");
    assert_eq!(decision.severity, Severity::Advisory);
    assert_eq!(
        decision.status,
        locus_core::governance::DecisionStatus::Advisory,
    );
}

#[test]
fn end_to_end_pipeline_with_enforced_concept_stays_clean_for_valid_locus() {
    // Locus's own registries match all declared concepts, so even
    // under Enforced mode + AgentStrict the run should produce zero
    // LOCUS005 diagnostics. Guards against any accidental elevation
    // of false positives once the mechanism ships.
    let air = AirWorkspace::new(Vec::new());
    let lf = Lockfile::empty();
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: vec![
            "registry-integrity".into(),
            "registry-coherence".into(),
            "concept-source-of-truth".into(),
            "default-pass-through".into(),
        ],
        concepts: vec![
            enforced_rule_concept(),
            paradigm_concept(),
            policy_concept(),
            governance_code_concept(),
        ],
    });
    let out = governance::run_with_arch(&air, &lf, CheckMode::AgentStrict, &arch);
    let locus005: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "LOCUS005")
        .collect();
    assert!(
        locus005.is_empty(),
        "Locus's own valid registries must stay LOCUS005-silent even under Enforced+AgentStrict; got {:?}",
        locus005
            .iter()
            .map(|d| d.message.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn rule_concept_silent_for_registered_rule_finding() {
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: Vec::new(),
        concepts: vec![rule_concept()],
    });
    let findings = run_isolated(arch, |minter, store| {
        store.insert(RuleFinding {
            id: minter.next(),
            source: FindingSource::RegisteredRule(RuleId::new("CX001")),
            rule_id: Some(RuleId::new("CX001")),
            paradigm_id: Some(ParadigmId::new("CX")),
            default_severity: Severity::Warning,
            span: None,
            concept: None,
            message: "m".into(),
            evidence: Vec::new(),
            why: Vec::new(),
            suggested_fix: None,
            diagnostic_code: None,
        });
    });
    assert!(
        findings.is_empty(),
        "registered rule must not trigger LOCUS005; got {:?}",
        findings
            .iter()
            .map(|f| f.message.as_str())
            .collect::<Vec<_>>()
    );
}
