//! Tests for [`super`] rule implementations.
//!
//! Extracted from `rules.rs` to keep the production module within the
//! CX002 line budget. Re-attached via `#[path = "rules_tests.rs"] mod
//! tests;` at the bottom of `rules.rs`.

use super::super::lockfile_schema::{
    default_event_macro_patterns, default_forbidden_log_targets, default_metric_macro_patterns,
};
use super::*;
use locus_air::{
    AIR_SCHEMA_VERSION, AirFile, AirFunction, AirPackage, AirSpan, AirWorkspace, Visibility,
};

fn func(symbol: &str, file: &str, line: u32) -> AirItem {
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

fn log_fact(symbol: &str, evidence: &str, reason: &str) -> AirFact {
    AirFact {
        kind: FactKind::Logging,
        target: FactTarget::Function {
            symbol: symbol.into(),
        },
        source: "test".into(),
        confidence: 1.0,
        reasons: vec![reason.into()],
        evidence: Some(evidence.into()),
    }
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
                module_path: module.map(|m| m.into()),
                items,
                hints: Vec::new(),
                parse_error: None,
                line_count: 1,
            }],
        }],
        facts,
    }
}

/// Onboarded baseline: a single observer pattern that doesn't match any
/// of the test fixture's `x::domain::*` modules. OB stays silent until
/// `observer_paths` is populated (mirrors every other lockfile-driven
/// rule), so tests need at least one observer pattern declared.
fn default_section() -> ObSection {
    ObSection {
        observer_paths: vec!["x::cli::*".into()],
        forbidden_log_targets: default_forbidden_log_targets(),
        ..ObSection::default()
    }
}

#[test]
fn ob001_fires_on_raw_println_in_non_observer_file() {
    let air = air_with(
        Some("x::domain::user"),
        vec![func("x::domain::user::greet", "t.rs", 5)],
        vec![log_fact(
            "x::domain::user::greet",
            "println",
            "`println!` is a raw print/dbg macro",
        )],
    );
    let diags = ob001(&air, &default_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "OB001");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(
        diags[0].message.contains("x::domain::user"),
        "expected module_path in message; got {}",
        diags[0].message
    );
    assert!(
        diags[0].message.contains("greet"),
        "expected function in message; got {}",
        diags[0].message
    );
    assert!(
        diags[0].why.iter().any(|w| w.contains("observer_paths")),
        "expected observer_paths reasoning in why; got {:?}",
        diags[0].why
    );
    assert!(
        diags[0].why.iter().any(|w| w.contains("println")),
        "expected loader reason in why; got {:?}",
        diags[0].why
    );
}

#[test]
fn ob001_quiet_on_non_forbidden_log_targets() {
    // `tracing::info` doesn't match any forbidden_log_targets pattern,
    // so OB001 stays silent — the canonical structured facility.
    let air = air_with(
        Some("x::domain::user"),
        vec![func("x::domain::user::greet", "t.rs", 5)],
        vec![log_fact(
            "x::domain::user::greet",
            "tracing::info",
            "`tracing::info!` is a structured log macro",
        )],
    );
    assert!(ob001(&air, &default_section(), CheckMode::Human).is_empty());
}

#[test]
fn ob001_quiet_on_raw_log_in_observer_path_matching_file() {
    let air = air_with(
        Some("x::cli::main"),
        vec![func("x::cli::main::run", "t.rs", 5)],
        vec![log_fact("x::cli::main::run", "println", "println")],
    );
    let section = ObSection {
        observer_paths: vec!["x::cli::*".into()],
        forbidden_log_targets: default_forbidden_log_targets(),
        ..ObSection::default()
    };
    assert!(ob001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ob001_skips_facts_without_module_path() {
    // Function exists in AIR but its file has no module path — the
    // function lookup misses the module check, the rule stays silent.
    let air = air_with(
        None,
        vec![func("anon::fn", "t.rs", 5)],
        vec![log_fact("anon::fn", "println", "println")],
    );
    assert!(ob001(&air, &default_section(), CheckMode::Human).is_empty());
}

#[test]
fn ob001_agent_strict_elevates_warning_to_fatal() {
    let air = air_with(
        Some("x::domain::user"),
        vec![func("x::domain::user::greet", "t.rs", 5)],
        vec![log_fact("x::domain::user::greet", "println", "println")],
    );
    let diags = ob001(&air, &default_section(), CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn ob001_multiple_raw_log_facts_produce_one_diagnostic_each() {
    let air = air_with(
        Some("x::domain::user"),
        vec![
            func("x::domain::user::greet", "t.rs", 5),
            func("x::domain::user::oops", "t.rs", 12),
            func("x::domain::user::ok", "t.rs", 14),
        ],
        vec![
            log_fact("x::domain::user::greet", "println", "println"),
            log_fact("x::domain::user::greet", "dbg", "dbg"),
            log_fact("x::domain::user::oops", "eprintln", "eprintln"),
            // `tracing::info` is the canonical facility — never flagged
            // because it doesn't match any forbidden_log_targets pattern.
            log_fact("x::domain::user::ok", "tracing::info", "tracing::info"),
        ],
    );
    let diags = ob001(&air, &default_section(), CheckMode::Human);
    assert_eq!(diags.len(), 3);
}

#[test]
fn ob001_silent_when_observer_paths_empty() {
    let air = air_with(
        Some("x::domain::user"),
        vec![func("x::domain::user::greet", "t.rs", 5)],
        vec![log_fact("x::domain::user::greet", "println", "println")],
    );
    let section = ObSection {
        observer_paths: Vec::new(),
        forbidden_log_targets: default_forbidden_log_targets(),
        ..ObSection::default()
    };
    assert!(ob001(&air, &section, CheckMode::Human).is_empty());
}

fn macro_call(callee: &str, function: Option<&str>, line: u32) -> AirItem {
    AirItem::CallSite(AirCallSite {
        callee: callee.into(),
        kind: CallKind::Meta,
        function: function.map(|s| s.to_string()),
        span: AirSpan::new("t.rs", line, line),
    })
}

fn fn_call(callee: &str, function: Option<&str>, line: u32) -> AirItem {
    AirItem::CallSite(AirCallSite {
        callee: callee.into(),
        kind: CallKind::Function,
        function: function.map(|s| s.to_string()),
        span: AirSpan::new("t.rs", line, line),
    })
}

fn air_with_calls(module: &str, items: Vec<AirItem>) -> AirWorkspace {
    air_with(Some(module), items, Vec::new())
}

// ─── OB002 ───────────────────────────────────────────────────────────

#[test]
fn ob002_fires_on_metrics_macro_outside_owner_path() {
    let air = air_with_calls(
        "x::domain::user",
        vec![macro_call(
            "metrics::counter",
            Some("x::domain::user::tick"),
            7,
        )],
    );
    let section = ObSection {
        metric_owner_paths: vec!["x::observability::*".into()],
        metric_macro_patterns: default_metric_macro_patterns(),
        ..ObSection::default()
    };
    let diags = ob002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "OB002");
    assert!(diags[0].message.contains("metrics::counter"));
    assert!(diags[0].message.contains("x::domain::user"));
}

#[test]
fn ob002_quiet_inside_metric_owner_path() {
    let air = air_with_calls(
        "x::observability::metrics",
        vec![macro_call(
            "metrics::counter",
            Some("x::observability::metrics::bump"),
            3,
        )],
    );
    let section = ObSection {
        metric_owner_paths: vec!["x::observability::*".into()],
        metric_macro_patterns: default_metric_macro_patterns(),
        ..ObSection::default()
    };
    assert!(ob002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ob002_silent_when_metric_owner_paths_empty() {
    let air = air_with_calls(
        "x::domain::user",
        vec![macro_call(
            "metrics::counter",
            Some("x::domain::user::tick"),
            7,
        )],
    );
    let section = ObSection::default();
    assert!(ob002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ob002_skips_function_calls() {
    // Function-shaped calls aren't macro emissions even if their text
    // matches a metric macro pattern.
    let air = air_with_calls(
        "x::domain::user",
        vec![fn_call(
            "metrics::counter",
            Some("x::domain::user::tick"),
            7,
        )],
    );
    let section = ObSection {
        metric_owner_paths: vec!["x::observability::*".into()],
        metric_macro_patterns: default_metric_macro_patterns(),
        ..ObSection::default()
    };
    assert!(ob002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ob002_quiet_when_callee_does_not_match_pattern() {
    let air = air_with_calls(
        "x::domain::user",
        vec![macro_call("println", Some("x::domain::user::tick"), 7)],
    );
    let section = ObSection {
        metric_owner_paths: vec!["x::observability::*".into()],
        metric_macro_patterns: default_metric_macro_patterns(),
        ..ObSection::default()
    };
    assert!(ob002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ob002_agent_strict_elevates_to_fatal() {
    let air = air_with_calls(
        "x::domain::user",
        vec![macro_call(
            "metrics::histogram",
            Some("x::domain::user::tick"),
            7,
        )],
    );
    let section = ObSection {
        metric_owner_paths: vec!["x::observability::*".into()],
        metric_macro_patterns: default_metric_macro_patterns(),
        ..ObSection::default()
    };
    let diags = ob002(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ─── OB003 ───────────────────────────────────────────────────────────

#[test]
fn ob003_fires_on_event_macro_outside_owner_path() {
    let air = air_with_calls(
        "x::domain::user",
        vec![macro_call("event", Some("x::domain::user::publish"), 7)],
    );
    let section = ObSection {
        event_owner_paths: vec!["x::observability::events::*".into()],
        event_macro_patterns: default_event_macro_patterns(),
        ..ObSection::default()
    };
    let diags = ob003(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "OB003");
    assert!(diags[0].message.contains("event"));
}

#[test]
fn ob003_quiet_inside_event_owner_path() {
    let air = air_with_calls(
        "x::observability::events::user",
        vec![macro_call(
            "publish",
            Some("x::observability::events::user::send"),
            3,
        )],
    );
    let section = ObSection {
        event_owner_paths: vec!["x::observability::events::*".into()],
        event_macro_patterns: default_event_macro_patterns(),
        ..ObSection::default()
    };
    assert!(ob003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ob003_silent_when_event_owner_paths_empty() {
    let air = air_with_calls(
        "x::domain::user",
        vec![macro_call("event", Some("x::domain::user::publish"), 7)],
    );
    let section = ObSection::default();
    assert!(ob003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ob003_matches_tracing_event_pattern() {
    let air = air_with_calls(
        "x::domain::user",
        vec![macro_call(
            "tracing::event",
            Some("x::domain::user::span"),
            9,
        )],
    );
    let section = ObSection {
        event_owner_paths: vec!["x::observability::events::*".into()],
        event_macro_patterns: default_event_macro_patterns(),
        ..ObSection::default()
    };
    let diags = ob003(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("tracing::event"));
}

#[test]
fn ob003_agent_strict_elevates_to_fatal() {
    let air = air_with_calls(
        "x::domain::user",
        vec![macro_call("emit", Some("x::domain::user::publish"), 7)],
    );
    let section = ObSection {
        event_owner_paths: vec!["x::observability::events::*".into()],
        event_macro_patterns: default_event_macro_patterns(),
        ..ObSection::default()
    };
    let diags = ob003(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ─── OB004 ───────────────────────────────────────────────────────────

fn boundary_entry_marker_fact(symbol: &str) -> AirFact {
    AirFact {
        kind: FactKind::BoundaryEntry,
        target: FactTarget::Function {
            symbol: symbol.into(),
        },
        source: "markers".into(),
        confidence: 1.0,
        reasons: vec!["// locus: fact boundary_entry".into()],
        evidence: None,
    }
}

fn logging_fact(symbol: &str, callee: &str) -> AirFact {
    AirFact {
        kind: FactKind::Logging,
        target: FactTarget::Function {
            symbol: symbol.into(),
        },
        source: "std-rt".into(),
        confidence: 0.9,
        reasons: vec![format!("calls `{callee}!`")],
        evidence: Some(callee.into()),
    }
}

#[test]
fn ob004_fires_when_boundary_entry_has_no_logging() {
    let air = air_with(
        Some("x::api::http"),
        vec![func("x::api::http::handle", "t.rs", 5)],
        vec![boundary_entry_marker_fact("x::api::http::handle")],
    );
    let diags = ob004(&air, &ObSection::default(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "OB004");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(
        diags[0].message.contains("x::api::http::handle"),
        "expected symbol in message; got {}",
        diags[0].message
    );
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("BoundaryEntry") && w.contains("marker")),
        "expected BoundaryEntry marker reason in why; got {:?}",
        diags[0].why
    );
    assert!(
        diags[0].why.iter().any(|w| w.contains("no `Logging` fact")),
        "expected logging-absence reason in why; got {:?}",
        diags[0].why
    );
    assert_eq!(diags[0].span.file, "t.rs");
    assert_eq!(diags[0].span.line_start, 5);
}

#[test]
fn ob004_quiet_when_boundary_entry_has_logging() {
    let air = air_with(
        Some("x::api::http"),
        vec![func("x::api::http::handle", "t.rs", 5)],
        vec![
            boundary_entry_marker_fact("x::api::http::handle"),
            logging_fact("x::api::http::handle", "tracing::info"),
        ],
    );
    assert!(ob004(&air, &ObSection::default(), CheckMode::Human).is_empty());
}

#[test]
fn ob004_quiet_when_only_logging_no_boundary_entry() {
    let air = air_with(
        Some("x::domain::user"),
        vec![func("x::domain::user::greet", "t.rs", 5)],
        vec![logging_fact("x::domain::user::greet", "tracing::info")],
    );
    assert!(ob004(&air, &ObSection::default(), CheckMode::Human).is_empty());
}

#[test]
fn ob004_silent_when_no_boundary_entry_facts_in_workspace() {
    let air = air_with(
        Some("x::domain::user"),
        vec![func("x::domain::user::greet", "t.rs", 5)],
        Vec::new(),
    );
    assert!(ob004(&air, &ObSection::default(), CheckMode::Human).is_empty());
}

#[test]
fn ob004_agent_strict_elevates_warning_to_fatal() {
    let air = air_with(
        Some("x::api::http"),
        vec![func("x::api::http::handle", "t.rs", 5)],
        vec![boundary_entry_marker_fact("x::api::http::handle")],
    );
    let diags = ob004(&air, &ObSection::default(), CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn ob004_multiple_boundary_entries_without_logging_produce_one_each() {
    let air = air_with(
        Some("x::api::http"),
        vec![
            func("x::api::http::create", "t.rs", 5),
            func("x::api::http::update", "t.rs", 12),
            func("x::api::http::delete", "t.rs", 19),
            func("x::api::http::read", "t.rs", 26),
        ],
        vec![
            boundary_entry_marker_fact("x::api::http::create"),
            boundary_entry_marker_fact("x::api::http::update"),
            boundary_entry_marker_fact("x::api::http::delete"),
            // `read` is a boundary entry that DOES log — must be quiet.
            boundary_entry_marker_fact("x::api::http::read"),
            logging_fact("x::api::http::read", "tracing::info"),
        ],
    );
    let diags = ob004(&air, &ObSection::default(), CheckMode::Human);
    assert_eq!(diags.len(), 3);
    let symbols: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert!(symbols.iter().any(|m| m.contains("create")));
    assert!(symbols.iter().any(|m| m.contains("update")));
    assert!(symbols.iter().any(|m| m.contains("delete")));
    assert!(
        !symbols.iter().any(|m| m.contains("::read`")),
        "boundary entry with logging should not be flagged; got {:?}",
        symbols
    );
}
