//! Tests for [`super`] rule implementations.
//!
//! Extracted from `rules.rs` to keep the production module within the
//! CX002 line budget. Re-attached via `#[path = "rules_tests.rs"] mod
//! tests;` at the bottom of `rules.rs`.

use super::*;
use locus_air::{AIR_SCHEMA_VERSION, AirFile, AirPackage, AirSpan, AirTruthAction, AirWorkspace};

fn construct(target: &str, function: &str, file_path: &str, line: u32) -> AirItem {
    AirItem::TruthAction(AirTruthAction {
        action: ActionKind::Construct,
        target: target.into(),
        function: Some(function.into()),
        span: AirSpan::new(file_path, line, line),
        confidence: 0.95,
        reasons: vec!["struct literal".into()],
    })
}

fn air_with_file(module_path: &str, file_path: &str, items: Vec<AirItem>) -> AirWorkspace {
    AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: file_path.into(),
                module_path: Some(module_path.into()),
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
fn cr001_fires_on_service_shaped_construct_outside_root() {
    let air = air_with_file(
        "crate::handler",
        "src/handler.rs",
        vec![construct(
            "UserRepository",
            "crate::handler::create_user",
            "src/handler.rs",
            12,
        )],
    );
    let section = CrSection {
        composition_root_paths: vec!["crate::wire".into(), "bin::*".into()],
        service_suffixes: Vec::new(),
        ..Default::default()
    };
    let diags = cr001(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "CR001");
    assert_eq!(diags[0].severity, Severity::Fatal);
    assert!(diags[0].message.contains("UserRepository"));
    assert!(diags[0].message.contains("crate::handler"));
    assert!(diags[0].message.contains("Repository"));
}

#[test]
fn cr001_quiet_inside_composition_root() {
    let air = air_with_file(
        "crate::wire",
        "src/wire.rs",
        vec![construct(
            "UserRepository",
            "crate::wire::build_app",
            "src/wire.rs",
            3,
        )],
    );
    let section = CrSection {
        composition_root_paths: vec!["crate::wire".into()],
        service_suffixes: Vec::new(),
        ..Default::default()
    };
    assert!(cr001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cr001_quiet_on_non_service_shaped_target() {
    let air = air_with_file(
        "crate::handler",
        "src/handler.rs",
        vec![construct(
            "User",
            "crate::handler::create_user",
            "src/handler.rs",
            7,
        )],
    );
    let section = CrSection {
        composition_root_paths: vec!["crate::wire".into()],
        service_suffixes: Vec::new(),
        ..Default::default()
    };
    assert!(cr001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cr001_silent_when_composition_root_paths_empty() {
    let air = air_with_file(
        "crate::handler",
        "src/handler.rs",
        vec![construct(
            "UserRepository",
            "crate::handler::create_user",
            "src/handler.rs",
            4,
        )],
    );
    let section = CrSection::default();
    assert!(
        cr001(&air, &section, CheckMode::Human).is_empty(),
        "rule should wait for explicit composition_root_paths declaration"
    );
}

#[test]
fn cr001_agent_strict_keeps_fatal() {
    let air = air_with_file(
        "crate::handler",
        "src/handler.rs",
        vec![construct(
            "PaymentClient",
            "crate::handler::charge",
            "src/handler.rs",
            9,
        )],
    );
    let section = CrSection {
        composition_root_paths: vec!["crate::wire".into()],
        service_suffixes: Vec::new(),
        ..Default::default()
    };
    let diags = cr001(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn cr001_custom_service_suffixes_override_defaults() {
    // Default suffixes would NOT catch `Gateway`; a `Repository` would.
    // With a user override that drops `Repository` and adds `Gateway`,
    // the behaviour flips.
    let air = air_with_file(
        "crate::handler",
        "src/handler.rs",
        vec![
            construct(
                "PaymentGateway",
                "crate::handler::charge",
                "src/handler.rs",
                11,
            ),
            construct(
                "UserRepository",
                "crate::handler::create_user",
                "src/handler.rs",
                22,
            ),
        ],
    );
    let section = CrSection {
        composition_root_paths: vec!["crate::wire".into()],
        service_suffixes: vec!["Gateway".into()],
        ..Default::default()
    };
    let diags = cr001(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1, "only `Gateway` should match; got {diags:?}");
    assert!(diags[0].message.contains("PaymentGateway"));
    assert!(!diags[0].message.contains("UserRepository"));
}

#[test]
fn cr001_matches_path_qualified_target() {
    // Constructions like `crate::infra::DbConnection { ... }` appear in
    // AIR with the full path; the suffix check uses the last `::` segment.
    let air = air_with_file(
        "crate::handler",
        "src/handler.rs",
        vec![construct(
            "crate::infra::DbConnection",
            "crate::handler::open",
            "src/handler.rs",
            5,
        )],
    );
    let section = CrSection {
        composition_root_paths: vec!["crate::wire".into()],
        service_suffixes: Vec::new(),
        ..Default::default()
    };
    let diags = cr001(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("crate::infra::DbConnection"));
    assert!(diags[0].message.contains("Connection"));
}

// ----- CR002 -----

fn many_constructs(targets: &[&str], function: &str, file_path: &str) -> Vec<AirItem> {
    targets
        .iter()
        .enumerate()
        .map(|(i, t)| construct(t, function, file_path, (i as u32) + 1))
        .collect()
}

#[test]
fn cr002_fires_when_wiring_density_meets_threshold() {
    let targets: Vec<&str> = (0..12).map(|_| "ServiceX").collect();
    let items = many_constructs(&targets, "crate::wire::build_app", "src/wire.rs");
    let air = air_with_file("crate::wire", "src/wire.rs", items);
    let section = CrSection {
        composition_root_paths: vec!["crate::wire".into()],
        wiring_density_threshold: 12,
        ..Default::default()
    };
    let diags = cr002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "CR002");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("12"));
    assert!(diags[0].message.contains("crate::wire::build_app"));
}

#[test]
fn cr002_quiet_below_threshold() {
    let targets: Vec<&str> = (0..11).map(|_| "ServiceX").collect();
    let items = many_constructs(&targets, "crate::wire::build_app", "src/wire.rs");
    let air = air_with_file("crate::wire", "src/wire.rs", items);
    let section = CrSection {
        composition_root_paths: vec!["crate::wire".into()],
        wiring_density_threshold: 12,
        ..Default::default()
    };
    assert!(cr002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cr002_silent_when_composition_root_paths_empty() {
    let targets: Vec<&str> = (0..30).map(|_| "ServiceX").collect();
    let items = many_constructs(&targets, "crate::handler::run", "src/handler.rs");
    let air = air_with_file("crate::handler", "src/handler.rs", items);
    let section = CrSection::default();
    assert!(cr002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cr002_quiet_for_function_outside_root_modules() {
    // Even with a populated `composition_root_paths`, a non-root file
    // doesn't trigger CR002 (CR001's job to flag wiring there).
    let targets: Vec<&str> = (0..30).map(|_| "ServiceX").collect();
    let items = many_constructs(&targets, "crate::handler::run", "src/handler.rs");
    let air = air_with_file("crate::handler", "src/handler.rs", items);
    let section = CrSection {
        composition_root_paths: vec!["crate::wire".into()],
        wiring_density_threshold: 12,
        ..Default::default()
    };
    assert!(cr002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cr002_groups_counts_per_enclosing_function() {
    // Two functions in the same root file, each below threshold,
    // shouldn't accumulate together.
    let mut items = many_constructs(
        &["A", "B", "C", "D", "E", "F"],
        "crate::wire::build_a",
        "src/wire.rs",
    );
    items.extend(many_constructs(
        &["A", "B", "C", "D", "E", "F", "G"],
        "crate::wire::build_b",
        "src/wire.rs",
    ));
    let air = air_with_file("crate::wire", "src/wire.rs", items);
    let section = CrSection {
        composition_root_paths: vec!["crate::wire".into()],
        wiring_density_threshold: 12,
        ..Default::default()
    };
    assert!(cr002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cr002_agent_strict_elevates_warning_to_fatal() {
    let targets: Vec<&str> = (0..15).map(|_| "ServiceX").collect();
    let items = many_constructs(&targets, "crate::wire::build_app", "src/wire.rs");
    let air = air_with_file("crate::wire", "src/wire.rs", items);
    let section = CrSection {
        composition_root_paths: vec!["crate::wire".into()],
        wiring_density_threshold: 12,
        ..Default::default()
    };
    let diags = cr002(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn cr002_threshold_zero_stays_silent() {
    // Defensive: a 0 threshold is almost certainly a config bug; rule
    // refuses to spam the user.
    let items = many_constructs(&["A"], "crate::wire::build", "src/wire.rs");
    let air = air_with_file("crate::wire", "src/wire.rs", items);
    let section = CrSection {
        composition_root_paths: vec!["crate::wire".into()],
        wiring_density_threshold: 0,
        ..Default::default()
    };
    assert!(cr002(&air, &section, CheckMode::Human).is_empty());
}
