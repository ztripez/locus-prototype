//! Verifies the strangler invariant for DG001: every DG001 finding from
//! the governance pipeline comes from `FindingSource::RegisteredRule`,
//! NOT from `FindingSource::LegacyDiagnostic`. The per-diagnostic-code
//! filter in `LegacyParadigmRuleAdapter` is now exercised on a third
//! rule code (CX001 + OT002 + DG001).

use locus_air::{AirFile, AirImport, AirItem, AirPackage, AirSpan, AirWorkspace, Visibility};
use locus_core::CheckMode;
use locus_core::governance::{self, FindingSource, RuleId};
use locus_core::lockfile::Lockfile;

#[test]
fn dg001_findings_come_from_registered_rule_not_legacy_adapter() {
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
        "forbidden_edges": [
            {
                "from": "pkg::feature_a::*",
                "to": "pkg::feature_b::*",
                "reason": "feature isolation"
            }
        ],
        "features": [],
        "shared_paths": []
    });
    lf.paradigms.insert("DG".to_string(), section);

    let out = governance::run(&air, &lf, CheckMode::Human);

    let dg001_findings: Vec<_> = out
        .findings
        .iter()
        .filter(|f| {
            matches!(&f.rule_id, Some(r) if *r == RuleId::new("DG001"))
                || matches!(
                    &f.source,
                    FindingSource::LegacyDiagnostic { rule_code, .. } if rule_code == "DG001"
                )
        })
        .collect();

    assert_eq!(
        dg001_findings.len(),
        1,
        "expected exactly one DG001 finding (no double-fire), got {} findings: {:?}",
        dg001_findings.len(),
        dg001_findings
    );

    match &dg001_findings[0].source {
        FindingSource::RegisteredRule(r) => {
            assert_eq!(r.as_str(), "DG001");
        }
        FindingSource::LegacyDiagnostic { rule_code, .. } => {
            panic!(
                "DG001 finding still flows through legacy adapter (rule_code={rule_code}); \
                 strangler filter is not working"
            );
        }
        other => panic!("unexpected source for DG001 finding: {other:?}"),
    }
}
