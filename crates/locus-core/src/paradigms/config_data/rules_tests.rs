//! Tests for [`super`] rule implementations.
//!
//! Extracted from `rules.rs` to keep the production module within the
//! CX002 line budget. Re-attached via `#[path = "rules_tests.rs"] mod
//! tests;` at the bottom of `rules.rs`.

use super::*;
use locus_air::{
    AIR_SCHEMA_VERSION, AirFile, AirFunction, AirPackage, AirScrutineeLiteral, AirSpan,
    AirWorkspace, LiteralContext, LiteralKind, Visibility,
};

fn func(symbol: &str, line: u32) -> AirItem {
    AirItem::Function(AirFunction {
        name: symbol.rsplit("::").next().unwrap_or(symbol).into(),
        symbol: symbol.into(),
        visibility: Visibility::Public,
        params: Vec::new(),
        return_type: None,
        span: AirSpan::new("t.rs", line, line + 5),
        line_count: 6,
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        doc: None,
    })
}

fn env_fact(symbol: &str, reason: &str) -> AirFact {
    AirFact {
        kind: FactKind::ConfigRead,
        target: FactTarget::Function {
            symbol: symbol.into(),
        },
        source: "test".into(),
        confidence: 1.0,
        reasons: vec![reason.into()],
        evidence: Some("std::env::var".into()),
    }
}

fn scrutinee_literal(
    value: &str,
    kind: LiteralKind,
    context: LiteralContext,
    function: Option<&str>,
    line: u32,
) -> AirItem {
    AirItem::ScrutineeLiteral(AirScrutineeLiteral {
        value: value.into(),
        kind,
        context,
        function: function.map(|s| s.to_string()),
        span: AirSpan::new("t.rs", line, line),
    })
}

fn air_with(module: Option<&str>, items: Vec<AirItem>, facts: Vec<AirFact>) -> AirWorkspace {
    AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: "t.rs".into(),
                module_path: module.map(|s| s.to_string()),
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
fn cf001_fires_when_env_read_in_non_config_file() {
    let air = air_with(
        Some("crate::handler::user"),
        vec![func("crate::handler::user::resolve_db", 12)],
        vec![env_fact(
            "crate::handler::user::resolve_db",
            "`std::env::var` reads an env var",
        )],
    );
    let section = CfSection {
        config_paths: vec!["crate::config::*".into()],
        ..Default::default()
    };
    let diags = cf001(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "CF001");
    assert_eq!(diags[0].severity, Severity::Fatal);
    assert!(diags[0].message.contains("crate::handler::user"));
    assert!(diags[0].message.contains("resolve_db"));
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("config_paths") && w.contains("crate::handler::user")),
        "expected module-vs-config_paths reason in why; got {:?}",
        diags[0].why
    );
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("env var") || w.contains("env-read")),
        "expected loader reason in why; got {:?}",
        diags[0].why
    );
    assert!(
        diags[0].why.iter().any(|w| w.contains("resolve_db")),
        "expected enclosing function in why; got {:?}",
        diags[0].why
    );
}

#[test]
fn cf001_quiet_when_env_read_in_config_pattern_file() {
    let air = air_with(
        Some("crate::config::loader"),
        vec![func("crate::config::loader::load", 10)],
        vec![env_fact("crate::config::loader::load", "env read")],
    );
    let section = CfSection {
        config_paths: vec!["crate::config::*".into()],
        ..Default::default()
    };
    assert!(cf001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cf001_quiet_on_non_readsenv_facts() {
    let air = air_with(
        Some("crate::handler::user"),
        vec![func("crate::handler::user::create", 20)],
        vec![
            AirFact {
                kind: FactKind::SpawnedWork,
                target: FactTarget::Function {
                    symbol: "crate::handler::user::create".into(),
                },
                source: "test".into(),
                confidence: 1.0,
                reasons: Vec::new(),
                evidence: None,
            },
            AirFact {
                kind: FactKind::Logging,
                target: FactTarget::Function {
                    symbol: "crate::handler::user::create".into(),
                },
                source: "test".into(),
                confidence: 1.0,
                reasons: Vec::new(),
                evidence: None,
            },
        ],
    );
    let section = CfSection {
        config_paths: vec!["crate::config::*".into()],
        ..Default::default()
    };
    assert!(cf001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cf001_silent_when_config_paths_empty() {
    let air = air_with(
        Some("crate::handler::user"),
        vec![func("crate::handler::user::resolve_db", 12)],
        vec![env_fact("crate::handler::user::resolve_db", "env read")],
    );
    let section = CfSection::default();
    assert!(cf001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cf001_skips_files_without_module_path() {
    // A file the adapter couldn't resolve to a module path can't be
    // judged against config_paths — skip it rather than firing
    // spuriously. The function lookup walks AIR — if no file with a
    // module path holds the function, the lookup misses and the rule
    // stays silent.
    let air = air_with(
        None,
        vec![func("anonymous::resolve", 12)],
        vec![env_fact("anonymous::resolve", "env read")],
    );
    let section = CfSection {
        config_paths: vec!["crate::config::*".into()],
        ..Default::default()
    };
    assert!(cf001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cf001_agent_strict_keeps_severity_fatal() {
    // CF001 is already Fatal in human mode; --agent-strict elevates but
    // can't go higher than Fatal — verify it stays Fatal, not panicked.
    let air = air_with(
        Some("crate::handler::user"),
        vec![func("crate::handler::user::call", 30)],
        vec![env_fact("crate::handler::user::call", "env read")],
    );
    let section = CfSection {
        config_paths: vec!["crate::config::*".into()],
        ..Default::default()
    };
    let diags = cf001(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn cf001_skips_facts_whose_function_isnt_in_air() {
    // A loader can produce a fact for a function the AIR doesn't carry
    // (e.g. external crate). CF001 has nothing to evaluate — skip
    // rather than panic.
    let air = air_with(
        Some("crate::handler::user"),
        Vec::new(), // no functions
        vec![env_fact("crate::other::resolve_db", "env read")],
    );
    let section = CfSection {
        config_paths: vec!["crate::config::*".into()],
        ..Default::default()
    };
    assert!(cf001(&air, &section, CheckMode::Human).is_empty());
}

// ---- CF002: magic decision constant in scrutinee ----

#[test]
fn cf002_fires_on_str_match_arm_outside_config_paths() {
    let air = air_with(
        Some("crate::handler::user"),
        vec![scrutinee_literal(
            "\"active\"",
            LiteralKind::Str,
            LiteralContext::MatchArm,
            Some("crate::handler::user::route"),
            42,
        )],
        Vec::new(),
    );
    let section = CfSection {
        config_paths: vec!["crate::config::*".into()],
        ..Default::default()
    };
    let diags = cf002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1, "diags = {:?}", diags);
    assert_eq!(diags[0].rule_id, "CF002");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("\"active\""));
    assert!(diags[0].message.contains("crate::handler::user"));
    assert!(diags[0].message.contains("route"));
    assert!(
        diags[0].why.iter().any(|w| w.contains("MatchArm")),
        "expected context in why; got {:?}",
        diags[0].why
    );
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("config_paths") && w.contains("crate::handler::user")),
        "expected gating reason in why; got {:?}",
        diags[0].why
    );
}

#[test]
fn cf002_fires_on_int_binary_compare_outside_config_paths() {
    let air = air_with(
        Some("crate::handler::user"),
        vec![scrutinee_literal(
            "2",
            LiteralKind::Int,
            LiteralContext::BinaryCompare,
            Some("crate::handler::user::pick"),
            10,
        )],
        Vec::new(),
    );
    let section = CfSection {
        config_paths: vec!["crate::config::*".into()],
        ..Default::default()
    };
    let diags = cf002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "CF002");
    assert!(diags[0].message.contains("magic int literal"));
    assert!(diags[0].message.contains('2'));
    assert!(
        diags[0].why.iter().any(|w| w.contains("BinaryCompare")),
        "expected context in why; got {:?}",
        diags[0].why
    );
}

#[test]
fn cf002_quiet_inside_config_paths() {
    let air = air_with(
        Some("crate::config::loader"),
        vec![scrutinee_literal(
            "\"active\"",
            LiteralKind::Str,
            LiteralContext::MatchArm,
            Some("crate::config::loader::pick"),
            10,
        )],
        Vec::new(),
    );
    let section = CfSection {
        config_paths: vec!["crate::config::*".into()],
        ..Default::default()
    };
    assert!(cf002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cf002_quiet_for_bool_literals() {
    // Default `forbidden_literal_kinds` excludes `bool`; `if x ==
    // true` patterns are noise, not a magic decision constant.
    let air = air_with(
        Some("crate::handler::user"),
        vec![scrutinee_literal(
            "true",
            LiteralKind::Bool,
            LiteralContext::BinaryCompare,
            Some("crate::handler::user::flag"),
            15,
        )],
        Vec::new(),
    );
    let section = CfSection {
        config_paths: vec!["crate::config::*".into()],
        ..Default::default()
    };
    assert!(cf002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cf002_silent_when_config_paths_empty() {
    let air = air_with(
        Some("crate::handler::user"),
        vec![scrutinee_literal(
            "\"active\"",
            LiteralKind::Str,
            LiteralContext::MatchArm,
            Some("crate::handler::user::route"),
            10,
        )],
        Vec::new(),
    );
    let section = CfSection::default(); // empty config_paths
    assert!(cf002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cf002_silent_when_forbidden_literal_kinds_empty() {
    // User can disable CF002 without touching config_paths.
    let air = air_with(
        Some("crate::handler::user"),
        vec![scrutinee_literal(
            "\"active\"",
            LiteralKind::Str,
            LiteralContext::MatchArm,
            Some("crate::handler::user::route"),
            10,
        )],
        Vec::new(),
    );
    let section = CfSection {
        config_paths: vec!["crate::config::*".into()],
        forbidden_literal_kinds: Vec::new(),
        ..Default::default()
    };
    assert!(cf002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cf002_agent_strict_elevates_warning_to_fatal() {
    let air = air_with(
        Some("crate::handler::user"),
        vec![scrutinee_literal(
            "\"active\"",
            LiteralKind::Str,
            LiteralContext::MatchArm,
            Some("crate::handler::user::route"),
            10,
        )],
        Vec::new(),
    );
    let section = CfSection {
        config_paths: vec!["crate::config::*".into()],
        ..Default::default()
    };
    let diags = cf002(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn cf002_user_can_narrow_to_strings_only() {
    // Narrow `forbidden_literal_kinds` to `["str"]` and integer
    // thresholds stop firing.
    let air = air_with(
        Some("crate::handler::user"),
        vec![
            scrutinee_literal(
                "\"active\"",
                LiteralKind::Str,
                LiteralContext::MatchArm,
                Some("crate::handler::user::route"),
                10,
            ),
            scrutinee_literal(
                "2",
                LiteralKind::Int,
                LiteralContext::BinaryCompare,
                Some("crate::handler::user::pick"),
                20,
            ),
        ],
        Vec::new(),
    );
    let section = CfSection {
        config_paths: vec!["crate::config::*".into()],
        forbidden_literal_kinds: vec!["str".into()],
        ..Default::default()
    };
    let diags = cf002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("\"active\""));
}

// ---- CF003: hardcoded provider/model/topic ID ----

#[test]
fn cf003_fires_on_gpt_pattern_in_binary_compare() {
    let air = air_with(
        Some("crate::handler::chat"),
        vec![scrutinee_literal(
            "\"gpt-4o\"",
            LiteralKind::Str,
            LiteralContext::BinaryCompare,
            Some("crate::handler::chat::pick_model"),
            12,
        )],
        Vec::new(),
    );
    let section = CfSection {
        config_paths: vec!["crate::config::*".into()],
        forbidden_id_patterns: vec!["gpt-*".into()],
        ..Default::default()
    };
    let diags = cf003(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1, "diags = {:?}", diags);
    assert_eq!(diags[0].rule_id, "CF003");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("\"gpt-4o\""));
    assert!(diags[0].message.contains("gpt-*"));
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("forbidden_id_patterns")),
        "expected forbidden_id_patterns reason in why; got {:?}",
        diags[0].why
    );
}

#[test]
fn cf003_fires_on_queue_pattern_in_match_arm() {
    let air = air_with(
        Some("crate::handler::worker"),
        vec![scrutinee_literal(
            "\"queue-events\"",
            LiteralKind::Str,
            LiteralContext::MatchArm,
            Some("crate::handler::worker::dispatch"),
            30,
        )],
        Vec::new(),
    );
    let section = CfSection {
        config_paths: vec!["crate::config::*".into()],
        forbidden_id_patterns: vec!["queue-*".into()],
        ..Default::default()
    };
    let diags = cf003(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("\"queue-events\""));
    assert!(diags[0].message.contains("queue-*"));
}

#[test]
fn cf003_quiet_when_value_matches_no_pattern() {
    let air = air_with(
        Some("crate::handler::chat"),
        vec![scrutinee_literal(
            "\"some-other-id\"",
            LiteralKind::Str,
            LiteralContext::BinaryCompare,
            Some("crate::handler::chat::pick"),
            12,
        )],
        Vec::new(),
    );
    let section = CfSection {
        config_paths: vec!["crate::config::*".into()],
        forbidden_id_patterns: vec!["gpt-*".into(), "claude-*".into()],
        ..Default::default()
    };
    assert!(cf003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cf003_silent_when_forbidden_id_patterns_empty() {
    let air = air_with(
        Some("crate::handler::chat"),
        vec![scrutinee_literal(
            "\"gpt-4o\"",
            LiteralKind::Str,
            LiteralContext::BinaryCompare,
            Some("crate::handler::chat::pick"),
            12,
        )],
        Vec::new(),
    );
    let section = CfSection {
        config_paths: vec!["crate::config::*".into()],
        ..Default::default() // forbidden_id_patterns empty
    };
    assert!(cf003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cf003_silent_when_config_paths_empty() {
    let air = air_with(
        Some("crate::handler::chat"),
        vec![scrutinee_literal(
            "\"gpt-4o\"",
            LiteralKind::Str,
            LiteralContext::BinaryCompare,
            Some("crate::handler::chat::pick"),
            12,
        )],
        Vec::new(),
    );
    let section = CfSection {
        forbidden_id_patterns: vec!["gpt-*".into()],
        ..Default::default()
    };
    assert!(cf003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cf003_agent_strict_elevates_warning_to_fatal() {
    let air = air_with(
        Some("crate::handler::chat"),
        vec![scrutinee_literal(
            "\"gpt-4o\"",
            LiteralKind::Str,
            LiteralContext::BinaryCompare,
            Some("crate::handler::chat::pick"),
            12,
        )],
        Vec::new(),
    );
    let section = CfSection {
        config_paths: vec!["crate::config::*".into()],
        forbidden_id_patterns: vec!["gpt-*".into()],
        ..Default::default()
    };
    let diags = cf003(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn cf003_strips_string_quotes_before_pattern_match() {
    // Headline check: literal value `"\"gpt-4\""` (with surrounding
    // quote chars preserved by the AIR visitor) must match pattern
    // `"gpt-*"` (which is segment-aware against the *unquoted*
    // value).
    let air = air_with(
        Some("crate::handler::chat"),
        vec![scrutinee_literal(
            "\"gpt-4\"",
            LiteralKind::Str,
            LiteralContext::BinaryCompare,
            Some("crate::handler::chat::pick"),
            12,
        )],
        Vec::new(),
    );
    let section = CfSection {
        config_paths: vec!["crate::config::*".into()],
        forbidden_id_patterns: vec!["gpt-*".into()],
        ..Default::default()
    };
    let diags = cf003(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1, "diags = {:?}", diags);
}

#[test]
fn cf003_quiet_inside_config_paths() {
    // An ID literal inside the declared config layer is fine.
    let air = air_with(
        Some("crate::config::models"),
        vec![scrutinee_literal(
            "\"gpt-4\"",
            LiteralKind::Str,
            LiteralContext::BinaryCompare,
            Some("crate::config::models::pick"),
            12,
        )],
        Vec::new(),
    );
    let section = CfSection {
        config_paths: vec!["crate::config::*".into()],
        forbidden_id_patterns: vec!["gpt-*".into()],
        ..Default::default()
    };
    assert!(cf003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cf003_skips_non_string_literals() {
    // CF003 is string-shaped IDs only; numeric literals belong to
    // CF002 territory.
    let air = air_with(
        Some("crate::handler::chat"),
        vec![scrutinee_literal(
            "42",
            LiteralKind::Int,
            LiteralContext::BinaryCompare,
            Some("crate::handler::chat::pick"),
            12,
        )],
        Vec::new(),
    );
    let section = CfSection {
        config_paths: vec!["crate::config::*".into()],
        forbidden_id_patterns: vec!["*".into()],
        ..Default::default()
    };
    assert!(cf003(&air, &section, CheckMode::Human).is_empty());
}

// ---- Lockfile schema round-trip ----

#[test]
fn cf_section_lockfile_fields_round_trip_through_serde() {
    // Users can pre-populate every CF lockfile field today.
    // The defaults survive a serde round-trip; partial JSON falls
    // back to the seeded patterns / defaults.
    let s = CfSection::default();
    assert!(!s.config_file_patterns.is_empty());
    assert!(!s.accepted_config_files.is_empty());
    assert_eq!(
        s.forbidden_literal_kinds,
        vec!["str".to_string(), "int".to_string(), "float".to_string()]
    );
    assert!(s.forbidden_id_patterns.is_empty());

    let j = serde_json::to_value(&s).unwrap();
    let back: CfSection = serde_json::from_value(j).unwrap();
    assert_eq!(s, back);

    let from_empty: CfSection = serde_json::from_str("{}").unwrap();
    assert_eq!(from_empty, CfSection::default());
}
