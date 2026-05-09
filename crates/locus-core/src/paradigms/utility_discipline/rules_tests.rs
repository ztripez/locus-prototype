//! Tests for [`super`] rule implementations.
//!
//! Extracted from `rules.rs` to keep the production module within the
//! CX002 line budget. Re-attached via `#[path = "rules_tests.rs"] mod
//! tests;` at the bottom of `rules.rs`.

use super::*;
use locus_air::{
    AIR_SCHEMA_VERSION, AirFile, AirImport, AirPackage, AirSpan, AirTruthAction, AirType, TypeKind,
    Visibility,
};

fn ty(name: &str, vis: Visibility) -> AirItem {
    AirItem::Type(AirType {
        kind: TypeKind::Struct,
        name: name.into(),
        symbol: format!("x::utils::{name}"),
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
fn ut001_fires_on_public_type_in_utility_module() {
    let air = air_with_module("x::utils", vec![ty("Helper", Visibility::Public)]);
    let section = UtSection {
        utility_paths: vec!["x::utils::*".into()],
        ..Default::default()
    };
    let diags = ut001(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "UT001");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("Helper"));
    assert!(diags[0].message.contains("x::utils"));
    assert!(diags[0].message.contains("x::utils::*"));
}

#[test]
fn ut001_quiet_on_private_type_in_utility_module() {
    let air = air_with_module("x::utils", vec![ty("Helper", Visibility::Private)]);
    let section = UtSection {
        utility_paths: vec!["x::utils::*".into()],
        ..Default::default()
    };
    assert!(ut001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ut001_quiet_on_crate_visible_type_in_utility_module() {
    // `pub(crate)` is not full Public — utility modules are allowed to
    // hold crate-visible helpers; only the truly Public surface trips UT001.
    let air = air_with_module("x::utils", vec![ty("Helper", Visibility::Module)]);
    let section = UtSection {
        utility_paths: vec!["x::utils::*".into()],
        ..Default::default()
    };
    assert!(ut001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ut001_quiet_on_public_type_in_non_matching_module() {
    let air = air_with_module("x::domain::user", vec![ty("User", Visibility::Public)]);
    let section = UtSection {
        utility_paths: vec!["x::utils::*".into()],
        ..Default::default()
    };
    assert!(ut001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ut001_silent_when_utility_paths_empty() {
    let air = air_with_module("x::utils", vec![ty("Helper", Visibility::Public)]);
    let section = UtSection::default();
    assert!(ut001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ut001_multiple_public_types_produce_multiple_diagnostics() {
    let air = air_with_module(
        "x::utils",
        vec![
            ty("Helper", Visibility::Public),
            ty("Adapter", Visibility::Public),
            ty("Internal", Visibility::Private), // not flagged
            ty("Bag", Visibility::Public),
        ],
    );
    let section = UtSection {
        utility_paths: vec!["x::utils::*".into()],
        ..Default::default()
    };
    let diags = ut001(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 3);
    let names: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert!(names.iter().any(|m| m.contains("Helper")));
    assert!(names.iter().any(|m| m.contains("Adapter")));
    assert!(names.iter().any(|m| m.contains("Bag")));
    assert!(!names.iter().any(|m| m.contains("Internal")));
}

#[test]
fn ut001_agent_strict_elevates_to_fatal() {
    let air = air_with_module("x::utils", vec![ty("Helper", Visibility::Public)]);
    let section = UtSection {
        utility_paths: vec!["x::utils::*".into()],
        ..Default::default()
    };
    let diags = ut001(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn ut001_matches_exact_module_path_too() {
    // Pattern `x::utils` (no `::*`) should match the exact module.
    let air = air_with_module("x::utils", vec![ty("Helper", Visibility::Public)]);
    let section = UtSection {
        utility_paths: vec!["x::utils".into()],
        ..Default::default()
    };
    let diags = ut001(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
}

fn import(path: &str) -> AirItem {
    AirItem::Import(AirImport {
        path: path.into(),
        path_segments: Vec::new(),
        visibility: Visibility::Private,
        span: AirSpan::new("t.rs", 1, 1),
    })
}

#[test]
fn ut002_fires_when_utility_file_imports_forbidden_path() {
    let air = air_with_module("x::utils", vec![import("crate::domain::user::User")]);
    let section = UtSection {
        utility_paths: vec!["x::utils::*".into()],
        forbidden_imports: vec!["crate::domain::*".into()],
        ..Default::default()
    };
    let diags = ut002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "UT002");
    assert_eq!(diags[0].severity, Severity::Fatal);
    assert!(diags[0].concept.is_none());
    assert!(diags[0].message.contains("x::utils"));
    assert!(diags[0].message.contains("crate::domain::user::User"));
    assert!(
        diags[0].why.iter().any(|w| w.contains("x::utils::*")),
        "expected utility pattern in why; got {:?}",
        diags[0].why
    );
    assert!(
        diags[0].why.iter().any(|w| w.contains("crate::domain::*")),
        "expected forbidden pattern in why; got {:?}",
        diags[0].why
    );
    assert!(
        diags[0].why.iter().any(|w| w.contains("x::utils")),
        "expected importer module in why; got {:?}",
        diags[0].why
    );
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("crate::domain::user::User")),
        "expected import path in why; got {:?}",
        diags[0].why
    );
}

#[test]
fn ut002_quiet_when_non_utility_file_imports_forbidden_path() {
    // Domain modules are allowed to import other domain things — only
    // *utility* modules should be domain-free.
    let air = air_with_module(
        "x::domain::orders",
        vec![import("crate::domain::user::User")],
    );
    let section = UtSection {
        utility_paths: vec!["x::utils::*".into()],
        forbidden_imports: vec!["crate::domain::*".into()],
        ..Default::default()
    };
    assert!(ut002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ut002_quiet_when_utility_file_imports_non_forbidden_path() {
    let air = air_with_module("x::utils", vec![import("std::collections::HashMap")]);
    let section = UtSection {
        utility_paths: vec!["x::utils::*".into()],
        forbidden_imports: vec!["crate::domain::*".into()],
        ..Default::default()
    };
    assert!(ut002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ut002_silent_when_forbidden_imports_empty() {
    let air = air_with_module("x::utils", vec![import("crate::domain::user::User")]);
    let section = UtSection {
        utility_paths: vec!["x::utils::*".into()],
        forbidden_imports: vec![],
        ..Default::default()
    };
    assert!(ut002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ut002_silent_when_utility_paths_empty() {
    let air = air_with_module("x::utils", vec![import("crate::domain::user::User")]);
    let section = UtSection {
        utility_paths: vec![],
        forbidden_imports: vec!["crate::domain::*".into()],
        ..Default::default()
    };
    assert!(ut002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ut002_silent_with_default_section() {
    let air = air_with_module("x::utils", vec![import("crate::domain::user::User")]);
    let section = UtSection::default();
    assert!(ut002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ut002_agent_strict_keeps_severity_fatal() {
    // UT002 is already Fatal in human mode; --agent-strict elevates but
    // can't go higher than Fatal — verify it stays Fatal.
    let air = air_with_module("x::utils", vec![import("crate::roles::Admin")]);
    let section = UtSection {
        utility_paths: vec!["x::utils::*".into()],
        forbidden_imports: vec!["crate::roles::*".into()],
        ..Default::default()
    };
    let diags = ut002(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ---- UT003 / UT004 / UT005 helpers ----

fn truth_action(kind: ActionKind, target: &str, line: u32) -> AirItem {
    AirItem::TruthAction(AirTruthAction {
        action: kind,
        target: target.into(),
        function: None,
        span: AirSpan::new("t.rs", line, line),
        confidence: 0.9,
        reasons: Vec::new(),
    })
}

// ---- UT003 tests ----

#[test]
fn ut003_silent_when_generic_utility_patterns_empty() {
    let air = air_with_module("x::utils", vec![ty("Helper", Visibility::Private)]);
    let section = UtSection::default();
    assert!(ut003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ut003_fires_on_generic_utility_module_without_acceptance() {
    let air = air_with_module("x::utils", vec![ty("Helper", Visibility::Private)]);
    let section = UtSection {
        generic_utility_patterns: vec!["*::utils::*".into()],
        ..Default::default()
    };
    let diags = ut003(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "UT003");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("x::utils"));
    assert!(diags[0].message.contains("*::utils::*"));
}

#[test]
fn ut003_quiet_when_module_is_explicitly_accepted() {
    let air = air_with_module("x::utils", vec![ty("Helper", Visibility::Private)]);
    let section = UtSection {
        generic_utility_patterns: vec!["*::utils::*".into()],
        accepted_utility_paths: vec!["x::utils".into()],
        ..Default::default()
    };
    assert!(ut003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ut003_accepted_supports_wildcard_patterns() {
    // Acceptance via a glob, not just exact path.
    let air = air_with_module("x::utils::time", vec![ty("Clock", Visibility::Private)]);
    let section = UtSection {
        generic_utility_patterns: vec!["*::utils::*".into()],
        accepted_utility_paths: vec!["x::utils::*".into()],
        ..Default::default()
    };
    assert!(ut003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ut003_quiet_when_module_does_not_match_generic_patterns() {
    let air = air_with_module("x::domain::user", vec![ty("User", Visibility::Public)]);
    let section = UtSection {
        generic_utility_patterns: vec!["*::utils::*".into(), "*::helpers".into()],
        ..Default::default()
    };
    assert!(ut003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ut003_agent_strict_elevates_to_fatal() {
    let air = air_with_module("x::helpers", vec![ty("Util", Visibility::Private)]);
    let section = UtSection {
        generic_utility_patterns: vec!["*::helpers".into()],
        ..Default::default()
    };
    let diags = ut003(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ---- UT004 tests ----

#[test]
fn ut004_silent_when_utility_paths_empty() {
    let air = air_with_module(
        "x::utils",
        vec![truth_action(ActionKind::Validate, "email", 5)],
    );
    let section = UtSection::default();
    assert!(ut004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ut004_fires_on_validate_action_in_utility_module() {
    // UT004 requires the action target to match a canonical pattern —
    // otherwise UT005 would handle it.
    let air = air_with_module(
        "x::utils",
        vec![truth_action(ActionKind::Validate, "Email", 5)],
    );
    let section = UtSection {
        utility_paths: vec!["x::utils::*".into()],
        canonical_construct_patterns: vec!["Email".into()],
        ..Default::default()
    };
    let diags = ut004(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "UT004");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("validation"));
    assert!(diags[0].message.contains("Email"));
}

#[test]
fn ut004_fires_on_normalize_action_in_utility_module() {
    let air = air_with_module(
        "x::utils",
        vec![truth_action(ActionKind::Normalize, "UserName", 7)],
    );
    let section = UtSection {
        utility_paths: vec!["x::utils::*".into()],
        canonical_construct_patterns: vec!["UserName".into()],
        ..Default::default()
    };
    let diags = ut004(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("normalization"));
}

#[test]
fn ut004_construct_only_fires_when_target_matches_canonical_pattern() {
    // Construct of a non-canonical target → quiet.
    let air = air_with_module(
        "x::utils",
        vec![truth_action(ActionKind::Construct, "Vec", 5)],
    );
    let section = UtSection {
        utility_paths: vec!["x::utils::*".into()],
        canonical_construct_patterns: vec!["*::User".into()],
        ..Default::default()
    };
    assert!(ut004(&air, &section, CheckMode::Human).is_empty());

    // Construct of a canonical target → fires.
    let air = air_with_module(
        "x::utils",
        vec![truth_action(
            ActionKind::Construct,
            "crate::domain::User",
            5,
        )],
    );
    let diags = ut004(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("construction"));
    assert!(diags[0].message.contains("crate::domain::User"));
}

#[test]
fn ut004_quiet_in_non_utility_module() {
    let air = air_with_module(
        "x::domain::user",
        vec![truth_action(ActionKind::Validate, "email", 5)],
    );
    let section = UtSection {
        utility_paths: vec!["x::utils::*".into()],
        ..Default::default()
    };
    assert!(ut004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ut004_agent_strict_elevates_to_fatal() {
    let air = air_with_module(
        "x::utils",
        vec![truth_action(ActionKind::Validate, "Email", 5)],
    );
    let section = UtSection {
        utility_paths: vec!["x::utils::*".into()],
        canonical_construct_patterns: vec!["Email".into()],
        ..Default::default()
    };
    let diags = ut004(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ---- UT005 tests ----

#[test]
fn ut005_silent_when_utility_paths_empty() {
    let air = air_with_module(
        "x::utils",
        vec![truth_action(ActionKind::Validate, "email", 5)],
    );
    let section = UtSection::default();
    assert!(ut005(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ut005_fires_on_validate_action() {
    let air = air_with_module(
        "x::utils",
        vec![truth_action(ActionKind::Validate, "email", 5)],
    );
    let section = UtSection {
        utility_paths: vec!["x::utils::*".into()],
        ..Default::default()
    };
    let diags = ut005(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "UT005");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("validation"));
    assert!(diags[0].message.contains("email"));
}

#[test]
fn ut005_fires_on_normalize_action() {
    let air = air_with_module(
        "x::utils",
        vec![truth_action(ActionKind::Normalize, "phone", 5)],
    );
    let section = UtSection {
        utility_paths: vec!["x::utils::*".into()],
        ..Default::default()
    };
    let diags = ut005(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("normalization"));
}

#[test]
fn ut005_quiet_on_construct_action_regardless_of_pattern() {
    // UT005 ignores Construct actions even when they match canonical
    // patterns — that's UT004's territory.
    let air = air_with_module(
        "x::utils",
        vec![truth_action(
            ActionKind::Construct,
            "crate::domain::User",
            5,
        )],
    );
    let section = UtSection {
        utility_paths: vec!["x::utils::*".into()],
        canonical_construct_patterns: vec!["*::User".into()],
        ..Default::default()
    };
    assert!(ut005(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ut005_quiet_in_non_utility_module() {
    let air = air_with_module(
        "x::domain::user",
        vec![truth_action(ActionKind::Validate, "email", 5)],
    );
    let section = UtSection {
        utility_paths: vec!["x::utils::*".into()],
        ..Default::default()
    };
    assert!(ut005(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ut005_agent_strict_elevates_to_fatal() {
    let air = air_with_module(
        "x::utils",
        vec![truth_action(ActionKind::Normalize, "name", 5)],
    );
    let section = UtSection {
        utility_paths: vec!["x::utils::*".into()],
        ..Default::default()
    };
    let diags = ut005(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}
