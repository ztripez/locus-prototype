use super::*;
use locus_air::{AIR_SCHEMA_VERSION, AirFile, AirPackage, AirSpan, AirType, Visibility};

fn trait_item(name: &str, symbol: &str, file: &str, line: u32) -> AirItem {
    AirItem::Type(AirType {
        kind: TypeKind::Trait,
        name: name.into(),
        symbol: symbol.into(),
        visibility: Visibility::Public,
        fields: Vec::new(),
        variants: Vec::new(),
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        span: AirSpan::new(file, line, line),
        doc: None,
    })
}

fn impl_item(trait_path: Option<&str>, self_ty: &str, file: &str, line: u32) -> AirItem {
    AirItem::Impl(AirImplBlock {
        interface: trait_path.map(|s| s.to_string()),
        target_type: self_ty.into(),
        method_names: Vec::new(),
        dispatch: locus_air::ImplDispatch::Static,
        span: AirSpan::new(file, line, line),
    })
}

fn workspace(files: Vec<(&str, Vec<AirItem>)>) -> AirWorkspace {
    AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: files
                .into_iter()
                .map(|(path, items)| AirFile {
                    path: path.into(),
                    module_path: Some(path.replace('/', "::").replace(".rs", "")),
                    items,
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                })
                .collect(),
        }],
        facts: Vec::new(),
    }
}

#[test]
fn pa001_fires_when_trait_and_only_impl_share_file() {
    let air = workspace(vec![(
        "src/lib.rs",
        vec![
            trait_item("Clock", "x::Clock", "src/lib.rs", 10),
            impl_item(Some("x::Clock"), "SystemClock", "src/lib.rs", 20),
        ],
    )]);
    let diags = pa001(&air, &PaSection::default(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "PA001");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("Clock"));
    assert!(diags[0].message.contains("SystemClock"));
    assert!(diags[0].message.contains("src/lib.rs"));
}

#[test]
fn pa001_quiet_when_impl_in_different_file() {
    let air = workspace(vec![
        (
            "src/ports.rs",
            vec![trait_item("Clock", "x::ports::Clock", "src/ports.rs", 10)],
        ),
        (
            "src/adapters.rs",
            vec![impl_item(
                Some("x::ports::Clock"),
                "SystemClock",
                "src/adapters.rs",
                5,
            )],
        ),
    ]);
    assert!(pa001(&air, &PaSection::default(), CheckMode::Human).is_empty());
}

#[test]
fn pa001_quiet_when_trait_has_zero_impls() {
    let air = workspace(vec![(
        "src/lib.rs",
        vec![trait_item("Clock", "x::Clock", "src/lib.rs", 10)],
    )]);
    assert!(pa001(&air, &PaSection::default(), CheckMode::Human).is_empty());
}

#[test]
fn pa001_quiet_when_trait_has_two_or_more_impls() {
    let air = workspace(vec![(
        "src/lib.rs",
        vec![
            trait_item("Clock", "x::Clock", "src/lib.rs", 10),
            impl_item(Some("x::Clock"), "SystemClock", "src/lib.rs", 20),
            impl_item(Some("x::Clock"), "TestClock", "src/lib.rs", 30),
        ],
    )]);
    assert!(pa001(&air, &PaSection::default(), CheckMode::Human).is_empty());
}

#[test]
fn pa001_pattern_in_accepted_colocated_traits_exempts_trait() {
    let air = workspace(vec![(
        "src/lib.rs",
        vec![
            trait_item("Helper", "x::utils::Helper", "src/lib.rs", 10),
            impl_item(Some("x::utils::Helper"), "Thing", "src/lib.rs", 20),
        ],
    )]);
    let section = PaSection {
        accepted_colocated_traits: vec!["x::utils::*".into()],
        ..Default::default()
    };
    assert!(pa001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn pa001_short_name_pattern_exempts_trait() {
    // Short-name fallback: `Helper` matches the trait's `name` even when
    // its `symbol` is fully-qualified.
    let air = workspace(vec![(
        "src/lib.rs",
        vec![
            trait_item("Helper", "x::utils::Helper", "src/lib.rs", 10),
            impl_item(Some("x::utils::Helper"), "Thing", "src/lib.rs", 20),
        ],
    )]);
    let section = PaSection {
        accepted_colocated_traits: vec!["Helper".into()],
        ..Default::default()
    };
    assert!(pa001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn pa001_inherent_impls_are_not_counted() {
    // Inherent `impl Foo` (no trait) must not count toward the "sole
    // impl" tally — otherwise a trait with zero trait-impls but one
    // inherent impl on the self type would falsely fire.
    let air = workspace(vec![(
        "src/lib.rs",
        vec![
            trait_item("Clock", "x::Clock", "src/lib.rs", 10),
            impl_item(None, "Clock", "src/lib.rs", 20), // inherent — ignored
        ],
    )]);
    assert!(pa001(&air, &PaSection::default(), CheckMode::Human).is_empty());
}

#[test]
fn pa001_agent_strict_elevates_to_fatal() {
    let air = workspace(vec![(
        "src/lib.rs",
        vec![
            trait_item("Clock", "x::Clock", "src/lib.rs", 10),
            impl_item(Some("x::Clock"), "SystemClock", "src/lib.rs", 20),
        ],
    )]);
    let diags = pa001(&air, &PaSection::default(), CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn pa001_matches_impl_by_trait_short_name() {
    // Trait's symbol may be `x::ports::Clock` while impl's `trait_path`
    // is the same fully-qualified path. The matcher uses the short name
    // (last `::` segment) so both line up.
    let air = workspace(vec![(
        "src/lib.rs",
        vec![
            trait_item("Clock", "x::ports::Clock", "src/lib.rs", 10),
            impl_item(Some("x::ports::Clock"), "SystemClock", "src/lib.rs", 20),
        ],
    )]);
    let diags = pa001(&air, &PaSection::default(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
}

#[test]
fn pa001_diagnostic_includes_why_and_fix() {
    let air = workspace(vec![(
        "src/lib.rs",
        vec![
            trait_item("Clock", "x::Clock", "src/lib.rs", 10),
            impl_item(Some("x::Clock"), "SystemClock", "src/lib.rs", 20),
        ],
    )]);
    let diags = pa001(&air, &PaSection::default(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert!(d.why.iter().any(|w| w.contains("declared in")));
    assert!(d.why.iter().any(|w| w.contains("sole impl")));
    assert!(
        d.why
            .iter()
            .any(|w| w.contains("accepted_colocated_traits"))
    );
    let fix = d.suggested_fix.as_deref().unwrap_or("");
    assert!(fix.contains("ports"));
    assert!(fix.contains("accepted_colocated_traits"));
}

// ----- PA002 / PA004 helpers -----

fn import_item(path: &str, file: &str, line: u32) -> AirItem {
    use locus_air::AirImport;
    AirItem::Import(AirImport {
        path: path.into(),
        path_segments: Vec::new(),
        visibility: Visibility::Private,
        span: AirSpan::new(file, line, line),
    })
}

fn construct_action(target: &str, function: &str, file: &str, line: u32) -> AirItem {
    use locus_air::AirTruthAction;
    AirItem::TruthAction(AirTruthAction {
        action: ActionKind::Construct,
        target: target.into(),
        function: Some(function.into()),
        span: AirSpan::new(file, line, line),
        confidence: 0.95,
        reasons: vec!["struct literal".into()],
    })
}

fn workspace_with_module(module_path: &str, file: &str, items: Vec<AirItem>) -> AirWorkspace {
    AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: file.into(),
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

// ----- PA002 -----

#[test]
fn pa002_fires_when_application_imports_concrete_adapter() {
    let air = workspace_with_module(
        "crate::application::user",
        "src/app.rs",
        vec![import_item("reqwest::Client", "src/app.rs", 4)],
    );
    let section = PaSection {
        application_paths: vec!["crate::application::*".into()],
        concrete_adapter_patterns: vec!["reqwest::*".into()],
        ..Default::default()
    };
    let diags = pa002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "PA002");
    assert_eq!(diags[0].severity, Severity::Fatal);
    assert!(diags[0].message.contains("reqwest::Client"));
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("crate::application::*"))
    );
}

#[test]
fn pa002_quiet_when_import_outside_application_layer() {
    // Infrastructure layer is allowed to import concrete adapters.
    let air = workspace_with_module(
        "crate::infrastructure::http_client",
        "src/inf.rs",
        vec![import_item("reqwest::Client", "src/inf.rs", 1)],
    );
    let section = PaSection {
        application_paths: vec!["crate::application::*".into()],
        concrete_adapter_patterns: vec!["reqwest::*".into()],
        ..Default::default()
    };
    assert!(pa002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn pa002_silent_when_application_paths_empty() {
    let air = workspace_with_module(
        "crate::application::user",
        "src/app.rs",
        vec![import_item("reqwest::Client", "src/app.rs", 1)],
    );
    let section = PaSection {
        application_paths: vec![],
        concrete_adapter_patterns: vec!["reqwest::*".into()],
        ..Default::default()
    };
    assert!(pa002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn pa002_silent_when_concrete_adapter_patterns_empty() {
    let air = workspace_with_module(
        "crate::application::user",
        "src/app.rs",
        vec![import_item("reqwest::Client", "src/app.rs", 1)],
    );
    let section = PaSection {
        application_paths: vec!["crate::application::*".into()],
        concrete_adapter_patterns: vec![],
        ..Default::default()
    };
    assert!(pa002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn pa002_quiet_when_application_imports_non_adapter_path() {
    let air = workspace_with_module(
        "crate::application::user",
        "src/app.rs",
        vec![import_item("crate::domain::User", "src/app.rs", 1)],
    );
    let section = PaSection {
        application_paths: vec!["crate::application::*".into()],
        concrete_adapter_patterns: vec!["sqlx::*".into(), "reqwest::*".into()],
        ..Default::default()
    };
    assert!(pa002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn pa002_agent_strict_keeps_fatal() {
    let air = workspace_with_module(
        "crate::application::user",
        "src/app.rs",
        vec![import_item("sqlx::PgPool", "src/app.rs", 1)],
    );
    let section = PaSection {
        application_paths: vec!["crate::application::*".into()],
        concrete_adapter_patterns: vec!["sqlx::*".into()],
        ..Default::default()
    };
    let diags = pa002(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ----- PA004 -----

#[test]
fn pa004_fires_when_adapter_constructed_outside_root() {
    let air = workspace_with_module(
        "crate::handler",
        "src/handler.rs",
        vec![construct_action(
            "PgUserRepository",
            "crate::handler::create_user",
            "src/handler.rs",
            12,
        )],
    );
    let section = PaSection {
        adapter_type_patterns: vec!["*::PgUserRepository".into()],
        ..Default::default()
    };
    let diags = pa004(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "PA004");
    assert_eq!(diags[0].severity, Severity::Fatal);
    assert!(diags[0].message.contains("PgUserRepository"));
    assert!(diags[0].message.contains("crate::handler"));
}

#[test]
fn pa004_quiet_when_constructed_inside_default_main() {
    let air = workspace_with_module(
        "crate::main",
        "src/main.rs",
        vec![construct_action(
            "PgUserRepository",
            "crate::main::main",
            "src/main.rs",
            3,
        )],
    );
    let section = PaSection {
        adapter_type_patterns: vec!["*::PgUserRepository".into()],
        ..Default::default()
    };
    assert!(pa004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn pa004_quiet_inside_bootstrap_module() {
    let air = workspace_with_module(
        "crate::bootstrap::wire",
        "src/bootstrap/wire.rs",
        vec![construct_action(
            "PgUserRepository",
            "crate::bootstrap::wire::build",
            "src/bootstrap/wire.rs",
            4,
        )],
    );
    let section = PaSection {
        adapter_type_patterns: vec!["*::PgUserRepository".into()],
        ..Default::default()
    };
    assert!(pa004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn pa004_silent_when_adapter_type_patterns_empty() {
    let air = workspace_with_module(
        "crate::handler",
        "src/handler.rs",
        vec![construct_action(
            "PgUserRepository",
            "crate::handler::create_user",
            "src/handler.rs",
            12,
        )],
    );
    let section = PaSection::default();
    assert!(pa004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn pa004_user_supplied_construction_paths_override_default() {
    // Override the default `*::main` etc. with `crate::wire` only;
    // construction in `main` should now fire.
    let air = workspace_with_module(
        "crate::main",
        "src/main.rs",
        vec![construct_action(
            "PgUserRepository",
            "crate::main::main",
            "src/main.rs",
            3,
        )],
    );
    let section = PaSection {
        adapter_type_patterns: vec!["*::PgUserRepository".into()],
        accepted_construction_paths: vec!["crate::wire".into()],
        ..Default::default()
    };
    let diags = pa004(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
}

#[test]
fn pa004_quiet_when_target_does_not_match_adapter_pattern() {
    let air = workspace_with_module(
        "crate::handler",
        "src/handler.rs",
        vec![construct_action(
            "User",
            "crate::handler::create_user",
            "src/handler.rs",
            7,
        )],
    );
    let section = PaSection {
        adapter_type_patterns: vec!["*::PgUserRepository".into()],
        ..Default::default()
    };
    assert!(pa004(&air, &section, CheckMode::Human).is_empty());
}

// ----- PA003 -----

fn func_item(symbol: &str, file: &str, line: u32) -> AirItem {
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

fn external_io_fact(symbol: &str, evidence: &str, reason: &str) -> AirFact {
    AirFact {
        kind: FactKind::ExternalIo,
        target: FactTarget::Function {
            symbol: symbol.into(),
        },
        source: "test".into(),
        confidence: 1.0,
        reasons: vec![reason.into()],
        evidence: Some(evidence.into()),
    }
}

fn workspace_with_module_facts(
    module_path: &str,
    file: &str,
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
                path: file.into(),
                module_path: Some(module_path.into()),
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
fn pa003_fires_on_external_io_in_application_path() {
    let air = workspace_with_module_facts(
        "crate::application::user",
        "src/app.rs",
        vec![func_item(
            "crate::application::user::create",
            "src/app.rs",
            8,
        )],
        vec![external_io_fact(
            "crate::application::user::create",
            "std::process::Command::output",
            "std::process::Command::output is external IO",
        )],
    );
    let section = PaSection {
        application_paths: vec!["crate::application::*".into()],
        ..Default::default()
    };
    let diags = pa003(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.rule_id, "PA003");
    assert_eq!(d.severity, Severity::Fatal);
    assert_eq!(d.span.line_start, 8);
    assert!(d.message.contains("crate::application::user::create"));
    assert!(d.message.contains("std::process::Command::output"));
    assert!(
        d.why.iter().any(|w| w.contains("crate::application::*")),
        "expected matched pattern in why; got {:?}",
        d.why
    );
    assert!(
        d.why
            .iter()
            .any(|w| w.contains("external IO must be abstracted")),
        "expected port-rationale why; got {:?}",
        d.why
    );
}

#[test]
fn pa003_quiet_when_function_outside_application_paths() {
    let air = workspace_with_module_facts(
        "crate::infrastructure::cmd",
        "src/inf.rs",
        vec![func_item(
            "crate::infrastructure::cmd::run",
            "src/inf.rs",
            4,
        )],
        vec![external_io_fact(
            "crate::infrastructure::cmd::run",
            "std::process::Command::output",
            "external IO",
        )],
    );
    let section = PaSection {
        application_paths: vec!["crate::application::*".into()],
        ..Default::default()
    };
    assert!(pa003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn pa003_quiet_on_other_fact_kinds() {
    let air = workspace_with_module_facts(
        "crate::application::user",
        "src/app.rs",
        vec![func_item(
            "crate::application::user::create",
            "src/app.rs",
            8,
        )],
        vec![
            AirFact {
                kind: FactKind::Logging,
                target: FactTarget::Function {
                    symbol: "crate::application::user::create".into(),
                },
                source: "test".into(),
                confidence: 1.0,
                reasons: Vec::new(),
                evidence: Some("tracing::info!".into()),
            },
            AirFact {
                kind: FactKind::PersistenceWrite,
                target: FactTarget::Function {
                    symbol: "crate::application::user::create".into(),
                },
                source: "test".into(),
                confidence: 1.0,
                reasons: Vec::new(),
                evidence: Some("std::fs::write".into()),
            },
            AirFact {
                kind: FactKind::BlockingCall,
                target: FactTarget::Function {
                    symbol: "crate::application::user::create".into(),
                },
                source: "test".into(),
                confidence: 1.0,
                reasons: Vec::new(),
                evidence: Some("std::thread::sleep".into()),
            },
        ],
    );
    let section = PaSection {
        application_paths: vec!["crate::application::*".into()],
        ..Default::default()
    };
    assert!(pa003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn pa003_silent_when_application_paths_empty() {
    let air = workspace_with_module_facts(
        "crate::application::user",
        "src/app.rs",
        vec![func_item(
            "crate::application::user::create",
            "src/app.rs",
            8,
        )],
        vec![external_io_fact(
            "crate::application::user::create",
            "std::net::TcpStream::connect",
            "external IO",
        )],
    );
    let section = PaSection::default();
    assert!(pa003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn pa003_agent_strict_keeps_fatal() {
    let air = workspace_with_module_facts(
        "crate::application::user",
        "src/app.rs",
        vec![func_item(
            "crate::application::user::create",
            "src/app.rs",
            8,
        )],
        vec![external_io_fact(
            "crate::application::user::create",
            "std::process::Command::output",
            "external IO",
        )],
    );
    let section = PaSection {
        application_paths: vec!["crate::application::*".into()],
        ..Default::default()
    };
    let diags = pa003(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn pa003_per_symbol_test_module_carve_out_via_symbol_match() {
    // The function lives in file `crate::application::user`, and it's
    // also matched by `crate::application::*`. But if the user has a
    // narrower opt-in restricted to non-test paths via a non-overlapping
    // pattern like `crate::application::user::api::*`, the file isn't
    // matched and nothing fires. More important: an inline `tests`
    // module's function symbol embeds `::tests::` — a check against
    // `application_paths` should still match the symbol if the user
    // wrote a pattern like `*::application::*`, but a per-symbol exempt
    // (a `*::tests::*` pattern in `application_paths` would be the
    // wrong direction). Instead, we model "test module under
    // application" by ensuring that when application_paths contains
    // `crate::application::*` and the function symbol is
    // `crate::application::user::tests::it_works` (file module path is
    // `crate::application::user::tests` for an inline mod), the rule
    // still fires — i.e. symbol-anywhere matching reaches inline test
    // modules just like the file path does. The carve-out, when
    // desired, is naturally expressed by *not* including
    // `*::tests::*` style files in `application_paths`.
    let air = workspace_with_module_facts(
        "crate::application::user::tests",
        "src/app.rs",
        vec![func_item(
            "crate::application::user::tests::it_works",
            "src/app.rs",
            30,
        )],
        vec![external_io_fact(
            "crate::application::user::tests::it_works",
            "std::process::Command::output",
            "external IO",
        )],
    );
    // User pinned application paths to non-test sub-paths only — tests
    // module is intentionally excluded.
    let section = PaSection {
        application_paths: vec!["crate::application::user::api::*".into()],
        ..Default::default()
    };
    assert!(
        pa003(&air, &section, CheckMode::Human).is_empty(),
        "tests module not in application_paths must not fire"
    );
}

#[test]
fn pa003_matches_via_function_symbol_when_module_path_is_unrelated() {
    // File-level module_path doesn't match (file is in
    // `crate::other`), but the function symbol embeds the application
    // path because it's an inline submodule. Same fix RW001 already
    // applied.
    let air = workspace_with_module_facts(
        "crate::other",
        "src/other.rs",
        vec![func_item(
            "crate::application::user::create",
            "src/other.rs",
            10,
        )],
        vec![external_io_fact(
            "crate::application::user::create",
            "std::process::Command::output",
            "external IO",
        )],
    );
    let section = PaSection {
        application_paths: vec!["crate::application::*".into()],
        ..Default::default()
    };
    // Symbol-anywhere fallback must catch it.
    let diags = pa003(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1, "symbol fallback should match");
}
