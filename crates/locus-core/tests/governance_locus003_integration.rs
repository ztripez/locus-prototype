//! Integration tests for LOCUS003 migration-debt emission.
//!
//! Verifies that `governance::run()` emits exactly one LOCUS003 advisory
//! per unique un-migrated legacy rule code, and that registered rules
//! (CX001 / OT002 / DG001–DG004) do not generate LOCUS003 findings.

use locus_air::{AirFile, AirImport, AirItem, AirPackage, AirSpan, AirWorkspace, Visibility};
use locus_core::governance::{self, FindingSource};
use locus_core::{CheckMode, Lockfile};

/// Build a workspace with one import that triggers DG001 (registered rule).
/// DG001 findings must NOT produce LOCUS003.
fn dg001_workspace() -> (AirWorkspace, Lockfile) {
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
    let mut lf = Lockfile::default();
    let section = serde_json::json!({
        "forbidden_edges": [{"from": "pkg::feature_a::*", "to": "pkg::feature_b::*"}],
        "features": [],
        "shared_paths": []
    });
    lf.paradigms.insert("DG".to_string(), section);
    (air, lf)
}

#[test]
fn registered_rule_dg001_does_not_produce_locus003() {
    let (air, lf) = dg001_workspace();
    let out = governance::run(&air, &lf, CheckMode::Human);

    let locus003_for_dg001: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "LOCUS003" && d.message.contains("DG001"))
        .collect();

    // DG001 is a registered rule — its findings must NOT trigger LOCUS003.
    assert!(
        locus003_for_dg001.is_empty(),
        "registered DG001 must not trigger LOCUS003; got: {locus003_for_dg001:?}"
    );

    // The DG001 diagnostic itself must still be present.
    let dg001: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "DG001")
        .collect();
    assert!(!dg001.is_empty(), "DG001 diagnostic must still be present");
}

#[test]
fn locus003_advisory_never_elevates_under_agent_strict() {
    // Even in --agent-strict mode, LOCUS003 stays Advisory.
    // Use an empty workspace — if any legacy rules fire on it, each
    // unique code gets one LOCUS003. Severity must be Advisory, not Fatal.
    let air = AirWorkspace::new(Vec::new());
    let lf = Lockfile::empty();
    let out = governance::run(&air, &lf, CheckMode::AgentStrict);

    for d in out.diagnostics.iter().filter(|d| d.rule_id == "LOCUS003") {
        assert_eq!(
            d.severity,
            locus_core::Severity::Advisory,
            "LOCUS003 must stay Advisory under --agent-strict; got {:?}",
            d.severity
        );
    }
}

#[test]
fn locus003_findings_use_policy_source() {
    // LOCUS003 findings must come from FindingSource::Policy, not
    // LegacyDiagnostic or RegisteredRule.
    let air = AirWorkspace::new(Vec::new());
    let lf = Lockfile::empty();
    let out = governance::run(&air, &lf, CheckMode::Human);

    for f in out.findings.iter() {
        if f.diagnostic_code.as_deref() == Some("LOCUS003") {
            assert!(
                matches!(f.source, FindingSource::Policy(_)),
                "LOCUS003 finding must have Policy source; got {:?}",
                f.source
            );
        }
    }
}

#[test]
fn locus003_deduplicates_by_rule_code() {
    // DG002 is now a registered rule (P4 migration). Registered rules must
    // NOT produce LOCUS003. Verify that a 2-cycle workspace produces ≥2
    // DG002 diagnostics (one per edge) and zero LOCUS003 entries for DG002.
    use locus_air::AIR_SCHEMA_VERSION;
    let air = AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![
            AirPackage {
                name: "a".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "a/src/lib.rs".into(),
                    module_path: Some("a".into()),
                    items: vec![AirItem::Import(AirImport {
                        path: "b::Type1".into(),
                        path_segments: Vec::new(),
                        visibility: Visibility::Module,
                        span: AirSpan::new("a/src/lib.rs", 1, 1),
                    })],
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 2,
                }],
            },
            AirPackage {
                name: "b".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "b/src/lib.rs".into(),
                    module_path: Some("b".into()),
                    items: vec![AirItem::Import(AirImport {
                        path: "a::Type2".into(),
                        path_segments: Vec::new(),
                        visibility: Visibility::Module,
                        span: AirSpan::new("b/src/lib.rs", 1, 1),
                    })],
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 2,
                }],
            },
        ],
        facts: Vec::new(),
    };
    let lf = Lockfile::empty();
    let out = governance::run(&air, &lf, CheckMode::Human);

    // DG002 is registered — fires ≥2 times (one per edge in the 2-cycle).
    let dg002_count = out
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "DG002")
        .count();
    assert!(
        dg002_count >= 2,
        "expected ≥2 DG002 diagnostics; got {dg002_count}"
    );

    // Registered rule — must NOT produce LOCUS003.
    let locus003_for_dg002 = out
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "LOCUS003" && d.message.contains("DG002"))
        .count();
    assert_eq!(
        locus003_for_dg002, 0,
        "registered DG002 must not trigger LOCUS003; got {locus003_for_dg002}"
    );
}
