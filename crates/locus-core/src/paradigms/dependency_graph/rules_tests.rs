use super::super::lockfile_schema::ForbiddenEdge;
use super::*;
use locus_air::{AIR_SCHEMA_VERSION, AirFile, AirImport, AirPackage, AirSpan, Visibility};

use crate::governance::finding::RuleFinding;
use crate::governance::ids::{FindingIdMinter, RuleId};
use crate::governance::registry::{ParadigmRegistry, RuleRegistry};
use crate::governance::rule::{RuleContext, RuleDefinition};
use crate::lockfile::Lockfile;

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

fn forbid(from: &str, to: &str) -> ForbiddenEdge {
    ForbiddenEdge {
        from: from.into(),
        to: to.into(),
        reason: None,
    }
}

fn observe_dg001(
    air: &AirWorkspace,
    section: &DgSection,
    mode: CheckMode,
) -> Vec<RuleFinding> {
    let mut lf = Lockfile::default();
    lf.paradigms
        .insert("DG".to_string(), serde_json::to_value(section).unwrap());
    let rules = RuleRegistry::standard();
    let paradigms = ParadigmRegistry::empty();
    let minter = FindingIdMinter::new();
    let ctx = RuleContext {
        air,
        lockfile: &lf,
        mode,
        rule_registry: &rules,
        paradigm_registry: &paradigms,
        finding_ids: &minter,
    };
    dg001::Dg001Rule.observe(&ctx)
}

#[test]
fn dg001_fires_when_module_imports_forbidden_path() {
    let air = air_with_module("lore::domain::user", vec![import("lore::api::v1::UserDto")]);
    let section = DgSection {
        forbidden_edges: vec![forbid("lore::domain::*", "lore::api::*")],
        ..DgSection::default()
    };
    let findings = observe_dg001(&air, &section, CheckMode::Human);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule_id, Some(RuleId::new("DG001")));
    assert_eq!(findings[0].default_severity, Severity::Fatal);
    assert!(findings[0].message.contains("lore::api::v1::UserDto"));
    assert!(findings[0].message.contains("lore::domain::user"));
}

#[test]
fn dg001_quiet_when_no_edges_match() {
    let air = air_with_module("lore::domain::user", vec![import("lore::core::Config")]);
    let section = DgSection {
        forbidden_edges: vec![forbid("lore::domain::*", "lore::api::*")],
        ..DgSection::default()
    };
    assert!(observe_dg001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn dg001_silent_with_empty_lockfile() {
    let air = air_with_module("lore::domain::user", vec![import("lore::api::v1::UserDto")]);
    let section = DgSection::default();
    assert!(observe_dg001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn dg001_skips_non_matching_module_even_if_import_matches() {
    // `from` constrains the importer; api importing api is fine here.
    let air = air_with_module("lore::api::handler", vec![import("lore::api::v1::UserDto")]);
    let section = DgSection {
        forbidden_edges: vec![forbid("lore::domain::*", "lore::api::*")],
        ..DgSection::default()
    };
    assert!(observe_dg001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn dg001_one_diagnostic_per_file_per_import_when_multiple_edges_match() {
    let air = air_with_module("lore::domain::user", vec![import("lore::api::v1::UserDto")]);
    let section = DgSection {
        forbidden_edges: vec![
            forbid("lore::domain::*", "lore::api::*"),
            forbid("*", "lore::api::v1::UserDto"), // separately covers the same import
        ],
        ..DgSection::default()
    };
    let findings = observe_dg001(&air, &section, CheckMode::Human);
    assert_eq!(
        findings.len(),
        1,
        "overlapping forbidden edges should not double-report; got {findings:?}"
    );
}

// ---- DG002 ----

type FileSpec<'a> = (&'a str, &'a str, Vec<&'a str>);
type PkgSpec<'a> = (&'a str, Vec<FileSpec<'a>>);

fn air_with_pkgs(pkgs: Vec<PkgSpec<'_>>) -> AirWorkspace {
    // Each pkg is (name, [(file_path, module_path, imports)]).
    AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: pkgs
            .into_iter()
            .map(|(name, files)| AirPackage {
                name: name.into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: files
                    .into_iter()
                    .map(|(path, module, imports)| AirFile {
                        path: path.into(),
                        module_path: Some(module.into()),
                        items: imports.into_iter().map(import).collect(),
                        hints: Vec::new(),
                        parse_error: None,
                        line_count: 1,
                    })
                    .collect(),
            })
            .collect(),
        facts: Vec::new(),
    }
}

#[test]
fn dg002_fires_on_two_crate_cycle() {
    let air = air_with_pkgs(vec![
        ("a", vec![("a/src/lib.rs", "a", vec!["b::Type1"])]),
        ("b", vec![("b/src/lib.rs", "b", vec!["a::Type2"])]),
    ]);
    let diags = dg002(&air, CheckMode::Human);
    assert_eq!(diags.len(), 2, "one diag per edge in SCC; got {diags:?}");
    for d in &diags {
        assert_eq!(d.rule_id, "DG002");
        assert_eq!(d.severity, Severity::Fatal);
        // 2-cycle uses ↔ shorthand in the cycle label.
        assert!(
            d.message.contains("`a` ↔ `b`") || d.message.contains("`b` ↔ `a`"),
            "expected ↔ label for 2-cycle; got `{}`",
            d.message
        );
    }
    let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert!(messages.iter().any(|m| m.contains("`a` -> `b::Type1`")));
    assert!(messages.iter().any(|m| m.contains("`b` -> `a::Type2`")));
}

#[test]
fn dg002_fires_on_three_cycle() {
    // a -> b -> c -> a, no shortcut edges.
    let air = air_with_pkgs(vec![
        ("a", vec![("a/src/lib.rs", "a", vec!["b::T"])]),
        ("b", vec![("b/src/lib.rs", "b", vec!["c::T"])]),
        ("c", vec![("c/src/lib.rs", "c", vec!["a::T"])]),
    ]);
    let diags = dg002(&air, CheckMode::Human);
    assert_eq!(
        diags.len(),
        3,
        "3-cycle has 3 edges, 3 diagnostics; got {diags:?}"
    );
    for d in &diags {
        assert!(d.message.contains("`a`"));
        assert!(d.message.contains("`b`"));
        assert!(d.message.contains("`c`"));
    }
}

// ---- DG003 ----

fn feature(name: &str, module: &str, public_api: &[&str]) -> FeatureDefinition {
    FeatureDefinition {
        name: name.into(),
        module: module.into(),
        public_api: public_api.iter().map(|s| (*s).to_string()).collect(),
    }
}

#[test]
fn dg003_fires_on_cross_feature_internals_reach() {
    let air = air_with_pkgs(vec![(
        "ethics",
        vec![(
            "ethics/src/eval.rs",
            "ethics::eval",
            vec!["anatom::morals::MoralAct"],
        )],
    )]);
    let section = DgSection {
        features: vec![
            feature("anatom", "anatom::*", &["anatom::api::*"]),
            feature("ethics", "ethics::*", &[]),
        ],
        ..DgSection::default()
    };
    let diags = dg003(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "DG003");
    assert_eq!(diags[0].severity, Severity::Fatal);
    assert!(diags[0].message.contains("`ethics`"));
    assert!(diags[0].message.contains("`anatom`"));
    assert!(diags[0].message.contains("MoralAct"));
}

#[test]
fn dg003_quiet_when_target_is_in_public_api() {
    let air = air_with_pkgs(vec![(
        "ethics",
        vec![(
            "ethics/src/eval.rs",
            "ethics::eval",
            vec!["anatom::api::evaluate"],
        )],
    )]);
    let section = DgSection {
        features: vec![
            feature("anatom", "anatom::*", &["anatom::api::*"]),
            feature("ethics", "ethics::*", &[]),
        ],
        ..DgSection::default()
    };
    assert!(dg003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn dg003_quiet_for_intra_feature_imports() {
    let air = air_with_pkgs(vec![(
        "anatom",
        vec![(
            "anatom/src/internal.rs",
            "anatom::internal",
            vec!["anatom::morals::MoralAct"],
        )],
    )]);
    let section = DgSection {
        features: vec![
            feature("anatom", "anatom::*", &["anatom::api::*"]),
            feature("ethics", "ethics::*", &[]),
        ],
        ..DgSection::default()
    };
    assert!(dg003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn dg003_silent_when_under_two_features_defined() {
    let air = air_with_pkgs(vec![("x", vec![("x/src/lib.rs", "x", vec!["y::Foo"])])]);
    let section = DgSection {
        features: vec![feature("x", "x::*", &[])],
        ..DgSection::default()
    };
    assert!(dg003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn dg003_quiet_when_importer_is_not_a_feature() {
    let air = air_with_pkgs(vec![(
        "scripts",
        vec![(
            "scripts/src/main.rs",
            "scripts::main",
            vec!["anatom::morals::MoralAct"],
        )],
    )]);
    let section = DgSection {
        features: vec![
            feature("anatom", "anatom::*", &[]),
            feature("ethics", "ethics::*", &[]),
        ],
        ..DgSection::default()
    };
    assert!(dg003(&air, &section, CheckMode::Human).is_empty());
}

// ---- DG004 ----

#[test]
fn dg004_fires_on_shared_to_feature_import() {
    let air = air_with_pkgs(vec![(
        "core",
        vec![(
            "core/src/util.rs",
            "core::util",
            vec!["anatom::types::Anatom"],
        )],
    )]);
    let section = DgSection {
        features: vec![feature("anatom", "anatom::*", &["anatom::api::*"])],
        shared_paths: vec!["core::*".into()],
        ..DgSection::default()
    };
    let diags = dg004(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "DG004");
    assert_eq!(diags[0].severity, Severity::Fatal);
    assert!(diags[0].message.contains("core::util"));
    assert!(diags[0].message.contains("anatom"));
}

#[test]
fn dg004_quiet_when_shared_imports_non_feature() {
    let air = air_with_pkgs(vec![(
        "core",
        vec![("core/src/util.rs", "core::util", vec!["std::fmt::Debug"])],
    )]);
    let section = DgSection {
        features: vec![feature("anatom", "anatom::*", &[])],
        shared_paths: vec!["core::*".into()],
        ..DgSection::default()
    };
    assert!(dg004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn dg004_quiet_when_importer_not_shared() {
    let air = air_with_pkgs(vec![(
        "anatom",
        vec![("anatom/src/lib.rs", "anatom", vec!["other_feature::Thing"])],
    )]);
    let section = DgSection {
        features: vec![
            feature("other_feature", "other_feature::*", &[]),
            feature("anatom", "anatom::*", &[]),
        ],
        shared_paths: vec!["core::*".into()],
        ..DgSection::default()
    };
    assert!(dg004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn dg004_silent_without_shared_paths() {
    let air = air_with_pkgs(vec![(
        "anywhere",
        vec![(
            "anywhere/src/lib.rs",
            "anywhere",
            vec!["anatom::types::Anatom"],
        )],
    )]);
    let section = DgSection {
        features: vec![feature("anatom", "anatom::*", &[])],
        shared_paths: vec![],
        ..DgSection::default()
    };
    assert!(dg004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn dg002_treats_disjoint_sccs_independently() {
    let air = air_with_pkgs(vec![
        ("a", vec![("a/src/lib.rs", "a", vec!["b::T"])]),
        ("b", vec![("b/src/lib.rs", "b", vec!["a::T"])]),
        ("c", vec![("c/src/lib.rs", "c", vec!["d::T"])]),
        ("d", vec![("d/src/lib.rs", "d", vec!["c::T"])]),
    ]);
    let diags = dg002(&air, CheckMode::Human);
    assert_eq!(
        diags.len(),
        4,
        "two disjoint 2-cycles → 4 diagnostics; got {diags:?}"
    );
    let ab = diags
        .iter()
        .filter(|d| d.message.contains("`a` ↔ `b`") || d.message.contains("`b` ↔ `a`"))
        .count();
    let cd = diags
        .iter()
        .filter(|d| d.message.contains("`c` ↔ `d`") || d.message.contains("`d` ↔ `c`"))
        .count();
    assert_eq!(ab, 2);
    assert_eq!(cd, 2);
}

#[test]
fn dg002_silent_when_only_one_direction() {
    let air = air_with_pkgs(vec![
        ("a", vec![("a/src/lib.rs", "a", vec!["b::Type"])]),
        ("b", vec![("b/src/lib.rs", "b", vec![])]),
    ]);
    assert!(dg002(&air, CheckMode::Human).is_empty());
}

#[test]
fn dg002_ignores_intra_crate_self_loops() {
    // a's file imports a::other — same crate, not a cycle.
    let air = air_with_pkgs(vec![(
        "a",
        vec![("a/src/lib.rs", "a", vec!["a::other::Thing"])],
    )]);
    assert!(dg002(&air, CheckMode::Human).is_empty());
}

#[test]
fn dg002_finds_multiple_cycles_independently() {
    let air = air_with_pkgs(vec![
        ("a", vec![("a/src/lib.rs", "a", vec!["b::T", "c::T"])]),
        ("b", vec![("b/src/lib.rs", "b", vec!["a::T"])]),
        ("c", vec![("c/src/lib.rs", "c", vec!["a::T"])]),
    ]);
    let diags = dg002(&air, CheckMode::Human);
    // Two separate 2-cycles (a<->b, a<->c) → 4 diagnostics total.
    assert_eq!(diags.len(), 4, "got {diags:?}");
}

#[test]
fn dg002_does_not_double_report_same_cycle() {
    // Multiple imports in each direction shouldn't multiply diagnostics.
    let air = air_with_pkgs(vec![
        (
            "a",
            vec![("a/src/lib.rs", "a", vec!["b::T1", "b::T2", "b::T3"])],
        ),
        ("b", vec![("b/src/lib.rs", "b", vec!["a::U1", "a::U2"])]),
    ]);
    let diags = dg002(&air, CheckMode::Human);
    assert_eq!(diags.len(), 2, "one diag per direction; got {diags:?}");
}

#[test]
fn dg001_carries_reason_into_why() {
    let air = air_with_module("lore::domain::user", vec![import("lore::api::v1::UserDto")]);
    let section = DgSection {
        forbidden_edges: vec![ForbiddenEdge {
            from: "lore::domain::*".into(),
            to: "lore::api::*".into(),
            reason: Some("domain must not depend on transport".into()),
        }],
        ..DgSection::default()
    };
    let findings = observe_dg001(&air, &section, CheckMode::Human);
    assert!(
        findings[0]
            .why
            .iter()
            .any(|w| w.contains("domain must not depend on transport")),
        "expected reason in `why`; got {:?}",
        findings[0].why
    );
}
