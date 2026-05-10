use super::*;
use locus_air::{
    AIR_SCHEMA_VERSION, AirField, AirFile, AirImplBlock, AirPackage, AirSpan, AirType,
};

fn ty(name: &str, vis: Visibility) -> AirItem {
    AirItem::Type(AirType {
        kind: TypeKind::Struct,
        name: name.into(),
        symbol: format!("x::tests::{name}"),
        visibility: vis,
        fields: Vec::new(),
        variants: Vec::new(),
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        span: AirSpan::new("t.rs", 1, 1),
        doc: None,
    })
}

fn air_with_module(module: &str, items: Vec<AirItem>) -> AirWorkspace {
    AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: "t.rs".into(),
                module_path: Some(module.into()),
                items,
                hints: Vec::new(),
                parse_error: None,
                line_count: 1,
            }],
        }],
        facts: Vec::new(),
    }
}

#[test]
fn ta001_fires_on_public_type_in_test_module() {
    let air = air_with_module("x::tests", vec![ty("User", Visibility::Public)]);
    let section = TaSection {
        test_paths: vec!["x::tests::*".into()],
        ..TaSection::default()
    };
    let diags = ta001(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "TA001");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("User"));
    assert!(diags[0].message.contains("x::tests"));
    assert!(diags[0].message.contains("x::tests::*"));
}

#[test]
fn ta001_quiet_on_private_type_in_test_module() {
    let air = air_with_module("x::tests", vec![ty("Fixture", Visibility::Private)]);
    let section = TaSection {
        test_paths: vec!["x::tests::*".into()],
        ..TaSection::default()
    };
    assert!(ta001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ta001_quiet_on_public_type_in_non_matching_module() {
    let air = air_with_module("x::domain::user", vec![ty("User", Visibility::Public)]);
    let section = TaSection {
        test_paths: vec!["x::tests::*".into()],
        ..TaSection::default()
    };
    assert!(ta001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ta001_silent_when_test_paths_empty() {
    let air = air_with_module("x::tests", vec![ty("User", Visibility::Public)]);
    let section = TaSection::default();
    assert!(ta001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ta001_multiple_public_types_produce_multiple_diagnostics() {
    let air = air_with_module(
        "x::tests",
        vec![
            ty("User", Visibility::Public),
            ty("Order", Visibility::Public),
            ty("Internal", Visibility::Private), // not flagged
            ty("Account", Visibility::Public),
        ],
    );
    let section = TaSection {
        test_paths: vec!["x::tests::*".into()],
        ..TaSection::default()
    };
    let diags = ta001(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 3);
    let names: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert!(names.iter().any(|m| m.contains("User")));
    assert!(names.iter().any(|m| m.contains("Order")));
    assert!(names.iter().any(|m| m.contains("Account")));
    assert!(!names.iter().any(|m| m.contains("Internal")));
}

#[test]
fn ta001_agent_strict_elevates_to_fatal() {
    let air = air_with_module("x::tests", vec![ty("User", Visibility::Public)]);
    let section = TaSection {
        test_paths: vec!["x::tests::*".into()],
        ..TaSection::default()
    };
    let diags = ta001(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

fn struct_with_fields(name: &str, fields: &[&str]) -> AirItem {
    AirItem::Type(AirType {
        kind: TypeKind::Struct,
        name: name.into(),
        symbol: format!("x::tests::{name}"),
        visibility: Visibility::Private,
        fields: fields
            .iter()
            .map(|n| AirField {
                name: (*n).into(),
                type_text: "()".into(),
                visibility: Visibility::Private,
            })
            .collect(),
        variants: Vec::new(),
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        span: AirSpan::new("t.rs", 1, 1),
        doc: None,
    })
}

fn impl_item(trait_path: Option<&str>, self_ty: &str) -> AirItem {
    AirItem::Impl(AirImplBlock {
        interface: trait_path.map(|s| s.to_string()),
        target_type: self_ty.into(),
        method_names: Vec::new(),
        dispatch: locus_air::ImplDispatch::Static,
        span: AirSpan::new("t.rs", 1, 1),
    })
}

// ─── TA002 ───────────────────────────────────────────────────────────

#[test]
fn ta002_fires_on_test_type_with_canonical_name() {
    let air = air_with_module(
        "x::tests::user",
        vec![
            ty("User", Visibility::Private),
            ty("Helper", Visibility::Private),
        ],
    );
    let section = TaSection {
        test_paths: vec!["x::tests::*".into()],
        canonical_name_patterns: vec!["User".into()],
        ..TaSection::default()
    };
    let diags = ta002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "TA002");
    assert!(diags[0].message.contains("User"));
    assert!(diags[0].message.contains("x::tests::user"));
}

#[test]
fn ta002_silent_when_canonical_name_patterns_empty() {
    let air = air_with_module("x::tests", vec![ty("User", Visibility::Public)]);
    let section = TaSection {
        test_paths: vec!["x::tests::*".into()],
        ..TaSection::default()
    };
    assert!(ta002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ta002_quiet_outside_test_paths() {
    let air = air_with_module("x::domain::user", vec![ty("User", Visibility::Public)]);
    let section = TaSection {
        test_paths: vec!["x::tests::*".into()],
        canonical_name_patterns: vec!["User".into()],
        ..TaSection::default()
    };
    assert!(ta002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ta002_wildcard_name_pattern_matches() {
    let air = air_with_module(
        "x::tests",
        vec![
            ty("OrderDto", Visibility::Private),
            ty("Misc", Visibility::Private),
        ],
    );
    let section = TaSection {
        test_paths: vec!["x::tests::*".into()],
        canonical_name_patterns: vec!["Order*".into()],
        ..TaSection::default()
    };
    let diags = ta002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("OrderDto"));
}

#[test]
fn ta002_agent_strict_elevates_to_fatal() {
    let air = air_with_module("x::tests", vec![ty("User", Visibility::Private)]);
    let section = TaSection {
        test_paths: vec!["x::tests::*".into()],
        canonical_name_patterns: vec!["User".into()],
        ..TaSection::default()
    };
    let diags = ta002(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ─── TA003 ───────────────────────────────────────────────────────────

#[test]
fn ta003_fires_on_shape_shadow() {
    // TestUser carries the canonical User's field set verbatim.
    let air = air_with_module(
        "x::tests",
        vec![struct_with_fields("TestUser", &["id", "email", "name"])],
    );
    let section = TaSection {
        test_paths: vec!["x::tests::*".into()],
        canonical_name_patterns: vec!["User".into()],
        canonical_field_sets: vec![vec!["id".into(), "email".into(), "name".into()]],
        ..TaSection::default()
    };
    let diags = ta003(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "TA003");
    assert!(diags[0].message.contains("TestUser"));
}

#[test]
fn ta003_quiet_when_field_overlap_below_threshold() {
    // Only 1 field shared out of a union of 5 → Jaccard 0.2 < 0.5.
    let air = air_with_module(
        "x::tests",
        vec![struct_with_fields(
            "UserFixture",
            &["id", "tag", "score", "color"],
        )],
    );
    let section = TaSection {
        test_paths: vec!["x::tests::*".into()],
        canonical_name_patterns: vec!["User".into()],
        canonical_field_sets: vec![vec!["id".into(), "email".into()]],
        ..TaSection::default()
    };
    assert!(ta003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ta003_quiet_when_name_does_not_overlap() {
    // Field set matches canonical, but type name doesn't echo the
    // canonical concept — TA003 needs both gates.
    let air = air_with_module(
        "x::tests",
        vec![struct_with_fields("Widget", &["id", "email", "name"])],
    );
    let section = TaSection {
        test_paths: vec!["x::tests::*".into()],
        canonical_name_patterns: vec!["User".into()],
        canonical_field_sets: vec![vec!["id".into(), "email".into(), "name".into()]],
        ..TaSection::default()
    };
    assert!(ta003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ta003_silent_when_canonical_field_sets_empty() {
    let air = air_with_module(
        "x::tests",
        vec![struct_with_fields("TestUser", &["id", "email", "name"])],
    );
    let section = TaSection {
        test_paths: vec!["x::tests::*".into()],
        canonical_name_patterns: vec!["User".into()],
        ..TaSection::default()
    };
    assert!(ta003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ta003_silent_when_test_paths_empty() {
    let air = air_with_module(
        "x::tests",
        vec![struct_with_fields("TestUser", &["id", "email", "name"])],
    );
    let section = TaSection {
        canonical_name_patterns: vec!["User".into()],
        canonical_field_sets: vec![vec!["id".into(), "email".into()]],
        ..TaSection::default()
    };
    assert!(ta003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ta003_agent_strict_elevates_to_fatal() {
    let air = air_with_module(
        "x::tests",
        vec![struct_with_fields("TestUser", &["id", "email", "name"])],
    );
    let section = TaSection {
        test_paths: vec!["x::tests::*".into()],
        canonical_name_patterns: vec!["User".into()],
        canonical_field_sets: vec![vec!["id".into(), "email".into(), "name".into()]],
        ..TaSection::default()
    };
    let diags = ta003(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ─── TA004 ───────────────────────────────────────────────────────────

#[test]
fn ta004_fires_on_port_impl_in_test_module() {
    let air = air_with_module(
        "x::tests::auth",
        vec![impl_item(Some("x::ports::UserRepository"), "FakeRepo")],
    );
    let section = TaSection {
        test_paths: vec!["x::tests::*".into()],
        port_trait_patterns: vec!["*Repository".into()],
        ..TaSection::default()
    };
    let diags = ta004(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "TA004");
    assert!(diags[0].message.contains("UserRepository"));
    assert!(diags[0].message.contains("FakeRepo"));
}

#[test]
fn ta004_quiet_in_accepted_test_adapter_path() {
    let air = air_with_module(
        "x::tests::support::repos",
        vec![impl_item(Some("x::ports::UserRepository"), "InMemoryRepo")],
    );
    let section = TaSection {
        test_paths: vec!["x::tests::*".into()],
        port_trait_patterns: vec!["*Repository".into()],
        accepted_test_adapter_paths: vec!["x::tests::support::*".into()],
        ..TaSection::default()
    };
    assert!(ta004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ta004_quiet_for_inherent_impl() {
    // No trait_path → not a port impl, never flagged.
    let air = air_with_module("x::tests", vec![impl_item(None, "FakeRepo")]);
    let section = TaSection {
        test_paths: vec!["x::tests::*".into()],
        port_trait_patterns: vec!["*Repository".into()],
        ..TaSection::default()
    };
    assert!(ta004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ta004_silent_when_port_trait_patterns_empty() {
    let air = air_with_module(
        "x::tests",
        vec![impl_item(Some("x::ports::UserRepository"), "FakeRepo")],
    );
    let section = TaSection {
        test_paths: vec!["x::tests::*".into()],
        ..TaSection::default()
    };
    assert!(ta004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ta004_quiet_outside_test_paths() {
    let air = air_with_module(
        "x::infrastructure::repos",
        vec![impl_item(Some("x::ports::UserRepository"), "PgRepo")],
    );
    let section = TaSection {
        test_paths: vec!["x::tests::*".into()],
        port_trait_patterns: vec!["*Repository".into()],
        ..TaSection::default()
    };
    assert!(ta004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ta004_agent_strict_elevates_to_fatal() {
    let air = air_with_module(
        "x::tests",
        vec![impl_item(Some("x::ports::UserGateway"), "FakeGw")],
    );
    let section = TaSection {
        test_paths: vec!["x::tests::*".into()],
        port_trait_patterns: vec!["*Gateway".into()],
        ..TaSection::default()
    };
    let diags = ta004(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}
