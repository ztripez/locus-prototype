//! Tests for [`super`] rule implementations.
//!
//! Extracted from `rules.rs` to keep the production module within the
//! CX002 line budget. Re-attached via `#[path = "rules_tests.rs"] mod
//! tests;` at the bottom of `rules.rs`.

use super::*;
use locus_air::{
    AIR_SCHEMA_VERSION, AirFallbackCall, AirFile, AirFunction, AirPackage, AirRetryLoop, AirSpan,
    AirWorkspace, ArmBodyShape, LoopKind, Visibility,
};

fn func(name: &str, return_type: Option<&str>) -> AirItem {
    AirItem::Function(AirFunction {
        name: name.into(),
        symbol: format!("x::domain::user::{name}"),
        visibility: Visibility::Public,
        params: Vec::new(),
        return_type: return_type.map(str::to_string),
        span: AirSpan::new("src/domain/user.rs", 10, 20),
        line_count: 11,
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
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
                path: "src/domain/user.rs".into(),
                module_path: Some(module.into()),
                items,
                hints: Vec::new(),
                parse_error: None,
                line_count: 50,
            }],
        }],
        facts: Vec::new(),
    }
}

fn domain_section() -> FlSection {
    FlSection {
        domain_paths: vec!["x::domain::*".into()],
        boundary_error_patterns: vec!["reqwest::Error".into(), "sqlx::*".into()],
        ..Default::default()
    }
}

// ---- fl001 behavioural tests ----

#[test]
fn fl001_fires_when_domain_function_returns_boundary_error() {
    let air = air_with_module(
        "x::domain::user",
        vec![func("fetch_user", Some("Result<User, reqwest::Error>"))],
    );
    let diags = fl001(&air, &domain_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "FL001");
    assert_eq!(diags[0].severity, Severity::Fatal);
    assert!(diags[0].message.contains("fetch_user"));
    assert!(diags[0].message.contains("reqwest::Error"));
    assert!(diags[0].message.contains("x::domain::*"));
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("Result<User, reqwest::Error>")),
        "why list should surface the full return type; got: {:?}",
        diags[0].why
    );
}

#[test]
fn fl001_fires_via_wildcard_boundary_pattern() {
    // `sqlx::*` matches `sqlx::Error` and any nested type.
    let air = air_with_module(
        "x::domain::orders",
        vec![func(
            "load_order",
            Some("Result<Order, sqlx::postgres::PgError>"),
        )],
    );
    let diags = fl001(&air, &domain_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("sqlx::postgres::PgError"));
    assert!(diags[0].message.contains("sqlx::*"));
}

#[test]
fn fl001_quiet_when_function_returns_domain_error() {
    let air = air_with_module(
        "x::domain::user",
        vec![func("fetch_user", Some("Result<User, UserError>"))],
    );
    assert!(fl001(&air, &domain_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl001_quiet_outside_domain_paths() {
    // Same boundary error type, but the function lives in an adapter
    // module — perfectly legal there.
    let air = air_with_module(
        "x::adapters::http",
        vec![func("fetch_user", Some("Result<User, reqwest::Error>"))],
    );
    assert!(fl001(&air, &domain_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl001_silent_when_lockfile_lists_empty() {
    // Either list empty → rule must be silent. Two cases.
    let air = air_with_module(
        "x::domain::user",
        vec![func("fetch_user", Some("Result<User, reqwest::Error>"))],
    );
    let only_domain = FlSection {
        domain_paths: vec!["x::domain::*".into()],
        boundary_error_patterns: Vec::new(),
        ..Default::default()
    };
    assert!(fl001(&air, &only_domain, CheckMode::Human).is_empty());
    let only_boundary = FlSection {
        domain_paths: Vec::new(),
        boundary_error_patterns: vec!["reqwest::Error".into()],
        ..Default::default()
    };
    assert!(fl001(&air, &only_boundary, CheckMode::Human).is_empty());
    assert!(fl001(&air, &FlSection::default(), CheckMode::Human).is_empty());
}

#[test]
fn fl001_quiet_on_non_result_return_type() {
    let air = air_with_module(
        "x::domain::user",
        vec![
            func("count_users", Some("u64")),
            func("noop", None),
            // Custom `Result<T>` alias with one type parameter — top-level
            // comma absent, so FL001 must skip it (no leakage to detect).
            func("custom_alias", Some("Result<User>")),
        ],
    );
    assert!(fl001(&air, &domain_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl001_handles_generics_in_ok_position() {
    // `Result<HashMap<K, V>, reqwest::Error>` — naive comma split would
    // pick `V>, reqwest::Error` as the error. The angle-bracket-aware
    // splitter must recover the right one.
    let air = air_with_module(
        "x::domain::user",
        vec![func(
            "fetch_users",
            Some("Result<HashMap<UserId, User>, reqwest::Error>"),
        )],
    );
    let diags = fl001(&air, &domain_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(
        diags[0].message.contains("`reqwest::Error`"),
        "extracted error type should be reqwest::Error, not the V>... fragment; got: {}",
        diags[0].message
    );
}

#[test]
fn fl001_emits_one_diag_per_offending_function() {
    let air = air_with_module(
        "x::domain::user",
        vec![
            func("fetch_user", Some("Result<User, reqwest::Error>")),
            func("save_user", Some("Result<(), sqlx::Error>")),
            func("count_users", Some("u64")), // not flagged
            func("ok_user", Some("Result<User, UserError>")), // not flagged
        ],
    );
    let diags = fl001(&air, &domain_section(), CheckMode::Human);
    assert_eq!(diags.len(), 2);
    let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert!(messages.iter().any(|m| m.contains("fetch_user")));
    assert!(messages.iter().any(|m| m.contains("save_user")));
    assert!(!messages.iter().any(|m| m.contains("count_users")));
    assert!(!messages.iter().any(|m| m.contains("ok_user")));
}

#[test]
fn fl001_severity_stays_fatal_in_human_mode() {
    // Unlike most paradigm rules where Human → Warning, FL001 is
    // structural enough that Human and AgentStrict are both Fatal.
    let air = air_with_module(
        "x::domain::user",
        vec![func("fetch_user", Some("Result<User, reqwest::Error>"))],
    );
    let human = fl001(&air, &domain_section(), CheckMode::Human);
    let strict = fl001(&air, &domain_section(), CheckMode::AgentStrict);
    assert_eq!(human.len(), 1);
    assert_eq!(strict.len(), 1);
    assert_eq!(human[0].severity, Severity::Fatal);
    assert_eq!(strict[0].severity, Severity::Fatal);
}

#[test]
fn fl001_skips_files_with_no_module_path() {
    // No `module_path` → can't be matched against `domain_paths`. Stay
    // silent rather than guess from the file path.
    let air = AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: "src/orphan.rs".into(),
                module_path: None,
                items: vec![func("fetch_user", Some("Result<User, reqwest::Error>"))],
                hints: Vec::new(),
                parse_error: None,
                line_count: 5,
            }],
        }],
        facts: Vec::new(),
    };
    assert!(fl001(&air, &domain_section(), CheckMode::Human).is_empty());
}

// ---- extract_result_error_type unit tests ----

#[test]
fn extract_result_error_type_basic() {
    assert_eq!(
        extract_result_error_type("Result<User, reqwest::Error>"),
        Some("reqwest::Error"),
    );
}

#[test]
fn extract_result_error_type_with_generic_ok() {
    assert_eq!(
        extract_result_error_type("Result<HashMap<UserId, User>, reqwest::Error>"),
        Some("reqwest::Error"),
    );
    assert_eq!(
        extract_result_error_type("Result<Vec<(K, V)>, MyError>"),
        Some("MyError"),
    );
}

#[test]
fn extract_result_error_type_rejects_malformed() {
    assert_eq!(extract_result_error_type("u64"), None);
    assert_eq!(extract_result_error_type("Result<User>"), None);
    assert_eq!(extract_result_error_type("Result<User, >"), None);
    assert_eq!(extract_result_error_type("Result<User, MyError"), None);
    // Stray closing bracket inside Ok position trips the depth check
    // before we ever find a top-level comma → rejected.
    assert_eq!(extract_result_error_type("Result<U>>, X>"), None);
}

#[test]
fn extract_result_error_type_strips_leading_double_colon() {
    assert_eq!(
        extract_result_error_type("::Result<User, MyError>"),
        Some("MyError"),
    );
}

#[test]
fn extract_result_error_type_does_not_match_qualified_result() {
    // We deliberately don't try to handle `std::result::Result<...>`. A
    // fully-qualified Result simply isn't matched — false positives on
    // user-defined `Result` aliases would be worse than missing this.
    assert_eq!(
        extract_result_error_type("std::result::Result<User, MyError>"),
        None,
    );
}

// ---- fl002 behavioural tests ----

fn call_site(callee: &str, kind: CallKind, function: Option<&str>, line: u32) -> AirItem {
    AirItem::CallSite(AirCallSite {
        callee: callee.to_string(),
        kind,
        function: function.map(|s| s.to_string()),
        span: AirSpan::new("src/domain/user.rs", line, line),
    })
}

/// Onboarded baseline for FL002: at least one invariant-owner pattern is
/// declared; default `forbidden_callees` covers the unwrap family.
fn fl002_section() -> FlSection {
    FlSection {
        invariant_owner_paths: vec!["x::supervisor::*".into()],
        ..Default::default()
    }
}

/// Onboarded baseline for FL003: at least one invariant-owner pattern is
/// declared; default `silent_discard_callees` covers the `.ok()` family.
fn fl003_section() -> FlSection {
    FlSection {
        invariant_owner_paths: vec!["x::supervisor::*".into()],
        ..Default::default()
    }
}

#[test]
fn fl002_fires_on_unwrap_method_call_in_non_invariant_owner_module() {
    let air = air_with_module(
        "x::domain::user",
        vec![call_site(
            "unwrap",
            CallKind::Method,
            Some("x::domain::user::greet"),
            7,
        )],
    );
    let diags = fl002(&air, &fl002_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "FL002");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].concept.is_none());
    assert_eq!(diags[0].span.line_start, 7);
    assert!(
        diags[0].message.contains("unwrap"),
        "message should surface the callee; got: {}",
        diags[0].message,
    );
    assert!(
        diags[0].message.contains("x::domain::user"),
        "message should surface the module path; got: {}",
        diags[0].message,
    );
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("invariant_owner_paths")),
        "why should reference invariant_owner_paths; got: {:?}",
        diags[0].why,
    );
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("x::domain::user::greet")),
        "why should reference enclosing function; got: {:?}",
        diags[0].why,
    );
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("forbidden_callees") || w.contains("unwrap")),
        "why should reference the forbidden callee; got: {:?}",
        diags[0].why,
    );
}

#[test]
fn fl002_fires_on_panic_macro_call_without_double_colon() {
    // `panic!` invocation: visitor records `callee = "panic"`, `Macro` kind.
    let air = air_with_module(
        "x::domain::user",
        vec![call_site(
            "panic",
            CallKind::Meta,
            Some("x::domain::user::oops"),
            12,
        )],
    );
    let diags = fl002(&air, &fl002_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(
        diags[0].message.contains("panic"),
        "message should mention the panic callee; got: {}",
        diags[0].message,
    );
}

#[test]
fn fl002_quiet_when_callee_is_in_invariant_owner_module() {
    // Same `unwrap` call, but the file's module_path matches an invariant
    // owner pattern — accepted, no diagnostic.
    let air = air_with_module(
        "x::supervisor::startup",
        vec![call_site(
            "unwrap",
            CallKind::Method,
            Some("x::supervisor::startup::boot"),
            3,
        )],
    );
    assert!(fl002(&air, &fl002_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl002_silent_when_invariant_owner_paths_empty() {
    // No `invariant_owner_paths` configured → silent posture, even though
    // `forbidden_callees` defaults are non-empty.
    let air = air_with_module(
        "x::domain::user",
        vec![call_site(
            "unwrap",
            CallKind::Method,
            Some("x::domain::user::greet"),
            7,
        )],
    );
    let section = FlSection::default();
    assert!(
        fl002(&air, &section, CheckMode::Human).is_empty(),
        "rule should wait for explicit invariant_owner_paths declaration",
    );
}

#[test]
fn fl002_agent_strict_elevates_warning_to_fatal() {
    let air = air_with_module(
        "x::domain::user",
        vec![call_site(
            "unwrap",
            CallKind::Method,
            Some("x::domain::user::greet"),
            7,
        )],
    );
    let diags = fl002(&air, &fl002_section(), CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn fl002_quiet_on_function_kind_callees() {
    // Function-shaped calls are intentionally excluded — a user free
    // function named `unwrap` shouldn't trip FL002.
    let air = air_with_module(
        "x::domain::user",
        vec![call_site(
            "unwrap",
            CallKind::Function,
            Some("x::domain::user::greet"),
            7,
        )],
    );
    assert!(fl002(&air, &fl002_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl002_quiet_on_unrelated_callees() {
    let air = air_with_module(
        "x::domain::user",
        vec![call_site(
            "len",
            CallKind::Method,
            Some("x::domain::user::greet"),
            7,
        )],
    );
    assert!(fl002(&air, &fl002_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl002_matches_path_qualified_macro_via_last_segment() {
    // `std::panic!` → callee `"std::panic"`. We match on the last `::`
    // segment, so this should still fire.
    let air = air_with_module(
        "x::domain::user",
        vec![call_site(
            "std::panic",
            CallKind::Meta,
            Some("x::domain::user::oops"),
            7,
        )],
    );
    let diags = fl002(&air, &fl002_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(
        diags[0].message.contains("std::panic"),
        "message should preserve the full callee; got: {}",
        diags[0].message,
    );
}

// ---- fl003 behavioural tests ----

#[test]
fn fl003_fires_on_dot_ok_method_call_in_non_invariant_owner_module() {
    let air = air_with_module(
        "x::domain::user",
        vec![call_site(
            "ok",
            CallKind::Method,
            Some("x::domain::user::greet"),
            12,
        )],
    );
    let diags = fl003(&air, &fl003_section(), CheckMode::Human);
    assert_eq!(
        diags.len(),
        1,
        "expected one FL003 diagnostic; got {diags:?}"
    );
    assert_eq!(diags[0].rule_id, "FL003");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("silent error discard"));
    assert!(diags[0].message.contains("ok"));
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("silent_discard_callees")),
        "why list should reference the lockfile field; got: {:?}",
        diags[0].why,
    );
}

#[test]
fn fl003_fires_on_dot_err_method_call() {
    let air = air_with_module(
        "x::domain::user",
        vec![call_site(
            "err",
            CallKind::Method,
            Some("x::domain::user::lookup"),
            30,
        )],
    );
    let diags = fl003(&air, &fl003_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("err"));
}

#[test]
fn fl003_quiet_on_function_kind_callsites() {
    // A free function literally named `ok` shouldn't trip FL003 — only
    // method calls carry the silent-discard semantics on Result.
    let air = air_with_module(
        "x::domain::user",
        vec![call_site(
            "ok",
            CallKind::Function,
            Some("x::domain::user::greet"),
            7,
        )],
    );
    assert!(fl003(&air, &fl003_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl003_quiet_in_invariant_owner_module() {
    let air = air_with_module(
        "x::supervisor::root",
        vec![call_site(
            "ok",
            CallKind::Method,
            Some("x::supervisor::root::run"),
            4,
        )],
    );
    assert!(fl003(&air, &fl003_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl003_silent_when_invariant_owner_paths_empty() {
    let air = air_with_module(
        "x::domain::user",
        vec![call_site(
            "ok",
            CallKind::Method,
            Some("x::domain::user::greet"),
            12,
        )],
    );
    assert!(
        fl003(&air, &FlSection::default(), CheckMode::Human).is_empty(),
        "rule should wait for explicit invariant_owner_paths declaration",
    );
}

#[test]
fn fl003_silent_when_silent_discard_callees_empty() {
    let air = air_with_module(
        "x::domain::user",
        vec![call_site(
            "ok",
            CallKind::Method,
            Some("x::domain::user::greet"),
            12,
        )],
    );
    let section = FlSection {
        invariant_owner_paths: vec!["x::supervisor::*".into()],
        silent_discard_callees: Vec::new(),
        ..Default::default()
    };
    assert!(fl003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn fl003_quiet_on_unrelated_callees() {
    let air = air_with_module(
        "x::domain::user",
        vec![
            call_site("len", CallKind::Method, Some("x::domain::user::greet"), 4),
            call_site("clone", CallKind::Method, Some("x::domain::user::greet"), 5),
        ],
    );
    assert!(fl003(&air, &fl003_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl003_agent_strict_elevates_warning_to_fatal() {
    let air = air_with_module(
        "x::domain::user",
        vec![call_site(
            "ok",
            CallKind::Method,
            Some("x::domain::user::greet"),
            12,
        )],
    );
    let diags = fl003(&air, &fl003_section(), CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ---- fl004 + fl005 helpers ----

fn discard(
    callee: Option<&str>,
    kind: locus_air::DiscardKind,
    function: Option<&str>,
    line: u32,
) -> AirItem {
    AirItem::SilentDiscard(locus_air::AirSilentDiscard {
        callee: callee.map(|s| s.to_string()),
        kind,
        function: function.map(|s| s.to_string()),
        span: AirSpan::new("src/domain/user.rs", line, line),
    })
}

fn partial_if_let(variant: &str, function: Option<&str>, line: u32) -> AirItem {
    // AIR v13: variant is a typed enum, not a String. Map the
    // legacy "Ok"/"Err" test fixture vocabulary to the new
    // ResultMatchVariant.
    let variant = match variant {
        "Ok" => locus_air::ResultMatchVariant::Success,
        "Err" => locus_air::ResultMatchVariant::Failure,
        other => panic!("test fixture passed unexpected variant: {other}"),
    };
    AirItem::PartialResultMatch(locus_air::AirPartialResultMatch {
        variant,
        function: function.map(|s| s.to_string()),
        span: AirSpan::new("src/domain/user.rs", line, line),
    })
}

/// Onboarded baseline for FL004 / FL005 — same shape as the FL003
/// baseline; both rules consult `invariant_owner_paths` only and
/// rely on the seeded `silent_discard_allowed_callees` defaults.
fn fl004_section() -> FlSection {
    FlSection {
        invariant_owner_paths: vec!["x::supervisor::*".into()],
        ..Default::default()
    }
}

// ---- fl004 behavioural tests ----

#[test]
fn fl004_fires_on_discarded_method_call_in_non_invariant_owner_module() {
    let air = air_with_module(
        "x::domain::user",
        vec![discard(
            Some("write"),
            locus_air::DiscardKind::Method,
            Some("x::domain::user::greet"),
            17,
        )],
    );
    let diags = fl004(&air, &fl004_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "FL004");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("write"));
    assert!(diags[0].message.contains("let _ ="));
}

#[test]
fn fl004_quiet_on_allowlist_callees() {
    // `lock`, `send`, `drop`, `set_logger`, `subscribe`, `try_init`
    // are seeded as legitimate fire-and-forget patterns.
    let air = air_with_module(
        "x::domain::user",
        vec![
            discard(
                Some("lock"),
                locus_air::DiscardKind::Method,
                Some("x::domain::user::greet"),
                1,
            ),
            discard(
                Some("send"),
                locus_air::DiscardKind::Method,
                Some("x::domain::user::greet"),
                2,
            ),
            discard(
                Some("drop"),
                locus_air::DiscardKind::Function,
                Some("x::domain::user::greet"),
                3,
            ),
        ],
    );
    assert!(fl004(&air, &fl004_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl004_quiet_on_other_kind_discards() {
    // `let _ = some_field;` is `DiscardKind::Other` — we don't flag
    // arbitrary expression discards (false-positive surface too large).
    let air = air_with_module(
        "x::domain::user",
        vec![discard(
            None,
            locus_air::DiscardKind::Other,
            Some("x::domain::user::greet"),
            4,
        )],
    );
    assert!(fl004(&air, &fl004_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl004_quiet_in_invariant_owner_module() {
    let air = air_with_module(
        "x::supervisor::root",
        vec![discard(
            Some("write"),
            locus_air::DiscardKind::Method,
            Some("x::supervisor::root::run"),
            17,
        )],
    );
    assert!(fl004(&air, &fl004_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl004_silent_when_invariant_owner_paths_empty() {
    let air = air_with_module(
        "x::domain::user",
        vec![discard(
            Some("write"),
            locus_air::DiscardKind::Method,
            Some("x::domain::user::greet"),
            17,
        )],
    );
    assert!(fl004(&air, &FlSection::default(), CheckMode::Human).is_empty());
}

#[test]
fn fl004_agent_strict_elevates_warning_to_fatal() {
    let air = air_with_module(
        "x::domain::user",
        vec![discard(
            Some("write"),
            locus_air::DiscardKind::Method,
            Some("x::domain::user::greet"),
            17,
        )],
    );
    let diags = fl004(&air, &fl004_section(), CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ---- fl005 behavioural tests ----

#[test]
fn fl005_fires_on_partial_if_let_ok_in_non_invariant_owner_module() {
    let air = air_with_module(
        "x::domain::user",
        vec![partial_if_let("Ok", Some("x::domain::user::greet"), 22)],
    );
    let diags = fl005(&air, &fl004_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "FL005");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("if let Ok"));
    assert!(diags[0].message.contains("Err"));
}

#[test]
fn fl005_fires_on_partial_if_let_err() {
    let air = air_with_module(
        "x::domain::user",
        vec![partial_if_let("Err", Some("x::domain::user::greet"), 22)],
    );
    let diags = fl005(&air, &fl004_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("if let Err"));
    assert!(diags[0].message.contains("Ok"));
}

#[test]
fn fl005_quiet_in_invariant_owner_module() {
    let air = air_with_module(
        "x::supervisor::root",
        vec![partial_if_let("Ok", Some("x::supervisor::root::run"), 22)],
    );
    assert!(fl005(&air, &fl004_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl005_silent_when_invariant_owner_paths_empty() {
    let air = air_with_module(
        "x::domain::user",
        vec![partial_if_let("Ok", Some("x::domain::user::greet"), 22)],
    );
    assert!(fl005(&air, &FlSection::default(), CheckMode::Human).is_empty());
}

#[test]
fn fl005_agent_strict_elevates_warning_to_fatal() {
    let air = air_with_module(
        "x::domain::user",
        vec![partial_if_let("Ok", Some("x::domain::user::greet"), 22)],
    );
    let diags = fl005(&air, &fl004_section(), CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ---- fl013 behavioural tests ----

/// Onboarded baseline for FL013 — same shape as fl003/fl004 baselines.
fn fl013_section() -> FlSection {
    FlSection {
        invariant_owner_paths: vec!["x::supervisor::*".into()],
        ..Default::default()
    }
}

#[test]
fn fl013_fires_when_string_result_fn_calls_to_string() {
    // `fn save() -> Result<(), String>` body contains `.to_string()`.
    let air = air_with_module(
        "x::domain::user",
        vec![
            func("save", Some("Result<(), String>")),
            call_site(
                "to_string",
                CallKind::Method,
                Some("x::domain::user::save"),
                14,
            ),
        ],
    );
    let diags = fl013(&air, &fl013_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected one FL013 diag, got {diags:?}");
    assert_eq!(diags[0].rule_id, "FL013");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert_eq!(diags[0].span.line_start, 14);
    assert!(diags[0].message.contains("save"));
    assert!(diags[0].message.contains("to_string"));
    assert!(diags[0].message.contains("Result<(), String>"));
    assert!(
        diags[0]
            .suggested_fix
            .as_deref()
            .unwrap_or("")
            .contains("?"),
        "suggested fix should mention `?` propagation; got: {:?}",
        diags[0].suggested_fix,
    );
}

#[test]
fn fl013_fires_on_format_macro_in_str_result() {
    // `fn save() -> Result<(), &str>` body contains `format!(...)`.
    let air = air_with_module(
        "x::domain::user",
        vec![
            func("describe", Some("Result<String, &str>")),
            call_site(
                "format",
                CallKind::Meta,
                Some("x::domain::user::describe"),
                7,
            ),
        ],
    );
    let diags = fl013(&air, &fl013_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("describe"));
    assert!(diags[0].message.contains("format"));
}

#[test]
fn fl013_quiet_when_error_type_is_typed() {
    // `Result<_, MyError>` is fine — even if the function calls
    // `.to_string()` somewhere internally, FL013 doesn't apply.
    let air = air_with_module(
        "x::domain::user",
        vec![
            func("greet", Some("Result<(), MyError>")),
            call_site(
                "to_string",
                CallKind::Method,
                Some("x::domain::user::greet"),
                3,
            ),
        ],
    );
    assert!(fl013(&air, &fl013_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl013_quiet_when_string_fn_has_no_stringification_call() {
    // Function returns `Result<_, String>` but its body has only a
    // `.len()` call — no FL013 trigger.
    let air = air_with_module(
        "x::domain::user",
        vec![
            func("save", Some("Result<(), String>")),
            call_site("len", CallKind::Method, Some("x::domain::user::save"), 4),
        ],
    );
    assert!(fl013(&air, &fl013_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl013_quiet_in_invariant_owner_module() {
    // Same offending pattern, but the file is on the invariant-owner
    // allowlist (file_module matches `x::supervisor::*`). FL013 must
    // suppress the diagnostic. We build the AIR explicitly so the
    // module_path and the enclosing-fn symbols line up under
    // `x::supervisor::*` (the shared `func` helper hardcodes a
    // domain-shaped symbol prefix).
    let air = AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: "src/supervisor/root.rs".into(),
                module_path: Some("x::supervisor::root".into()),
                items: vec![
                    AirItem::Function(AirFunction {
                        name: "save".into(),
                        symbol: "x::supervisor::root::save".into(),
                        visibility: Visibility::Public,
                        params: Vec::new(),
                        return_type: Some("Result<(), String>".into()),
                        span: AirSpan::new("src/supervisor/root.rs", 1, 5),
                        line_count: 5,
                        decorators: Vec::new(),
                        symbol_segments: Vec::new(),
                        doc: None,
                    }),
                    AirItem::CallSite(AirCallSite {
                        callee: "to_string".into(),
                        kind: CallKind::Method,
                        function: Some("x::supervisor::root::save".into()),
                        span: AirSpan::new("src/supervisor/root.rs", 3, 3),
                    }),
                ],
                hints: Vec::new(),
                parse_error: None,
                line_count: 5,
            }],
        }],
        facts: Vec::new(),
    };
    assert!(fl013(&air, &fl013_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl013_silent_when_invariant_owner_paths_empty() {
    let air = air_with_module(
        "x::domain::user",
        vec![
            func("save", Some("Result<(), String>")),
            call_site(
                "to_string",
                CallKind::Method,
                Some("x::domain::user::save"),
                14,
            ),
        ],
    );
    assert!(fl013(&air, &FlSection::default(), CheckMode::Human).is_empty());
}

#[test]
fn fl013_agent_strict_elevates_warning_to_fatal() {
    let air = air_with_module(
        "x::domain::user",
        vec![
            func("save", Some("Result<(), String>")),
            call_site(
                "to_string",
                CallKind::Method,
                Some("x::domain::user::save"),
                14,
            ),
        ],
    );
    let diags = fl013(&air, &fl013_section(), CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ---- fl006 / fl007 / fl011 helpers ----

fn closure_method_call(
    callee: &str,
    discards: bool,
    body_shape: ArmBodyShape,
    function: Option<&str>,
    line: u32,
) -> AirItem {
    AirItem::ClosureMethodCall(AirClosureMethodCall {
        callee: callee.to_string(),
        closure_discards_arg: discards,
        body_shape,
        function: function.map(|s| s.to_string()),
        span: AirSpan::new("src/domain/user.rs", line, line),
    })
}

fn match_arm(
    scrutinee: &str,
    pattern: &str,
    has_wildcard: bool,
    body_shape: ArmBodyShape,
    function: Option<&str>,
    line: u32,
) -> AirItem {
    AirItem::MatchArm(AirMatchArm {
        scrutinee: scrutinee.to_string(),
        pattern: pattern.to_string(),
        pattern_has_wildcard: has_wildcard,
        body_shape,
        function: function.map(|s| s.to_string()),
        span: AirSpan::new("src/domain/user.rs", line, line),
    })
}

/// Onboarded baseline — same shape as fl003/fl004/fl005 baselines;
/// FL006/FL007/FL011 all consult `invariant_owner_paths` only.
fn fl_arm_section() -> FlSection {
    FlSection {
        invariant_owner_paths: vec!["x::supervisor::*".into()],
        ..Default::default()
    }
}

// ---- fl006 behavioural tests ----

#[test]
fn fl006_fires_on_map_err_with_underscore_closure_in_non_invariant_owner_module() {
    let air = air_with_module(
        "x::domain::user",
        vec![closure_method_call(
            "map_err",
            true,
            ArmBodyShape::Call,
            Some("x::domain::user::greet"),
            42,
        )],
    );
    let diags = fl006(&air, &fl_arm_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected one FL006 diag, got {diags:?}");
    assert_eq!(diags[0].rule_id, "FL006");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].concept.is_none());
    assert_eq!(diags[0].span.line_start, 42);
    assert!(
        diags[0].message.contains("map_err"),
        "message should surface the callee; got: {}",
        diags[0].message,
    );
    assert!(diags[0].message.contains("x::domain::user"));
    assert!(diags[0].message.contains("x::domain::user::greet"));
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("closure pattern is `_`")),
        "why list should call out the discarded closure arg; got: {:?}",
        diags[0].why,
    );
}

#[test]
fn fl006_fires_inside_inline_test_module_when_test_paths_not_in_invariant_owners() {
    // Inline `mod tests { fn x() { result.map_err(|_| ()); } }` —
    // file `module_path` is `x::domain::user`, function symbol is
    // `x::domain::user::tests::it_works`. `containing_module_of`
    // strips the last segment → `x::domain::user::tests`, which
    // doesn't match the supervisor-only baseline → FL006 still fires.
    let air = air_with_module(
        "x::domain::user",
        vec![closure_method_call(
            "map_err",
            true,
            ArmBodyShape::Call,
            Some("x::domain::user::tests::it_works"),
            7,
        )],
    );
    let diags = fl006(&air, &fl_arm_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    // Now make the test module an invariant owner — diagnostic disappears.
    let owner_section = FlSection {
        invariant_owner_paths: vec!["*::tests".into()],
        ..Default::default()
    };
    assert!(fl006(&air, &owner_section, CheckMode::Human).is_empty());
}

#[test]
fn fl006_quiet_when_closure_uses_its_arg() {
    // `result.map_err(|e| MyError::from(e))` — `closure_discards_arg`
    // is false. No FL006.
    let air = air_with_module(
        "x::domain::user",
        vec![closure_method_call(
            "map_err",
            false,
            ArmBodyShape::Call,
            Some("x::domain::user::greet"),
            42,
        )],
    );
    assert!(fl006(&air, &fl_arm_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl006_quiet_when_callee_is_not_map_err() {
    // `unwrap_or_else(|_| ...)` is FL003's territory; FL006 must
    // ignore non-`map_err` callees regardless of closure shape.
    let air = air_with_module(
        "x::domain::user",
        vec![
            closure_method_call(
                "unwrap_or_else",
                true,
                ArmBodyShape::Call,
                Some("x::domain::user::greet"),
                1,
            ),
            closure_method_call(
                "or_else",
                true,
                ArmBodyShape::Call,
                Some("x::domain::user::greet"),
                2,
            ),
            closure_method_call(
                "and_then",
                true,
                ArmBodyShape::Call,
                Some("x::domain::user::greet"),
                3,
            ),
        ],
    );
    assert!(fl006(&air, &fl_arm_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl006_silent_when_invariant_owner_paths_empty() {
    let air = air_with_module(
        "x::domain::user",
        vec![closure_method_call(
            "map_err",
            true,
            ArmBodyShape::Call,
            Some("x::domain::user::greet"),
            42,
        )],
    );
    assert!(fl006(&air, &FlSection::default(), CheckMode::Human).is_empty());
}

#[test]
fn fl006_quiet_in_invariant_owner_module() {
    let air = air_with_module(
        "x::supervisor::root",
        vec![closure_method_call(
            "map_err",
            true,
            ArmBodyShape::Call,
            Some("x::supervisor::root::run"),
            7,
        )],
    );
    assert!(fl006(&air, &fl_arm_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl006_agent_strict_elevates_warning_to_fatal() {
    let air = air_with_module(
        "x::domain::user",
        vec![closure_method_call(
            "map_err",
            true,
            ArmBodyShape::Call,
            Some("x::domain::user::greet"),
            42,
        )],
    );
    let diags = fl006(&air, &fl_arm_section(), CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ---- fl007 behavioural tests ----

#[test]
fn fl007_fires_on_err_underscore_arm_with_literal_body() {
    // `match result { Ok(x) => x, Err(_) => 0 }` — Err arm body is
    // `Literal`, FL007 fires.
    let air = air_with_module(
        "x::domain::user",
        vec![match_arm(
            "result",
            "Err(_)",
            true,
            ArmBodyShape::Literal,
            Some("x::domain::user::lookup"),
            12,
        )],
    );
    let diags = fl007(&air, &fl_arm_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected one FL007 diag, got {diags:?}");
    assert_eq!(diags[0].rule_id, "FL007");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].concept.is_none());
    assert!(diags[0].message.contains("Err(_)"));
    assert!(diags[0].message.contains("silently swallows"));
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("arm pattern `Err(_)`")),
        "why should surface the pattern text; got: {:?}",
        diags[0].why,
    );
}

#[test]
fn fl007_fires_on_err_underscore_arm_with_call_body() {
    // `Err(_) => Default::default()` — body shape is `Call`. Still silent.
    let air = air_with_module(
        "x::domain::user",
        vec![match_arm(
            "result",
            "Err(_)",
            true,
            ArmBodyShape::Call,
            Some("x::domain::user::lookup"),
            4,
        )],
    );
    let diags = fl007(&air, &fl_arm_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("call expression"));
}

#[test]
fn fl007_quiet_when_arm_body_propagates() {
    // `Err(e) => return Err(e.into())` — well, propagation by `?` is
    // the canonical example. Body shape is `Propagate`; FL007 stays
    // quiet because the arm has explicitly handled the failure.
    let air = air_with_module(
        "x::domain::user",
        vec![match_arm(
            "result",
            "Err(_)",
            true,
            ArmBodyShape::ErrorPropagation,
            Some("x::domain::user::lookup"),
            4,
        )],
    );
    assert!(fl007(&air, &fl_arm_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl007_quiet_when_arm_body_returns() {
    // `Err(_) => return None` — body is `Return`. Control flow leaves
    // the function explicitly, so the failure has an owner.
    let air = air_with_module(
        "x::domain::user",
        vec![match_arm(
            "result",
            "Err(_)",
            true,
            ArmBodyShape::Return,
            Some("x::domain::user::lookup"),
            4,
        )],
    );
    assert!(fl007(&air, &fl_arm_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl007_quiet_when_pattern_does_not_target_err() {
    // `Ok(_) => 0` — wildcard binder is present but the pattern
    // doesn't match `Err`. FL011 reasons about `_` patterns; FL007
    // is the `Err`-specific rule and must not fire here.
    let air = air_with_module(
        "x::domain::user",
        vec![match_arm(
            "result",
            "Ok(_)",
            true,
            ArmBodyShape::Literal,
            Some("x::domain::user::lookup"),
            4,
        )],
    );
    assert!(fl007(&air, &fl_arm_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl007_silent_when_invariant_owner_paths_empty() {
    let air = air_with_module(
        "x::domain::user",
        vec![match_arm(
            "result",
            "Err(_)",
            true,
            ArmBodyShape::Literal,
            Some("x::domain::user::lookup"),
            12,
        )],
    );
    assert!(fl007(&air, &FlSection::default(), CheckMode::Human).is_empty());
}

#[test]
fn fl007_agent_strict_elevates_warning_to_fatal() {
    let air = air_with_module(
        "x::domain::user",
        vec![match_arm(
            "result",
            "Err(_)",
            true,
            ArmBodyShape::Literal,
            Some("x::domain::user::lookup"),
            12,
        )],
    );
    let diags = fl007(&air, &fl_arm_section(), CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ---- fl011 behavioural tests ----

#[test]
fn fl011_fires_on_bare_wildcard_arm_with_literal_body() {
    // `match status { Status::A => 1, _ => 0 }` — bare `_` arm body
    // is `Literal`. FL011 fires.
    let air = air_with_module(
        "x::domain::user",
        vec![match_arm(
            "status",
            "_",
            true,
            ArmBodyShape::Literal,
            Some("x::domain::user::classify"),
            9,
        )],
    );
    let diags = fl011(&air, &fl_arm_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected one FL011 diag, got {diags:?}");
    assert_eq!(diags[0].rule_id, "FL011");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].concept.is_none());
    assert!(diags[0].message.contains("bare `_` arm"));
    assert!(diags[0].message.contains("literal default"));
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("scrutinee `status`")),
        "why should surface the scrutinee; got: {:?}",
        diags[0].why,
    );
}

#[test]
fn fl011_fires_on_bare_wildcard_arm_with_call_body() {
    // `_ => Default::default()` — body shape `Call`. Still silent.
    let air = air_with_module(
        "x::domain::user",
        vec![match_arm(
            "status",
            "_",
            true,
            ArmBodyShape::Call,
            Some("x::domain::user::classify"),
            3,
        )],
    );
    let diags = fl011(&air, &fl_arm_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("call expression"));
}

#[test]
fn fl011_quiet_on_err_underscore_pattern() {
    // `Err(_)` is FL007's territory. FL011 must ignore patterns that
    // aren't the bare `_`.
    let air = air_with_module(
        "x::domain::user",
        vec![match_arm(
            "result",
            "Err(_)",
            true,
            ArmBodyShape::Literal,
            Some("x::domain::user::lookup"),
            4,
        )],
    );
    assert!(fl011(&air, &fl_arm_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl011_quiet_when_arm_body_returns() {
    // `_ => return None` — body is `Return`. Explicit handling, no FL011.
    let air = air_with_module(
        "x::domain::user",
        vec![match_arm(
            "status",
            "_",
            true,
            ArmBodyShape::Return,
            Some("x::domain::user::classify"),
            3,
        )],
    );
    assert!(fl011(&air, &fl_arm_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl011_quiet_when_arm_body_is_block() {
    // Multi-statement block — could be doing real work. FL011 doesn't
    // pre-judge.
    let air = air_with_module(
        "x::domain::user",
        vec![match_arm(
            "status",
            "_",
            true,
            ArmBodyShape::Block,
            Some("x::domain::user::classify"),
            3,
        )],
    );
    assert!(fl011(&air, &fl_arm_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl011_silent_when_invariant_owner_paths_empty() {
    let air = air_with_module(
        "x::domain::user",
        vec![match_arm(
            "status",
            "_",
            true,
            ArmBodyShape::Literal,
            Some("x::domain::user::classify"),
            9,
        )],
    );
    assert!(fl011(&air, &FlSection::default(), CheckMode::Human).is_empty());
}

#[test]
fn fl011_agent_strict_elevates_warning_to_fatal() {
    let air = air_with_module(
        "x::domain::user",
        vec![match_arm(
            "status",
            "_",
            true,
            ArmBodyShape::Literal,
            Some("x::domain::user::classify"),
            9,
        )],
    );
    let diags = fl011(&air, &fl_arm_section(), CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ---- fl010 / fl012 helpers ----

fn fallback_call(callee: &str, shape: ArmBodyShape, function: Option<&str>, line: u32) -> AirItem {
    AirItem::FallbackCall(AirFallbackCall {
        pattern: locus_air::FallbackPattern::ValueOr,
        callee: callee.to_string(),
        default_shape: shape,
        function: function.map(|s| s.to_string()),
        span: AirSpan::new("src/domain/user.rs", line, line),
    })
}

fn retry_loop(
    kind: LoopKind,
    propagates: bool,
    has_break: bool,
    function: Option<&str>,
    line: u32,
) -> AirItem {
    AirItem::RetryLoop(AirRetryLoop {
        loop_kind: kind,
        propagates,
        has_break,
        function: function.map(|s| s.to_string()),
        span: AirSpan::new("src/domain/user.rs", line, line),
    })
}

/// Onboarded baseline for FL012 — populates `retry_policy_owner_paths`
/// with a single supervisor pattern. Mirrors `fl_arm_section`'s shape.
fn fl012_section() -> FlSection {
    FlSection {
        retry_policy_owner_paths: vec!["x::retry::*".into()],
        ..Default::default()
    }
}

// ---- fl010 behavioural tests ----

#[test]
fn fl010_fires_on_unwrap_or_with_literal_default() {
    // `result.unwrap_or(0)` outside any invariant owner.
    let air = air_with_module(
        "x::domain::user",
        vec![fallback_call(
            "unwrap_or",
            ArmBodyShape::Literal,
            Some("x::domain::user::greet"),
            42,
        )],
    );
    let diags = fl010(&air, &fl_arm_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected one FL010 diag, got {diags:?}");
    assert_eq!(diags[0].rule_id, "FL010");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].concept.is_none());
    assert_eq!(diags[0].span.line_start, 42);
    assert!(
        diags[0].message.contains(".unwrap_or(...)"),
        "message should surface the callee; got: {}",
        diags[0].message,
    );
    assert!(diags[0].message.contains("silent literal default"));
    assert!(diags[0].message.contains("x::domain::user"));
    assert!(diags[0].message.contains("x::domain::user::greet"));
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("callee `unwrap_or`")),
        "why list should record the callee; got: {:?}",
        diags[0].why,
    );
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("default-arg shape: literal default")),
        "why list should record the default-arg shape; got: {:?}",
        diags[0].why,
    );
}

#[test]
fn fl010_fires_on_or_with_call_default() {
    // `option.or(Vec::new())` — callee `or`, shape `Call`. Still silent.
    let air = air_with_module(
        "x::domain::user",
        vec![fallback_call(
            "or",
            ArmBodyShape::Call,
            Some("x::domain::user::collect"),
            7,
        )],
    );
    let diags = fl010(&air, &fl_arm_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains(".or(...)"));
    assert!(diags[0].message.contains("silent call default"));
}

#[test]
fn fl010_quiet_on_unwrap_or_default_no_arg_form() {
    // `unwrap_or_default()` (Empty) is FL002's territory via the
    // default `forbidden_callees` list. FL010 must skip it even
    // when the callee string slips through here.
    let air = air_with_module(
        "x::domain::user",
        vec![
            fallback_call(
                "unwrap_or_default",
                ArmBodyShape::Empty,
                Some("x::domain::user::greet"),
                1,
            ),
            // And FL010 also shouldn't fire when default_shape is Empty
            // even on `unwrap_or` (defensive — the visitor wouldn't
            // emit this combination, but the rule must still skip).
            fallback_call(
                "unwrap_or",
                ArmBodyShape::Empty,
                Some("x::domain::user::greet"),
                2,
            ),
        ],
    );
    assert!(fl010(&air, &fl_arm_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl010_quiet_when_default_shape_is_block_or_other() {
    // Multi-statement fallback block / unrecognised shape — could be
    // doing real recovery work. FL010 stays conservative.
    let air = air_with_module(
        "x::domain::user",
        vec![
            fallback_call(
                "unwrap_or",
                ArmBodyShape::Block,
                Some("x::domain::user::greet"),
                1,
            ),
            fallback_call(
                "unwrap_or",
                ArmBodyShape::Other,
                Some("x::domain::user::greet"),
                2,
            ),
            fallback_call(
                "unwrap_or",
                ArmBodyShape::Return,
                Some("x::domain::user::greet"),
                3,
            ),
            fallback_call(
                "unwrap_or",
                ArmBodyShape::ErrorPropagation,
                Some("x::domain::user::greet"),
                4,
            ),
        ],
    );
    assert!(fl010(&air, &fl_arm_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl010_silent_when_invariant_owner_paths_empty() {
    let air = air_with_module(
        "x::domain::user",
        vec![fallback_call(
            "unwrap_or",
            ArmBodyShape::Literal,
            Some("x::domain::user::greet"),
            42,
        )],
    );
    assert!(fl010(&air, &FlSection::default(), CheckMode::Human).is_empty());
}

#[test]
fn fl010_agent_strict_elevates_warning_to_fatal() {
    let air = air_with_module(
        "x::domain::user",
        vec![fallback_call(
            "unwrap_or",
            ArmBodyShape::Literal,
            Some("x::domain::user::greet"),
            42,
        )],
    );
    let diags = fl010(&air, &fl_arm_section(), CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn fl010_per_symbol_carve_out_for_inline_test_modules() {
    // File `module_path` is `x::domain::user` (doesn't match
    // `*::tests::*`). The function symbol is
    // `x::domain::user::tests::it_works` whose containing module is
    // `x::domain::user::tests` — that's what `*::tests::*` carves out.
    let air = air_with_module(
        "x::domain::user",
        vec![fallback_call(
            "unwrap_or",
            ArmBodyShape::Literal,
            Some("x::domain::user::tests::it_works"),
            7,
        )],
    );
    // Without the carve-out, FL010 fires.
    let diags = fl010(&air, &fl_arm_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    // With `*::tests::*` carved out, it disappears.
    let owner_section = FlSection {
        invariant_owner_paths: vec!["*::tests::*".into()],
        ..Default::default()
    };
    assert!(fl010(&air, &owner_section, CheckMode::Human).is_empty());
}

// ---- fl012 behavioural tests ----

#[test]
fn fl012_fires_on_loop_with_propagation_and_break() {
    // `loop { try_thing()?; if ok { break; } }` outside any retry-policy owner.
    let air = air_with_module(
        "x::domain::user",
        vec![retry_loop(
            LoopKind::Loop,
            true,
            true,
            Some("x::domain::user::poll"),
            33,
        )],
    );
    let diags = fl012(&air, &fl012_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected one FL012 diag, got {diags:?}");
    assert_eq!(diags[0].rule_id, "FL012");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].concept.is_none());
    assert_eq!(diags[0].span.line_start, 33);
    assert!(
        diags[0].message.contains("retry-shaped loop loop"),
        "message should surface the loop kind; got: {}",
        diags[0].message,
    );
    assert!(diags[0].message.contains("x::domain::user"));
    assert!(diags[0].message.contains("x::domain::user::poll"));
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("uses `?` and contains `break`")),
        "why should explain the retry shape; got: {:?}",
        diags[0].why,
    );
}

#[test]
fn fl012_fires_on_for_loop_with_propagation_and_break() {
    // `for _ in 0..N { try_thing()?; if maybe { break; } }`.
    let air = air_with_module(
        "x::domain::user",
        vec![retry_loop(
            LoopKind::For,
            true,
            true,
            Some("x::domain::user::poll"),
            3,
        )],
    );
    let diags = fl012(&air, &fl012_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("retry-shaped for loop"));
    assert!(
        diags[0].why.iter().any(|w| w.contains("loop kind: `for`")),
        "why should record the loop kind; got: {:?}",
        diags[0].why,
    );
}

#[test]
fn fl012_quiet_when_loop_propagates_but_has_no_break() {
    // `for x in xs { do_thing(x)?; }` — propagates but never breaks.
    // Just an iterator, not a retry.
    let air = air_with_module(
        "x::domain::user",
        vec![retry_loop(
            LoopKind::For,
            true,
            false,
            Some("x::domain::user::process"),
            3,
        )],
    );
    assert!(fl012(&air, &fl012_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl012_quiet_when_loop_breaks_but_has_no_propagation() {
    // `loop { if cond { break; } }` — breaks but no `?`. No fallible op.
    let air = air_with_module(
        "x::domain::user",
        vec![retry_loop(
            LoopKind::Loop,
            false,
            true,
            Some("x::domain::user::wait"),
            3,
        )],
    );
    assert!(fl012(&air, &fl012_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl012_quiet_in_declared_retry_policy_owner() {
    // `x::retry::backoff` matches the `x::retry::*` owner pattern.
    let air = air_with_module(
        "x::retry::backoff",
        vec![retry_loop(
            LoopKind::Loop,
            true,
            true,
            Some("x::retry::backoff::run"),
            3,
        )],
    );
    assert!(fl012(&air, &fl012_section(), CheckMode::Human).is_empty());
}

#[test]
fn fl012_silent_when_retry_policy_owner_paths_empty() {
    let air = air_with_module(
        "x::domain::user",
        vec![retry_loop(
            LoopKind::Loop,
            true,
            true,
            Some("x::domain::user::poll"),
            3,
        )],
    );
    assert!(fl012(&air, &FlSection::default(), CheckMode::Human).is_empty());
}

#[test]
fn fl012_agent_strict_elevates_warning_to_fatal() {
    let air = air_with_module(
        "x::domain::user",
        vec![retry_loop(
            LoopKind::While,
            true,
            true,
            Some("x::domain::user::poll"),
            33,
        )],
    );
    let diags = fl012(&air, &fl012_section(), CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
    assert!(diags[0].message.contains("retry-shaped while loop"));
}
