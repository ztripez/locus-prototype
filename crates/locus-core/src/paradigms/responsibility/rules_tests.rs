use super::*;
use locus_air::{
    AIR_SCHEMA_VERSION, AirFile, AirFunction, AirPackage, AirSpan, AirTruthAction, AirWorkspace,
    Visibility,
};

fn action(kind: ActionKind, target: &str, function: &str, file: &str, line: u32) -> AirItem {
    AirItem::TruthAction(AirTruthAction {
        action: kind,
        target: target.into(),
        function: Some(function.into()),
        span: AirSpan::new(file, line, line),
        confidence: 0.9,
        reasons: Vec::new(),
    })
}

fn func(symbol: &str, file: &str, line: u32) -> AirItem {
    AirItem::Function(AirFunction {
        name: symbol.rsplit("::").next().unwrap_or(symbol).into(),
        symbol: symbol.into(),
        visibility: Visibility::Public,
        params: Vec::new(),
        return_type: None,
        span: AirSpan::new(file, line, line + 10),
        line_count: 11,
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        doc: None,
    })
}

fn air_with(files: Vec<(&str, Option<&str>, Vec<AirItem>)>) -> AirWorkspace {
    AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: files
                .into_iter()
                .map(|(path, module, items)| AirFile {
                    path: path.into(),
                    module_path: module.map(|s| s.to_string()),
                    items,
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 50,
                })
                .collect(),
        }],
        facts: Vec::new(),
    }
}

fn enabled_section(cap: u32) -> RmSection {
    RmSection {
        default_max_action_kinds: Some(cap),
        ..RmSection::default()
    }
}

#[test]
fn rm001_fires_on_three_distinct_action_kinds() {
    let air = air_with(vec![(
        "src/handler.rs",
        Some("crate::handler"),
        vec![
            func("crate::handler::do_it", "src/handler.rs", 10),
            action(
                ActionKind::Construct,
                "User",
                "crate::handler::do_it",
                "src/handler.rs",
                11,
            ),
            action(
                ActionKind::Validate,
                "email",
                "crate::handler::do_it",
                "src/handler.rs",
                12,
            ),
            action(
                ActionKind::Normalize,
                "name",
                "crate::handler::do_it",
                "src/handler.rs",
                13,
            ),
        ],
    )]);
    let diags = rm001(&air, &enabled_section(2), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.rule_id, "RM001");
    assert_eq!(d.severity, Severity::Warning);
    assert!(d.message.contains("crate::handler::do_it"));
    assert!(d.message.contains('3'));
    // Span pinned to the function, not the action's line.
    assert_eq!(d.span.line_start, 10);
}

#[test]
fn rm001_quiet_at_or_below_cap() {
    let air = air_with(vec![(
        "src/handler.rs",
        Some("crate::handler"),
        vec![
            func("crate::handler::ok", "src/handler.rs", 10),
            action(
                ActionKind::Construct,
                "User",
                "crate::handler::ok",
                "src/handler.rs",
                11,
            ),
            action(
                ActionKind::Validate,
                "email",
                "crate::handler::ok",
                "src/handler.rs",
                12,
            ),
        ],
    )]);
    assert!(rm001(&air, &enabled_section(2), CheckMode::Human).is_empty());
}

#[test]
fn rm001_quiet_when_module_path_is_exempt() {
    let air = air_with(vec![(
        "src/handler.rs",
        Some("crate::handler::tests"),
        vec![
            func("crate::handler::tests::it_works", "src/handler.rs", 10),
            action(
                ActionKind::Construct,
                "User",
                "crate::handler::tests::it_works",
                "src/handler.rs",
                11,
            ),
            action(
                ActionKind::Validate,
                "email",
                "crate::handler::tests::it_works",
                "src/handler.rs",
                12,
            ),
            action(
                ActionKind::Normalize,
                "name",
                "crate::handler::tests::it_works",
                "src/handler.rs",
                13,
            ),
        ],
    )]);
    let section = RmSection {
        default_max_action_kinds: Some(2),
        exempt_paths: vec!["crate::handler::tests::*".into()],
        ..RmSection::default()
    };
    assert!(rm001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rm001_silent_when_default_max_action_kinds_is_none() {
    let air = air_with(vec![(
        "src/handler.rs",
        Some("crate::handler"),
        vec![
            func("crate::handler::do_it", "src/handler.rs", 10),
            action(
                ActionKind::Construct,
                "User",
                "crate::handler::do_it",
                "src/handler.rs",
                11,
            ),
            action(
                ActionKind::Validate,
                "email",
                "crate::handler::do_it",
                "src/handler.rs",
                12,
            ),
            action(
                ActionKind::Normalize,
                "name",
                "crate::handler::do_it",
                "src/handler.rs",
                13,
            ),
            action(
                ActionKind::DiscriminatedMatch,
                "Status",
                "crate::handler::do_it",
                "src/handler.rs",
                14,
            ),
        ],
    )]);
    // Even though exempt_paths is empty AND there are 4 distinct kinds,
    // an unset default_max_action_kinds means the rule is fully silent.
    let section = RmSection::default();
    assert!(rm001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rm001_one_diagnostic_per_function_regardless_of_action_count() {
    // Five Construct actions + one Validate + one Normalize = 3 distinct
    // kinds. Should fire exactly once for the function, not per action.
    let mut items = vec![func("crate::handler::do_it", "src/handler.rs", 10)];
    for i in 0..5 {
        items.push(action(
            ActionKind::Construct,
            "User",
            "crate::handler::do_it",
            "src/handler.rs",
            11 + i,
        ));
    }
    items.push(action(
        ActionKind::Validate,
        "email",
        "crate::handler::do_it",
        "src/handler.rs",
        20,
    ));
    items.push(action(
        ActionKind::Normalize,
        "name",
        "crate::handler::do_it",
        "src/handler.rs",
        21,
    ));
    let air = air_with(vec![("src/handler.rs", Some("crate::handler"), items)]);
    let diags = rm001(&air, &enabled_section(2), CheckMode::Human);
    assert_eq!(diags.len(), 1, "one diagnostic per function symbol");
}

#[test]
fn rm001_agent_strict_elevates_to_fatal() {
    let air = air_with(vec![(
        "src/handler.rs",
        Some("crate::handler"),
        vec![
            func("crate::handler::do_it", "src/handler.rs", 10),
            action(
                ActionKind::Construct,
                "User",
                "crate::handler::do_it",
                "src/handler.rs",
                11,
            ),
            action(
                ActionKind::Validate,
                "email",
                "crate::handler::do_it",
                "src/handler.rs",
                12,
            ),
            action(
                ActionKind::Normalize,
                "name",
                "crate::handler::do_it",
                "src/handler.rs",
                13,
            ),
        ],
    )]);
    let diags = rm001(&air, &enabled_section(2), CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn rm001_falls_back_to_first_action_span_when_function_not_in_air() {
    // No `AirItem::Function` for `crate::handler::do_it` — simulates an
    // enclosing function that isn't a top-level fn. Diagnostic should
    // still fire and pin to the first action's span.
    let air = air_with(vec![(
        "src/handler.rs",
        Some("crate::handler"),
        vec![
            action(
                ActionKind::Construct,
                "User",
                "crate::handler::do_it",
                "src/handler.rs",
                11,
            ),
            action(
                ActionKind::Validate,
                "email",
                "crate::handler::do_it",
                "src/handler.rs",
                12,
            ),
            action(
                ActionKind::Normalize,
                "name",
                "crate::handler::do_it",
                "src/handler.rs",
                13,
            ),
        ],
    )]);
    let diags = rm001(&air, &enabled_section(2), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].span.line_start, 11);
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("no top-level `AirItem::Function`")),
        "why should explain the fallback; got {:?}",
        diags[0].why
    );
}

#[test]
fn rm001_skips_actions_without_function() {
    // Actions with `function: None` are simply skipped; they shouldn't
    // fold into any group.
    let air = air_with(vec![(
        "src/handler.rs",
        Some("crate::handler"),
        vec![
            AirItem::TruthAction(AirTruthAction {
                action: ActionKind::Construct,
                target: "User".into(),
                function: None,
                span: AirSpan::new("src/handler.rs", 11, 11),
                confidence: 0.9,
                reasons: Vec::new(),
            }),
            AirItem::TruthAction(AirTruthAction {
                action: ActionKind::Validate,
                target: "email".into(),
                function: None,
                span: AirSpan::new("src/handler.rs", 12, 12),
                confidence: 0.9,
                reasons: Vec::new(),
            }),
            AirItem::TruthAction(AirTruthAction {
                action: ActionKind::Normalize,
                target: "name".into(),
                function: None,
                span: AirSpan::new("src/handler.rs", 13, 13),
                confidence: 0.9,
                reasons: Vec::new(),
            }),
        ],
    )]);
    assert!(rm001(&air, &enabled_section(2), CheckMode::Human).is_empty());
}

// ---------- RM002 ----------

fn fact(kind: FactKind, symbol: &str, evidence: &str, reason: &str) -> AirFact {
    AirFact {
        kind,
        target: FactTarget::Function {
            symbol: symbol.into(),
        },
        source: "test".into(),
        confidence: 1.0,
        reasons: vec![reason.into()],
        evidence: Some(evidence.into()),
    }
}

fn air_with_facts(
    files: Vec<(&str, Option<&str>, Vec<AirItem>)>,
    facts: Vec<AirFact>,
) -> AirWorkspace {
    let mut air = air_with(files);
    air.facts = facts;
    air
}

fn converter_section(patterns: Vec<&str>) -> RmSection {
    RmSection {
        converter_paths: patterns.into_iter().map(|s| s.to_string()).collect(),
        ..RmSection::default()
    }
}

#[test]
fn rm002_fires_on_logging_in_converter_module() {
    let air = air_with_facts(
        vec![(
            "src/mapping/user.rs",
            Some("crate::mapping::user"),
            vec![func(
                "crate::mapping::user::to_dto",
                "src/mapping/user.rs",
                7,
            )],
        )],
        vec![fact(
            FactKind::Logging,
            "crate::mapping::user::to_dto",
            "tracing::info!",
            "`tracing::info!` is a logging primitive",
        )],
    );
    let section = converter_section(vec!["crate::mapping::*"]);
    let diags = rm002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.rule_id, "RM002");
    assert_eq!(d.severity, Severity::Warning);
    assert_eq!(d.span.line_start, 7);
    assert!(d.message.contains("crate::mapping::user::to_dto"));
    assert!(d.message.contains("logging"));
    assert!(
        d.why.iter().any(|w| w.contains("crate::mapping::*")),
        "expected matched pattern in why; got {:?}",
        d.why
    );
    assert!(
        d.why.iter().any(|w| w.contains("tracing::info!")),
        "expected evidence in why; got {:?}",
        d.why
    );
    assert!(
        d.why.iter().any(|w| w.contains("logging")),
        "expected fact-kind label in why; got {:?}",
        d.why
    );
    assert!(
        d.why.iter().any(|w| w.contains("to_dto")),
        "expected enclosing function in why; got {:?}",
        d.why
    );
    assert!(
        d.why.iter().any(|w| w.contains("logging primitive")),
        "expected loader reason propagated; got {:?}",
        d.why
    );
}

#[test]
fn rm002_fires_on_spawned_work_in_converter_module() {
    let air = air_with_facts(
        vec![(
            "src/mapping/user.rs",
            Some("crate::mapping::user"),
            vec![func(
                "crate::mapping::user::to_dto",
                "src/mapping/user.rs",
                9,
            )],
        )],
        vec![fact(
            FactKind::SpawnedWork,
            "crate::mapping::user::to_dto",
            "tokio::spawn",
            "spawn-shaped call",
        )],
    );
    let section = converter_section(vec!["crate::mapping::*"]);
    let diags = rm002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "RM002");
    assert!(diags[0].message.contains("spawned-work"));
}

#[test]
fn rm002_fires_on_config_read_in_converter_module() {
    let air = air_with_facts(
        vec![(
            "src/mapping/user.rs",
            Some("crate::mapping::user"),
            vec![func(
                "crate::mapping::user::to_dto",
                "src/mapping/user.rs",
                11,
            )],
        )],
        vec![fact(
            FactKind::ConfigRead,
            "crate::mapping::user::to_dto",
            "std::env::var",
            "env-var read",
        )],
    );
    let section = converter_section(vec!["crate::mapping::*"]);
    let diags = rm002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "RM002");
    assert!(diags[0].message.contains("config-read"));
}

#[test]
fn rm002_quiet_on_non_side_effect_facts_and_non_converter_paths() {
    let air = air_with_facts(
        vec![
            (
                "src/mapping/user.rs",
                Some("crate::mapping::user"),
                vec![func(
                    "crate::mapping::user::to_dto",
                    "src/mapping/user.rs",
                    7,
                )],
            ),
            (
                "src/handler.rs",
                Some("crate::handler"),
                vec![func("crate::handler::create_user", "src/handler.rs", 12)],
            ),
        ],
        vec![
            // Non-side-effect kind targeting a converter — must not fire.
            AirFact {
                kind: FactKind::BlockingCall,
                target: FactTarget::Function {
                    symbol: "crate::mapping::user::to_dto".into(),
                },
                source: "test".into(),
                confidence: 1.0,
                reasons: Vec::new(),
                evidence: Some("std::thread::sleep".into()),
            },
            AirFact {
                kind: FactKind::ExternalIo,
                target: FactTarget::Function {
                    symbol: "crate::mapping::user::to_dto".into(),
                },
                source: "test".into(),
                confidence: 1.0,
                reasons: Vec::new(),
                evidence: Some("reqwest::get".into()),
            },
            // Side-effect kind targeting a non-converter — must not fire.
            fact(
                FactKind::Logging,
                "crate::handler::create_user",
                "tracing::info!",
                "logging primitive",
            ),
        ],
    );
    let section = converter_section(vec!["crate::mapping::*"]);
    assert!(rm002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rm002_silent_when_converter_paths_empty() {
    let air = air_with_facts(
        vec![(
            "src/mapping/user.rs",
            Some("crate::mapping::user"),
            vec![func(
                "crate::mapping::user::to_dto",
                "src/mapping/user.rs",
                7,
            )],
        )],
        vec![fact(
            FactKind::Logging,
            "crate::mapping::user::to_dto",
            "tracing::info!",
            "logging primitive",
        )],
    );
    // Default RmSection has empty converter_paths; rule must be silent
    // even when a side-effect fact is present.
    let section = RmSection::default();
    assert!(rm002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rm002_agent_strict_elevates_to_fatal() {
    let air = air_with_facts(
        vec![(
            "src/mapping/user.rs",
            Some("crate::mapping::user"),
            vec![func(
                "crate::mapping::user::to_dto",
                "src/mapping/user.rs",
                7,
            )],
        )],
        vec![fact(
            FactKind::Logging,
            "crate::mapping::user::to_dto",
            "tracing::info!",
            "logging primitive",
        )],
    );
    let section = converter_section(vec!["crate::mapping::*"]);
    let diags = rm002(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ---------- RM003 ----------

fn handler_section(patterns: Vec<&str>, cap: Option<u32>) -> RmSection {
    RmSection {
        handler_paths: patterns.into_iter().map(|s| s.to_string()).collect(),
        max_handler_decisions: cap,
        ..RmSection::default()
    }
}

#[test]
fn rm003_fires_on_branch_rich_handler() {
    let mut items = vec![func("crate::handler::create_user", "src/handler.rs", 10)];
    for i in 0..4 {
        items.push(action(
            ActionKind::StringCompare,
            "role",
            "crate::handler::create_user",
            "src/handler.rs",
            11 + i,
        ));
    }
    let air = air_with(vec![("src/handler.rs", Some("crate::handler"), items)]);
    let section = handler_section(vec!["crate::handler::*"], Some(3));
    let diags = rm003(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.rule_id, "RM003");
    assert_eq!(d.severity, Severity::Warning);
    assert_eq!(d.span.line_start, 10);
    assert!(d.message.contains("crate::handler::create_user"));
    assert!(d.message.contains("handler"));
    assert!(
        d.why.iter().any(|w| w.contains("handler_paths")),
        "expected handler_paths in why; got {:?}",
        d.why
    );
    assert!(
        d.suggested_fix
            .as_deref()
            .map(|f| f.contains("delegates"))
            .unwrap_or(false),
        "expected handler-flavoured fix; got {:?}",
        d.suggested_fix
    );
}

#[test]
fn rm003_quiet_at_or_below_cap() {
    let mut items = vec![func("crate::handler::small", "src/handler.rs", 4)];
    for i in 0..3 {
        items.push(action(
            ActionKind::DiscriminatedMatch,
            "Status",
            "crate::handler::small",
            "src/handler.rs",
            5 + i,
        ));
    }
    let air = air_with(vec![("src/handler.rs", Some("crate::handler"), items)]);
    let section = handler_section(vec!["crate::handler::*"], Some(3));
    assert!(rm003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rm003_silent_when_handler_paths_empty() {
    let mut items = vec![func("crate::handler::go", "src/handler.rs", 6)];
    for i in 0..6 {
        items.push(action(
            ActionKind::StringCompare,
            "kind",
            "crate::handler::go",
            "src/handler.rs",
            7 + i,
        ));
    }
    let air = air_with(vec![("src/handler.rs", Some("crate::handler"), items)]);
    let section = RmSection::default();
    assert!(rm003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rm003_ignores_non_handler_modules() {
    let mut items = vec![func("crate::domain::go", "src/domain.rs", 6)];
    for i in 0..6 {
        items.push(action(
            ActionKind::StringCompare,
            "kind",
            "crate::domain::go",
            "src/domain.rs",
            7 + i,
        ));
    }
    let air = air_with(vec![("src/domain.rs", Some("crate::domain"), items)]);
    let section = handler_section(vec!["crate::handler::*"], Some(3));
    assert!(rm003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rm003_ignores_non_decision_actions() {
    let mut items = vec![func("crate::handler::go", "src/handler.rs", 6)];
    for i in 0..6 {
        items.push(action(
            ActionKind::Construct,
            "User",
            "crate::handler::go",
            "src/handler.rs",
            7 + i,
        ));
    }
    let air = air_with(vec![("src/handler.rs", Some("crate::handler"), items)]);
    let section = handler_section(vec!["crate::handler::*"], Some(3));
    assert!(rm003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rm003_agent_strict_elevates_to_fatal() {
    let mut items = vec![func("crate::handler::create_user", "src/handler.rs", 10)];
    for i in 0..4 {
        items.push(action(
            ActionKind::StringCompare,
            "role",
            "crate::handler::create_user",
            "src/handler.rs",
            11 + i,
        ));
    }
    let air = air_with(vec![("src/handler.rs", Some("crate::handler"), items)]);
    let section = handler_section(vec!["crate::handler::*"], Some(3));
    let diags = rm003(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ---------- RM004 ----------

fn repository_section(patterns: Vec<&str>, cap: Option<u32>) -> RmSection {
    RmSection {
        repository_paths: patterns.into_iter().map(|s| s.to_string()).collect(),
        max_repository_decisions: cap,
        ..RmSection::default()
    }
}

#[test]
fn rm004_fires_on_branch_rich_repository_function() {
    let mut items = vec![func("crate::repo::find_by", "src/repo.rs", 8)];
    for i in 0..5 {
        items.push(action(
            ActionKind::DiscriminatedMatch,
            "QueryShape",
            "crate::repo::find_by",
            "src/repo.rs",
            9 + i,
        ));
    }
    let air = air_with(vec![("src/repo.rs", Some("crate::repo"), items)]);
    let section = repository_section(vec!["crate::repo::*"], Some(3));
    let diags = rm004(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.rule_id, "RM004");
    assert_eq!(d.severity, Severity::Warning);
    assert_eq!(d.span.line_start, 8);
    assert!(d.message.contains("repository"));
    assert!(
        d.why.iter().any(|w| w.contains("repository_paths")),
        "expected repository_paths in why; got {:?}",
        d.why
    );
    assert!(
        d.suggested_fix
            .as_deref()
            .map(|f| f.contains("Repositories"))
            .unwrap_or(false),
        "expected repository-flavoured fix; got {:?}",
        d.suggested_fix
    );
}

#[test]
fn rm004_quiet_at_or_below_cap() {
    let mut items = vec![func("crate::repo::tiny", "src/repo.rs", 4)];
    for i in 0..3 {
        items.push(action(
            ActionKind::StringCompare,
            "table",
            "crate::repo::tiny",
            "src/repo.rs",
            5 + i,
        ));
    }
    let air = air_with(vec![("src/repo.rs", Some("crate::repo"), items)]);
    let section = repository_section(vec!["crate::repo::*"], Some(3));
    assert!(rm004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rm004_silent_when_repository_paths_empty() {
    let mut items = vec![func("crate::repo::big", "src/repo.rs", 4)];
    for i in 0..6 {
        items.push(action(
            ActionKind::StringCompare,
            "table",
            "crate::repo::big",
            "src/repo.rs",
            5 + i,
        ));
    }
    let air = air_with(vec![("src/repo.rs", Some("crate::repo"), items)]);
    let section = RmSection::default();
    assert!(rm004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rm004_uses_default_cap_when_unset() {
    // Section enabled (repository_paths populated) but max not pinned.
    // Fires above the default of 3.
    let mut items = vec![func("crate::repo::big", "src/repo.rs", 4)];
    for i in 0..4 {
        items.push(action(
            ActionKind::DiscriminatedMatch,
            "Q",
            "crate::repo::big",
            "src/repo.rs",
            5 + i,
        ));
    }
    let air = air_with(vec![("src/repo.rs", Some("crate::repo"), items)]);
    let section = repository_section(vec!["crate::repo::*"], None);
    let diags = rm004(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1, "default cap should be 3");
}

#[test]
fn rm004_agent_strict_elevates_to_fatal() {
    let mut items = vec![func("crate::repo::find_by", "src/repo.rs", 8)];
    for i in 0..5 {
        items.push(action(
            ActionKind::StringCompare,
            "field",
            "crate::repo::find_by",
            "src/repo.rs",
            9 + i,
        ));
    }
    let air = air_with(vec![("src/repo.rs", Some("crate::repo"), items)]);
    let section = repository_section(vec!["crate::repo::*"], Some(3));
    let diags = rm004(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ---------- RM005 ----------

fn validator_section(patterns: Vec<&str>) -> RmSection {
    RmSection {
        validator_paths: patterns.into_iter().map(|s| s.to_string()).collect(),
        ..RmSection::default()
    }
}

#[test]
fn rm005_fires_on_external_io_in_validator_module() {
    let air = air_with_facts(
        vec![(
            "src/validation/email.rs",
            Some("crate::validation::email"),
            vec![func(
                "crate::validation::email::is_valid",
                "src/validation/email.rs",
                7,
            )],
        )],
        vec![fact(
            FactKind::ExternalIo,
            "crate::validation::email::is_valid",
            "reqwest::get",
            "external IO",
        )],
    );
    let section = validator_section(vec!["crate::validation::*"]);
    let diags = rm005(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.rule_id, "RM005");
    assert_eq!(d.severity, Severity::Warning);
    assert_eq!(d.span.line_start, 7);
    assert!(d.message.contains("crate::validation::email::is_valid"));
    assert!(d.message.contains("external-io"));
    assert!(d.message.contains("reqwest::get"));
    assert!(
        d.why.iter().any(|w| w.contains("crate::validation::*")),
        "expected matched pattern in why; got {:?}",
        d.why
    );
    assert!(
        d.why
            .iter()
            .any(|w| w.contains("validation and IO are different")),
        "expected rationale in why; got {:?}",
        d.why
    );
}

#[test]
fn rm005_fires_on_persistence_write_in_validator_module() {
    let air = air_with_facts(
        vec![(
            "src/validation/email.rs",
            Some("crate::validation::email"),
            vec![func(
                "crate::validation::email::is_valid",
                "src/validation/email.rs",
                9,
            )],
        )],
        vec![fact(
            FactKind::PersistenceWrite,
            "crate::validation::email::is_valid",
            "std::fs::write",
            "persistence write",
        )],
    );
    let section = validator_section(vec!["crate::validation::*"]);
    let diags = rm005(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "RM005");
    assert!(diags[0].message.contains("persistence-write"));
}

#[test]
fn rm005_quiet_on_logging_fact() {
    let air = air_with_facts(
        vec![(
            "src/validation/email.rs",
            Some("crate::validation::email"),
            vec![func(
                "crate::validation::email::is_valid",
                "src/validation/email.rs",
                7,
            )],
        )],
        vec![fact(
            FactKind::Logging,
            "crate::validation::email::is_valid",
            "tracing::info!",
            "logging primitive",
        )],
    );
    let section = validator_section(vec!["crate::validation::*"]);
    assert!(rm005(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rm005_silent_when_validator_paths_empty() {
    let air = air_with_facts(
        vec![(
            "src/validation/email.rs",
            Some("crate::validation::email"),
            vec![func(
                "crate::validation::email::is_valid",
                "src/validation/email.rs",
                7,
            )],
        )],
        vec![fact(
            FactKind::ExternalIo,
            "crate::validation::email::is_valid",
            "reqwest::get",
            "external IO",
        )],
    );
    let section = RmSection::default();
    assert!(rm005(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rm005_agent_strict_elevates_to_fatal() {
    let air = air_with_facts(
        vec![(
            "src/validation/email.rs",
            Some("crate::validation::email"),
            vec![func(
                "crate::validation::email::is_valid",
                "src/validation/email.rs",
                7,
            )],
        )],
        vec![fact(
            FactKind::ExternalIo,
            "crate::validation::email::is_valid",
            "reqwest::get",
            "external IO",
        )],
    );
    let section = validator_section(vec!["crate::validation::*"]);
    let diags = rm005(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn rm005_per_symbol_test_module_carve_out() {
    // Function lives in a file whose module_path is the test mod, but
    // user has put `*::tests::*` into validator_paths to flag tests
    // explicitly — should fire. Conversely, when validator_paths
    // doesn't include tests modules and the function symbol contains
    // `::tests::`, it shouldn't fire.
    let air = air_with_facts(
        vec![(
            "src/validation/email.rs",
            Some("crate::validation::email::tests"),
            vec![func(
                "crate::validation::email::tests::it_validates",
                "src/validation/email.rs",
                50,
            )],
        )],
        vec![fact(
            FactKind::PersistenceWrite,
            "crate::validation::email::tests::it_validates",
            "std::fs::write",
            "persistence write",
        )],
    );
    // Narrow patterns that exclude tests submodules — must not fire.
    let section = validator_section(vec!["crate::validation::email::api::*"]);
    assert!(rm005(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rm005_matches_via_function_symbol_when_module_path_unrelated() {
    // File-level `module_path` doesn't match, but function symbol
    // embeds the validator path — symbol fallback should catch it.
    let air = air_with_facts(
        vec![(
            "src/other.rs",
            Some("crate::other"),
            vec![func(
                "crate::validation::email::is_valid",
                "src/other.rs",
                11,
            )],
        )],
        vec![fact(
            FactKind::ExternalIo,
            "crate::validation::email::is_valid",
            "reqwest::get",
            "external IO",
        )],
    );
    let section = validator_section(vec!["crate::validation::*"]);
    let diags = rm005(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
}

// ---------- RM006 ----------

fn domain_rm_section(patterns: Vec<&str>) -> RmSection {
    RmSection {
        domain_paths_rm: patterns.into_iter().map(|s| s.to_string()).collect(),
        ..RmSection::default()
    }
}

#[test]
fn rm006_fires_on_persistence_write_in_domain_method() {
    let air = air_with_facts(
        vec![(
            "src/domain/user.rs",
            Some("crate::domain::user"),
            vec![func(
                "crate::domain::user::User::save",
                "src/domain/user.rs",
                14,
            )],
        )],
        vec![fact(
            FactKind::PersistenceWrite,
            "crate::domain::user::User::save",
            "std::fs::write",
            "persistence write",
        )],
    );
    let section = domain_rm_section(vec!["crate::domain::*"]);
    let diags = rm006(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.rule_id, "RM006");
    assert_eq!(d.severity, Severity::Warning);
    assert_eq!(d.span.line_start, 14);
    assert!(d.message.contains("crate::domain::user::User::save"));
    assert!(
        d.why.iter().any(|w| w.contains("domain_paths_rm")),
        "expected matched pattern label in why; got {:?}",
        d.why
    );
    assert!(
        d.why.iter().any(|w| w.contains("method-shaped")),
        "expected method-shape reason in why; got {:?}",
        d.why
    );
    assert!(
        d.suggested_fix
            .as_deref()
            .map(|f| f.contains("Repository"))
            .unwrap_or(false),
        "expected Repository in fix; got {:?}",
        d.suggested_fix
    );
}

#[test]
fn rm006_quiet_when_target_is_free_function() {
    // Symbol shape `crate::domain::user::save` has no TypeName segment
    // — looks like a free function, so the rule must skip it.
    let air = air_with_facts(
        vec![(
            "src/domain/user.rs",
            Some("crate::domain::user"),
            vec![func("crate::domain::user::save", "src/domain/user.rs", 14)],
        )],
        vec![fact(
            FactKind::PersistenceWrite,
            "crate::domain::user::save",
            "std::fs::write",
            "persistence write",
        )],
    );
    let section = domain_rm_section(vec!["crate::domain::*"]);
    assert!(rm006(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rm006_quiet_on_other_fact_kinds() {
    let air = air_with_facts(
        vec![(
            "src/domain/user.rs",
            Some("crate::domain::user"),
            vec![func(
                "crate::domain::user::User::save",
                "src/domain/user.rs",
                14,
            )],
        )],
        vec![
            fact(
                FactKind::ExternalIo,
                "crate::domain::user::User::save",
                "reqwest::get",
                "external IO",
            ),
            fact(
                FactKind::Logging,
                "crate::domain::user::User::save",
                "tracing::info!",
                "logging primitive",
            ),
        ],
    );
    let section = domain_rm_section(vec!["crate::domain::*"]);
    assert!(rm006(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rm006_silent_when_domain_paths_rm_empty() {
    let air = air_with_facts(
        vec![(
            "src/domain/user.rs",
            Some("crate::domain::user"),
            vec![func(
                "crate::domain::user::User::save",
                "src/domain/user.rs",
                14,
            )],
        )],
        vec![fact(
            FactKind::PersistenceWrite,
            "crate::domain::user::User::save",
            "std::fs::write",
            "persistence write",
        )],
    );
    let section = RmSection::default();
    assert!(rm006(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rm006_agent_strict_elevates_to_fatal() {
    let air = air_with_facts(
        vec![(
            "src/domain/user.rs",
            Some("crate::domain::user"),
            vec![func(
                "crate::domain::user::User::save",
                "src/domain/user.rs",
                14,
            )],
        )],
        vec![fact(
            FactKind::PersistenceWrite,
            "crate::domain::user::User::save",
            "std::fs::write",
            "persistence write",
        )],
    );
    let section = domain_rm_section(vec!["crate::domain::*"]);
    let diags = rm006(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn rm006_looks_like_method_unit_checks() {
    // Free fn `pkg::foo` — too few segments
    assert!(!looks_like_method("pkg::foo"));
    // Free fn `pkg::module::foo` — no TypeName segment
    assert!(!looks_like_method("pkg::module::foo"));
    // Method `pkg::Type::method`
    assert!(looks_like_method("pkg::Type::method"));
    // Method nested deeper `pkg::module::Type::method`
    assert!(looks_like_method("pkg::module::Type::method"));
    // Inline tests mod free fn — lowercase `tests` doesn't trip
    assert!(!looks_like_method("pkg::module::tests::it_works"));
}
