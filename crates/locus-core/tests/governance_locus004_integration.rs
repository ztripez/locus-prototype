//! Integration tests for LOCUS004 architecture-coherence emission.
//!
//! Verifies that `governance::run_with_arch()` end-to-end emits
//! `LOCUS004` advisories for declared/registered drift and stays silent
//! when `.locus/arch.json` declares exactly the registered policy set.

use locus_air::AirWorkspace;
use locus_core::governance::{self, ArchDeclaration, ArchLoadOutcome};
use locus_core::{CheckMode, Lockfile};

#[test]
fn missing_arch_declaration_emits_one_locus004() {
    let air = AirWorkspace::new(Vec::new());
    let lf = Lockfile::empty();
    let out = governance::run_with_arch(&air, &lf, CheckMode::Human, &ArchLoadOutcome::Missing);

    let locus004: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "LOCUS004")
        .collect();
    assert_eq!(
        locus004.len(),
        1,
        "missing arch.json should emit exactly one LOCUS004; got {locus004:?}"
    );
    assert!(
        locus004[0].message.contains(".locus/arch.json"),
        "LOCUS004 message should reference .locus/arch.json; got `{}`",
        locus004[0].message
    );
    assert_eq!(
        locus004[0].severity,
        locus_core::Severity::Advisory,
        "LOCUS004 must be Advisory"
    );
}

#[test]
fn invalid_arch_declaration_emits_locus004_with_parse_error_in_why() {
    let air = AirWorkspace::new(Vec::new());
    let lf = Lockfile::empty();
    let out = governance::run_with_arch(
        &air,
        &lf,
        CheckMode::Human,
        &ArchLoadOutcome::Invalid("trailing comma at line 3".into()),
    );

    let locus004: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "LOCUS004")
        .collect();
    assert_eq!(locus004.len(), 1);
    assert!(
        locus004[0]
            .why
            .iter()
            .any(|w| w.contains("trailing comma at line 3")),
        "parse error must appear in why[]; got {:?}",
        locus004[0].why
    );
}

#[test]
fn arch_matching_standard_registry_is_silent() {
    // `.locus/arch.json` declares exactly the policies in
    // `PolicyRegistry::standard()` — no drift, no LOCUS004.
    let air = AirWorkspace::new(Vec::new());
    let lf = Lockfile::empty();
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: vec![
            "registry-integrity".into(),
            "registry-coherence".into(),
            "concept-source-of-truth".into(),
            "default-pass-through".into(),
        ],
        concepts: Vec::new(),
    });
    let out = governance::run_with_arch(&air, &lf, CheckMode::Human, &arch);
    let locus004: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "LOCUS004")
        .collect();
    assert!(
        locus004.is_empty(),
        "matched declaration should be silent; got {locus004:?}"
    );
}

#[test]
fn extra_declared_policy_produces_locus004_drift() {
    let air = AirWorkspace::new(Vec::new());
    let lf = Lockfile::empty();
    let arch = ArchLoadOutcome::Present(ArchDeclaration {
        policies: vec![
            "registry-integrity".into(),
            "registry-coherence".into(),
            "concept-source-of-truth".into(),
            "default-pass-through".into(),
            "ghost-policy".into(),
        ],
        concepts: Vec::new(),
    });
    let out = governance::run_with_arch(&air, &lf, CheckMode::Human, &arch);
    let drift: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| {
            d.rule_id == "LOCUS004"
                && d.message.contains("ghost-policy")
                && d.message.contains("not registered")
        })
        .collect();
    assert_eq!(
        drift.len(),
        1,
        "expected exactly one LOCUS004 for ghost-policy; got {:?}",
        out.diagnostics
            .iter()
            .filter(|d| d.rule_id == "LOCUS004")
            .map(|d| d.message.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn locus004_always_advisory_under_agent_strict() {
    let air = AirWorkspace::new(Vec::new());
    let lf = Lockfile::empty();
    let out =
        governance::run_with_arch(&air, &lf, CheckMode::AgentStrict, &ArchLoadOutcome::Missing);
    for d in out.diagnostics.iter().filter(|d| d.rule_id == "LOCUS004") {
        assert_eq!(
            d.severity,
            locus_core::Severity::Advisory,
            "LOCUS004 must stay Advisory under --agent-strict; got {:?}",
            d.severity
        );
    }
}

#[test]
fn run_without_arch_defaults_to_missing_outcome() {
    // The legacy `run` overload exists for backward compatibility and
    // treats arch as Missing — so it produces the same single LOCUS004.
    let air = AirWorkspace::new(Vec::new());
    let lf = Lockfile::empty();
    let out_default = governance::run(&air, &lf, CheckMode::Human);
    let out_explicit =
        governance::run_with_arch(&air, &lf, CheckMode::Human, &ArchLoadOutcome::Missing);
    let count_default = out_default
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "LOCUS004")
        .count();
    let count_explicit = out_explicit
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "LOCUS004")
        .count();
    assert_eq!(count_default, count_explicit);
    assert_eq!(count_default, 1, "missing arch should produce 1 LOCUS004");
}
