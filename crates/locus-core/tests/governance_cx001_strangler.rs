//! Verifies the strangler invariant: after CX001 migrates to
//! `RuleDefinition`, every CX001 finding in the governance pipeline comes
//! from `FindingSource::RegisteredRule`, NOT from
//! `FindingSource::LegacyDiagnostic`. The per-diagnostic-code filter in
//! `LegacyParadigmRuleAdapter` is exercised on real data for the first
//! time.

use locus_air::{
    AIR_SCHEMA_VERSION, AirFile, AirFunction, AirItem, AirPackage, AirSpan, AirWorkspace,
    Visibility,
};
use locus_core::CheckMode;
use locus_core::governance::{self, FindingSource, RuleId};
use locus_core::lockfile::Lockfile;

#[test]
fn cx001_findings_come_from_registered_rule_not_legacy_adapter() {
    // Workspace with one overlong function — guaranteed CX001 trigger.
    let air = AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "demo".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: "src/big.rs".into(),
                module_path: Some("demo::module".into()),
                items: vec![AirItem::Function(AirFunction {
                    name: "big_fn".into(),
                    symbol: "demo::module::big_fn".into(),
                    symbol_segments: Vec::new(),
                    visibility: Visibility::Public,
                    params: Vec::new(),
                    return_type: None,
                    span: AirSpan::new("src/big.rs", 1, 200),
                    line_count: 200,
                    doc: None,
                    decorators: Vec::new(),
                })],
                hints: Vec::new(),
                parse_error: None,
                line_count: 205,
            }],
        }],
        facts: Vec::new(),
    };
    let lf = Lockfile::default();

    let out = governance::run(&air, &lf, CheckMode::Human);

    // Pull every finding referencing CX001 from the store, whether by
    // rule_id or by the legacy adapter's rule_code.
    let cx001_findings: Vec<_> = out
        .findings
        .iter()
        .filter(|f| {
            matches!(&f.rule_id, Some(r) if *r == RuleId::new("CX001"))
                || matches!(
                    &f.source,
                    FindingSource::LegacyDiagnostic { rule_code, .. } if rule_code == "CX001"
                )
        })
        .collect();

    assert_eq!(
        cx001_findings.len(),
        1,
        "expected exactly one CX001 finding (no double-fire), got {} findings: {:?}",
        cx001_findings.len(),
        cx001_findings
    );

    // The single CX001 finding MUST come from the registered rule, not the
    // legacy adapter — proves the per-diagnostic-code filter works.
    match &cx001_findings[0].source {
        FindingSource::RegisteredRule(r) => {
            assert_eq!(r.as_str(), "CX001");
        }
        FindingSource::LegacyDiagnostic { rule_code, .. } => {
            panic!(
                "CX001 finding still flows through legacy adapter (rule_code={rule_code}); \
                 strangler filter is not working"
            );
        }
        other => panic!("unexpected source for CX001 finding: {other:?}"),
    }
}
