//! Verifies the strangler invariant for OT002: every OT002 finding from
//! the governance pipeline comes from `FindingSource::RegisteredRule`,
//! NOT from `FindingSource::LegacyDiagnostic`. The per-diagnostic-code
//! filter in `LegacyParadigmRuleAdapter` is now exercised on a second
//! rule code (CX001 + OT002).

use locus_air::{
    AirField, AirFile, AirHint, AirItem, AirPackage, AirSpan, AirType, AirWorkspace, HintKind,
    TypeKind, Visibility,
};
use locus_core::CheckMode;
use locus_core::governance::{self, FindingSource, RuleId};
use locus_core::lockfile::Lockfile;

#[test]
fn ot002_findings_come_from_registered_rule_not_legacy_adapter() {
    let canonical = AirType {
        kind: TypeKind::Struct,
        name: "User".into(),
        symbol: "demo::user::User".into(),
        symbol_segments: Vec::new(),
        visibility: Visibility::Public,
        fields: vec![
            AirField {
                name: "id".into(),
                type_text: "u32".into(),
                visibility: Visibility::Public,
            },
            AirField {
                name: "name".into(),
                type_text: "String".into(),
                visibility: Visibility::Public,
            },
        ],
        variants: Vec::new(),
        decorators: Vec::new(),
        span: AirSpan::new("src/user.rs", 5, 8),
        doc: None,
    };
    let sibling = AirType {
        kind: TypeKind::Struct,
        name: "UserResponse".into(),
        symbol: "demo::user::UserResponse".into(),
        symbol_segments: Vec::new(),
        visibility: Visibility::Public,
        fields: vec![
            AirField {
                name: "id".into(),
                type_text: "u32".into(),
                visibility: Visibility::Public,
            },
            AirField {
                name: "name".into(),
                type_text: "String".into(),
                visibility: Visibility::Public,
            },
        ],
        variants: Vec::new(),
        decorators: Vec::new(),
        span: AirSpan::new("src/user.rs", 12, 15),
        doc: None,
    };
    let hint = AirHint {
        kind: HintKind::Canonical,
        raw: "// locus: ot canonical".into(),
        span: AirSpan::new("src/user.rs", 4, 4),
        target_span: Some(AirSpan::new("src/user.rs", 5, 5)),
    };
    let air = AirWorkspace::new(vec![AirPackage {
        name: "demo".into(),
        version: "0.0.1".into(),
        root_dir: "/tmp/demo".into(),
        files: vec![AirFile {
            path: "src/user.rs".into(),
            module_path: Some("demo::user".into()),
            items: vec![AirItem::Type(canonical), AirItem::Type(sibling)],
            hints: vec![hint],
            parse_error: None,
            line_count: 20,
        }],
    }]);
    let lf = Lockfile::default();

    let out = governance::run(&air, &lf, CheckMode::Human);

    let ot002_findings: Vec<_> = out
        .findings
        .iter()
        .filter(|f| {
            matches!(&f.rule_id, Some(r) if *r == RuleId::new("OT002"))
                || matches!(
                    &f.source,
                    FindingSource::LegacyDiagnostic { rule_code, .. } if rule_code == "OT002"
                )
        })
        .collect();

    assert_eq!(
        ot002_findings.len(),
        1,
        "expected exactly one OT002 finding (no double-fire), got {} findings: {:?}",
        ot002_findings.len(),
        ot002_findings
    );

    match &ot002_findings[0].source {
        FindingSource::RegisteredRule(r) => {
            assert_eq!(r.as_str(), "OT002");
        }
        FindingSource::LegacyDiagnostic { rule_code, .. } => {
            panic!(
                "OT002 finding still flows through legacy adapter (rule_code={rule_code}); \
                 strangler filter is not working"
            );
        }
        other => panic!("unexpected source for OT002 finding: {other:?}"),
    }
}
