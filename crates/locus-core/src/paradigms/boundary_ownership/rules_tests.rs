use super::*;
use locus_air::{
    AIR_SCHEMA_VERSION, AirFact, AirFile, AirImport, AirPackage, AirSpan, FactKind, FactTarget,
    Visibility,
};

fn import(path: &str) -> AirItem {
    AirItem::Import(AirImport {
        path: path.into(),
        path_segments: Vec::new(),
        visibility: Visibility::Private,
        span: AirSpan::new("t.rs", 1, 1),
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
fn bo001_fires_when_domain_file_imports_forbidden_path() {
    let air = air_with_module("crate::domain::user", vec![import("sqlx::Pool")]);
    let section = BoSection {
        domain_paths: vec!["crate::domain::*".into()],
        forbidden_in_domain: vec!["sqlx::*".into()],
        ..Default::default()
    };
    let diags = bo001(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "BO001");
    assert_eq!(diags[0].severity, Severity::Fatal);
    assert!(diags[0].message.contains("crate::domain::user"));
    assert!(diags[0].message.contains("sqlx::Pool"));
    assert!(
        diags[0].why.iter().any(|w| w.contains("crate::domain::*")),
        "expected domain pattern in why; got {:?}",
        diags[0].why
    );
    assert!(
        diags[0].why.iter().any(|w| w.contains("sqlx::*")),
        "expected forbidden pattern in why; got {:?}",
        diags[0].why
    );
}

#[test]
fn bo001_quiet_when_non_domain_file_imports_forbidden_path() {
    // Adapter/infra layer is allowed to use sqlx — that's the whole point
    // of putting persistence at the boundary.
    let air = air_with_module("crate::infra::user_repo", vec![import("sqlx::Pool")]);
    let section = BoSection {
        domain_paths: vec!["crate::domain::*".into()],
        forbidden_in_domain: vec!["sqlx::*".into()],
        ..Default::default()
    };
    assert!(bo001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn bo001_quiet_when_domain_file_imports_non_forbidden_path() {
    let air = air_with_module(
        "crate::domain::user",
        vec![import("crate::domain::value::Email")],
    );
    let section = BoSection {
        domain_paths: vec!["crate::domain::*".into()],
        forbidden_in_domain: vec!["sqlx::*".into()],
        ..Default::default()
    };
    assert!(bo001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn bo001_silent_when_domain_paths_empty() {
    let air = air_with_module("crate::domain::user", vec![import("sqlx::Pool")]);
    let section = BoSection {
        domain_paths: vec![],
        forbidden_in_domain: vec!["sqlx::*".into()],
        ..Default::default()
    };
    assert!(bo001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn bo001_silent_when_forbidden_in_domain_empty() {
    let air = air_with_module("crate::domain::user", vec![import("sqlx::Pool")]);
    let section = BoSection {
        domain_paths: vec!["crate::domain::*".into()],
        forbidden_in_domain: vec![],
        ..Default::default()
    };
    assert!(bo001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn bo001_silent_with_default_section() {
    let air = air_with_module("crate::domain::user", vec![import("sqlx::Pool")]);
    let section = BoSection::default();
    assert!(bo001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn bo001_agent_strict_keeps_severity_fatal() {
    // BO001 is already Fatal in human mode; --agent-strict elevates but
    // can't go higher than Fatal — verify it stays Fatal, not panicked.
    let air = air_with_module("crate::domain::user", vec![import("reqwest::Client")]);
    let section = BoSection {
        domain_paths: vec!["crate::domain::*".into()],
        forbidden_in_domain: vec!["reqwest::*".into()],
        ..Default::default()
    };
    let diags = bo001(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ----- BO002 -----

fn function_item(
    name: &str,
    symbol: &str,
    params: Vec<(&str, &str)>,
    return_type: Option<&str>,
) -> AirItem {
    use locus_air::AirFunction;
    AirItem::Function(AirFunction {
        name: name.into(),
        symbol: symbol.into(),
        visibility: Visibility::Public,
        params: params
            .into_iter()
            .map(|(n, t)| (n.to_string(), t.to_string()))
            .collect(),
        return_type: return_type.map(|s| s.to_string()),
        span: AirSpan::new("t.rs", 1, 1),
        line_count: 1,
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        doc: None,
    })
}

#[test]
fn bo002_fires_on_persistence_param_in_domain_function() {
    let air = air_with_module(
        "crate::domain::user",
        vec![function_item(
            "load",
            "x::domain::user::load",
            vec![("row", "sqlx::PgRow")],
            None,
        )],
    );
    let section = BoSection {
        domain_paths: vec!["crate::domain::*".into()],
        persistence_type_patterns: vec!["sqlx::*".into()],
        ..Default::default()
    };
    let diags = bo002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "BO002");
    assert_eq!(diags[0].severity, Severity::Fatal);
    assert!(diags[0].message.contains("sqlx::PgRow"));
    assert!(diags[0].message.contains("parameter `row`"));
    assert!(
        diags[0].why.iter().any(|w| w.contains("crate::domain::*")),
        "expected domain pattern in why; got {:?}",
        diags[0].why
    );
}

#[test]
fn bo002_fires_on_persistence_return_type() {
    let air = air_with_module(
        "crate::domain::user",
        vec![function_item(
            "fetch",
            "x::domain::user::fetch",
            vec![],
            Some("Result<diesel::result::QueryResult, diesel::result::Error>"),
        )],
    );
    let section = BoSection {
        domain_paths: vec!["crate::domain::*".into()],
        persistence_type_patterns: vec!["diesel::*".into()],
        ..Default::default()
    };
    let diags = bo002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("return type"));
}

#[test]
fn bo002_quiet_in_non_domain_module() {
    // Adapter/infra layer is allowed to expose persistence types.
    let air = air_with_module(
        "crate::infra::user_repo",
        vec![function_item(
            "load",
            "x::infra::user_repo::load",
            vec![("row", "sqlx::PgRow")],
            None,
        )],
    );
    let section = BoSection {
        domain_paths: vec!["crate::domain::*".into()],
        persistence_type_patterns: vec!["sqlx::*".into()],
        ..Default::default()
    };
    assert!(bo002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn bo002_silent_when_persistence_patterns_empty() {
    let air = air_with_module(
        "crate::domain::user",
        vec![function_item(
            "load",
            "x::domain::user::load",
            vec![("row", "sqlx::PgRow")],
            None,
        )],
    );
    let section = BoSection {
        domain_paths: vec!["crate::domain::*".into()],
        persistence_type_patterns: vec![],
        ..Default::default()
    };
    assert!(bo002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn bo002_quiet_when_signature_uses_only_domain_types() {
    let air = air_with_module(
        "crate::domain::user",
        vec![function_item(
            "rename",
            "x::domain::user::rename",
            vec![("user", "User"), ("name", "&str")],
            Some("Result<User, DomainError>"),
        )],
    );
    let section = BoSection {
        domain_paths: vec!["crate::domain::*".into()],
        persistence_type_patterns: vec!["sqlx::*".into(), "diesel::*".into()],
        ..Default::default()
    };
    assert!(bo002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn bo002_agent_strict_stays_fatal() {
    let air = air_with_module(
        "crate::domain::user",
        vec![function_item(
            "load",
            "x::domain::user::load",
            vec![("row", "sea_orm::ActiveModel")],
            None,
        )],
    );
    let section = BoSection {
        domain_paths: vec!["crate::domain::*".into()],
        persistence_type_patterns: vec!["sea_orm::*".into()],
        ..Default::default()
    };
    let diags = bo002(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ----- BO004 -----

fn type_with_derives(name: &str, symbol: &str, derives: Vec<&str>) -> AirItem {
    use locus_air::{AirDecorator, AirType, DecoratorSource, TypeKind};
    AirItem::Type(AirType {
        kind: TypeKind::Struct,
        name: name.into(),
        symbol: symbol.into(),
        symbol_segments: Vec::new(),
        visibility: Visibility::Public,
        fields: Vec::new(),
        variants: Vec::new(),
        decorators: derives
            .into_iter()
            .map(|s| AirDecorator {
                source: DecoratorSource::Derive,
                name: s.to_string(),
                args: Vec::new(),
            })
            .collect(),
        span: AirSpan::new("t.rs", 1, 1),
        doc: None,
    })
}

#[test]
fn bo004_fires_on_serialize_in_canonical_module() {
    let air = air_with_module(
        "crate::domain::user",
        vec![type_with_derives(
            "User",
            "x::domain::user::User",
            vec!["Debug", "Clone", "Serialize"],
        )],
    );
    let section = BoSection {
        canonical_paths: vec!["crate::domain::*".into()],
        ..Default::default()
    };
    let diags = bo004(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "BO004");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("User"));
    assert!(diags[0].message.contains("Serialize"));
}

#[test]
fn bo004_quiet_when_canonical_paths_empty() {
    let air = air_with_module(
        "crate::domain::user",
        vec![type_with_derives(
            "User",
            "x::domain::user::User",
            vec!["Serialize"],
        )],
    );
    let section = BoSection::default(); // canonical_paths empty
    assert!(bo004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn bo004_quiet_for_non_canonical_module() {
    let air = air_with_module(
        "crate::api::dto",
        vec![type_with_derives(
            "UserDto",
            "x::api::dto::UserDto",
            vec!["Serialize", "Deserialize"],
        )],
    );
    let section = BoSection {
        canonical_paths: vec!["crate::domain::*".into()],
        ..Default::default()
    };
    assert!(bo004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn bo004_matches_qualified_derive_via_short_name() {
    // Some adapters render derives as `serde::Serialize`. The default
    // forbidden list uses short names — match by trailing segment.
    let air = air_with_module(
        "crate::domain::user",
        vec![type_with_derives(
            "User",
            "x::domain::user::User",
            vec!["serde::Serialize"],
        )],
    );
    let section = BoSection {
        canonical_paths: vec!["crate::domain::*".into()],
        ..Default::default()
    };
    let diags = bo004(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("serde::Serialize"));
}

#[test]
fn bo004_emits_one_diagnostic_per_type_even_with_multiple_forbidden_derives() {
    let air = air_with_module(
        "crate::domain::user",
        vec![type_with_derives(
            "User",
            "x::domain::user::User",
            vec!["Serialize", "Deserialize", "ToSchema"],
        )],
    );
    let section = BoSection {
        canonical_paths: vec!["crate::domain::*".into()],
        ..Default::default()
    };
    let diags = bo004(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1, "one diag per type, not one per derive");
}

#[test]
fn bo004_agent_strict_elevates_warning_to_fatal() {
    let air = air_with_module(
        "crate::domain::user",
        vec![type_with_derives(
            "User",
            "x::domain::user::User",
            vec!["Serialize"],
        )],
    );
    let section = BoSection {
        canonical_paths: vec!["crate::domain::*".into()],
        ..Default::default()
    };
    let diags = bo004(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ----- BO005 -----

fn func(symbol: &str, file: &str, line: u32) -> AirItem {
    use locus_air::AirFunction;
    AirItem::Function(AirFunction {
        name: symbol.rsplit("::").next().unwrap_or(symbol).into(),
        symbol: symbol.into(),
        visibility: Visibility::Public,
        params: Vec::new(),
        return_type: None,
        span: AirSpan::new(file, line, line + 5),
        line_count: 6,
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        doc: None,
    })
}

fn persistence_write_fact(symbol: &str, evidence: &str, reason: &str) -> AirFact {
    AirFact {
        kind: FactKind::PersistenceWrite,
        target: FactTarget::Function {
            symbol: symbol.into(),
        },
        source: "std-rt".into(),
        confidence: 1.0,
        reasons: vec![reason.into()],
        evidence: Some(evidence.into()),
    }
}

fn air_with_module_facts(
    module_path: Option<&str>,
    file_path: &str,
    items: Vec<AirItem>,
    facts: Vec<AirFact>,
) -> AirWorkspace {
    AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: file_path.into(),
                module_path: module_path.map(|s| s.into()),
                items,
                hints: Vec::new(),
                parse_error: None,
                line_count: 1,
            }],
        }],
        facts,
    }
}

#[test]
fn bo005_fires_on_persistence_write_in_domain_function() {
    let air = air_with_module_facts(
        Some("crate::domain::user"),
        "src/domain/user.rs",
        vec![func("crate::domain::user::save", "src/domain/user.rs", 8)],
        vec![persistence_write_fact(
            "crate::domain::user::save",
            "std::fs::write",
            "`std::fs::write` is a persistence-write call",
        )],
    );
    let section = BoSection {
        domain_paths: vec!["crate::domain::*".into()],
        ..Default::default()
    };
    let diags = bo005(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.rule_id, "BO005");
    assert_eq!(d.severity, Severity::Fatal);
    assert!(d.message.contains("crate::domain::user::save"));
    assert!(d.message.contains("std::fs::write"));
    assert!(d.message.contains("persistence write"));
    assert!(
        d.why.iter().any(|w| w.contains("crate::domain::*")),
        "expected domain pattern in why; got {:?}",
        d.why
    );
    assert!(
        d.why.iter().any(|w| w.contains("persistence-write")),
        "expected loader reason in why; got {:?}",
        d.why
    );
}

#[test]
fn bo005_quiet_when_target_function_outside_domain_paths() {
    // Adapter/infra layer is allowed to write to storage — that's the
    // whole point of BO putting persistence at the boundary.
    let air = air_with_module_facts(
        Some("crate::infra::user_repo"),
        "src/infra/user_repo.rs",
        vec![func(
            "crate::infra::user_repo::save",
            "src/infra/user_repo.rs",
            8,
        )],
        vec![persistence_write_fact(
            "crate::infra::user_repo::save",
            "std::fs::write",
            "persistence-write call",
        )],
    );
    let section = BoSection {
        domain_paths: vec!["crate::domain::*".into()],
        ..Default::default()
    };
    assert!(bo005(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn bo005_quiet_on_non_persistence_write_facts() {
    let air = air_with_module_facts(
        Some("crate::domain::user"),
        "src/domain/user.rs",
        vec![func("crate::domain::user::save", "src/domain/user.rs", 8)],
        vec![
            AirFact {
                kind: FactKind::Logging,
                target: FactTarget::Function {
                    symbol: "crate::domain::user::save".into(),
                },
                source: "std-rt".into(),
                confidence: 1.0,
                reasons: Vec::new(),
                evidence: None,
            },
            AirFact {
                kind: FactKind::ConfigRead,
                target: FactTarget::Function {
                    symbol: "crate::domain::user::save".into(),
                },
                source: "std-rt".into(),
                confidence: 1.0,
                reasons: Vec::new(),
                evidence: None,
            },
            AirFact {
                kind: FactKind::ExternalIo,
                target: FactTarget::Function {
                    symbol: "crate::domain::user::save".into(),
                },
                source: "std-rt".into(),
                confidence: 1.0,
                reasons: Vec::new(),
                evidence: None,
            },
        ],
    );
    let section = BoSection {
        domain_paths: vec!["crate::domain::*".into()],
        ..Default::default()
    };
    assert!(bo005(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn bo005_silent_when_domain_paths_empty() {
    let air = air_with_module_facts(
        Some("crate::domain::user"),
        "src/domain/user.rs",
        vec![func("crate::domain::user::save", "src/domain/user.rs", 8)],
        vec![persistence_write_fact(
            "crate::domain::user::save",
            "std::fs::write",
            "persistence-write call",
        )],
    );
    let section = BoSection::default(); // domain_paths empty
    assert!(
        bo005(&air, &section, CheckMode::Human).is_empty(),
        "rule should wait for explicit domain_paths declaration"
    );
}

#[test]
fn bo005_agent_strict_keeps_severity_fatal() {
    // BO005 is already Fatal in human mode; --agent-strict elevates but
    // can't go higher than Fatal — verify it stays Fatal, not panicked.
    let air = air_with_module_facts(
        Some("crate::domain::user"),
        "src/domain/user.rs",
        vec![func("crate::domain::user::save", "src/domain/user.rs", 8)],
        vec![persistence_write_fact(
            "crate::domain::user::save",
            "std::fs::create_dir_all",
            "persistence-write call",
        )],
    );
    let section = BoSection {
        domain_paths: vec!["crate::domain::*".into()],
        ..Default::default()
    };
    let diags = bo005(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn bo005_segment_anywhere_pattern_fires_on_inline_test_module_symbol() {
    // The headline use case for the function-symbol fallback: the
    // file's `module_path` is `crate::infra::user_repo` (boundary
    // code, not domain), but the user has carved `*::tests::*` into
    // `domain_paths` to forbid persistence writes in any inline
    // `mod tests {}` block (test fixtures should mock storage, not
    // touch the disk). The function symbol contains `::tests::`,
    // the file's module does not — so only the symbol-side match
    // catches it.
    let air = air_with_module_facts(
        Some("crate::infra::user_repo"),
        "src/infra/user_repo.rs",
        vec![func(
            "crate::infra::user_repo::tests::roundtrip",
            "src/infra/user_repo.rs",
            42,
        )],
        vec![persistence_write_fact(
            "crate::infra::user_repo::tests::roundtrip",
            "std::fs::write",
            "persistence-write call",
        )],
    );
    let section = BoSection {
        domain_paths: vec!["*::tests::*".into()],
        ..Default::default()
    };
    let diags = bo005(&air, &section, CheckMode::Human);
    assert_eq!(
        diags.len(),
        1,
        "function-symbol match should catch inline test modules; got {:?}",
        diags
    );
    assert!(diags[0].message.contains("tests::roundtrip"));
}
