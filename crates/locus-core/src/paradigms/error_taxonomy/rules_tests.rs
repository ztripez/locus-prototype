//! Tests for [`super`] rule implementations.
//!
//! Extracted from `rules.rs` to keep the production module within the
//! CX002 line budget. Re-attached via `#[path = "rules_tests.rs"] mod
//! tests;` at the bottom of `rules.rs`.

use super::*;
use locus_air::{
    AIR_SCHEMA_VERSION, AirFile, AirItem, AirPackage, AirSpan, AirType, AirWorkspace, TypeKind,
    Visibility,
};

fn ty(name: &str, visibility: Visibility) -> AirItem {
    AirItem::Type(AirType {
        kind: TypeKind::Enum,
        name: name.into(),
        symbol: format!("crate::errors::{name}"),
        visibility,
        fields: Vec::new(),
        variants: Vec::new(),
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        span: AirSpan::new("src/errors.rs", 1, 1),
        doc: None,
    })
}

fn pub_ty(name: &str) -> AirItem {
    ty(name, Visibility::Public)
}

fn air_with_file_items(file_path: &str, items: Vec<AirItem>) -> AirWorkspace {
    AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: file_path.into(),
                module_path: Some("crate".into()),
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
fn er001_fires_when_file_has_two_error_types() {
    let air = air_with_file_items(
        "src/errors.rs",
        vec![pub_ty("UserError"), pub_ty("CreateUserError")],
    );
    let diags = er001(&air, &ErSection::default(), CheckMode::Human);
    assert_eq!(
        diags.len(),
        1,
        "two error types → one diagnostic on the second"
    );
    assert_eq!(diags[0].rule_id, "ER001");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(
        diags[0].message.contains("CreateUserError"),
        "should flag the non-incumbent; got: {}",
        diags[0].message
    );
    assert!(
        diags[0].message.contains("UserError"),
        "should reference the incumbent; got: {}",
        diags[0].message
    );
    assert!(
        diags[0]
            .why
            .iter()
            .any(|line| line.contains("UserError") && line.contains("CreateUserError")),
        "why list should enumerate every error type in the file; got: {:?}",
        diags[0].why
    );
}

#[test]
fn er001_emits_one_diag_per_extra_error_type() {
    let air = air_with_file_items(
        "src/errors.rs",
        vec![
            pub_ty("UserError"),
            pub_ty("CreateUserError"),
            pub_ty("UserServiceError"),
        ],
    );
    let diags = er001(&air, &ErSection::default(), CheckMode::Human);
    assert_eq!(
        diags.len(),
        2,
        "three error types → two duplicate diagnostics"
    );
    assert!(diags.iter().all(|d| d.rule_id == "ER001"));
    // Each extra error type gets flagged; the incumbent (UserError) is not.
    let flagged: Vec<&str> = diags
        .iter()
        .map(|d| {
            if d.message.contains("CreateUserError") {
                "CreateUserError"
            } else if d.message.contains("UserServiceError") {
                "UserServiceError"
            } else {
                "(unknown)"
            }
        })
        .collect();
    assert!(flagged.contains(&"CreateUserError"));
    assert!(flagged.contains(&"UserServiceError"));
}

#[test]
fn er001_quiet_when_file_has_one_error_type() {
    let air = air_with_file_items("src/errors.rs", vec![pub_ty("UserError")]);
    assert!(er001(&air, &ErSection::default(), CheckMode::Human).is_empty());
}

#[test]
fn er001_quiet_when_file_has_zero_error_types() {
    let air = air_with_file_items(
        "src/model.rs",
        vec![pub_ty("User"), pub_ty("Team"), pub_ty("Account")],
    );
    assert!(er001(&air, &ErSection::default(), CheckMode::Human).is_empty());
}

#[test]
fn er001_rejects_substring_matches_on_error() {
    // `Errand`, `Errata`, `Errno` end in lowercase tails — not the
    // CamelCase `Error` / `Err` suffix. A `Bearer` ends in `r`, not `Err`,
    // so it never even reaches the boundary check.
    let air = air_with_file_items(
        "src/words.rs",
        vec![
            pub_ty("Errand"),
            pub_ty("Errata"),
            pub_ty("Errno"),
            pub_ty("Bearer"),
        ],
    );
    assert!(
        er001(&air, &ErSection::default(), CheckMode::Human).is_empty(),
        "substring matches must not trip ER001"
    );
}

#[test]
fn er001_detects_err_suffix_too() {
    // `IoErr` and `ParseErr` are full-word `Err` suffixes; both should
    // count as error types and trigger ER001 when they live together.
    let air = air_with_file_items("src/io.rs", vec![pub_ty("IoErr"), pub_ty("ParseErr")]);
    let diags = er001(&air, &ErSection::default(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("ParseErr"));
    assert!(diags[0].message.contains("IoErr"));
}

#[test]
fn er001_agent_strict_elevates_to_fatal() {
    let air = air_with_file_items(
        "src/errors.rs",
        vec![pub_ty("UserError"), pub_ty("CreateUserError")],
    );
    let diags = er001(&air, &ErSection::default(), CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn er001_skips_private_error_types() {
    // Two private error types and one public one: only one *public* error
    // type means no diagnostic. Private types are noise — likely internal
    // helper types, not part of the user-facing taxonomy.
    let air = air_with_file_items(
        "src/errors.rs",
        vec![
            pub_ty("UserError"),
            ty("PrivateError", Visibility::Private),
            ty("AlsoPrivateError", Visibility::Module),
            ty("RestrictedError", Visibility::Restricted),
        ],
    );
    assert!(
        er001(&air, &ErSection::default(), CheckMode::Human).is_empty(),
        "only public error types should count"
    );
}

#[test]
fn er001_isolated_files_do_not_cross_contaminate() {
    // Two files, each with a single error type → no diagnostic. ER001
    // operates per-file, not per-workspace.
    let air = AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![
                AirFile {
                    path: "src/a.rs".into(),
                    module_path: Some("crate::a".into()),
                    items: vec![pub_ty("AError")],
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                },
                AirFile {
                    path: "src/b.rs".into(),
                    module_path: Some("crate::b".into()),
                    items: vec![pub_ty("BError")],
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                },
            ],
        }],
        facts: Vec::new(),
    };
    assert!(er001(&air, &ErSection::default(), CheckMode::Human).is_empty());
}

// ---- has_error_suffix unit tests ----

#[test]
fn has_error_suffix_accepts_camel_case_words() {
    assert!(has_error_suffix("UserError"));
    assert!(has_error_suffix("CreateUserError"));
    assert!(has_error_suffix("Error")); // bare match is allowed
    assert!(has_error_suffix("Err"));
    assert!(has_error_suffix("IoErr"));
    assert!(has_error_suffix("ParseErr"));
    assert!(has_error_suffix("HTTPError"));
    assert!(has_error_suffix("io_Error")); // underscore separator is fine
}

#[test]
fn has_error_suffix_rejects_substring_traps() {
    // Each of these would catch a sloppy "contains `error`" check, but
    // the case-sensitive CamelCase suffix avoids them all.
    assert!(!has_error_suffix("Errand")); // ends in `and`
    assert!(!has_error_suffix("Errata")); // ends in `ata`
    assert!(!has_error_suffix("Errno")); // ends in `no`
    assert!(!has_error_suffix("Bearer")); // ends in `er`, not `Err`
    assert!(!has_error_suffix("Terror")); // ends in `rror` (lowercase e)
    assert!(!has_error_suffix("Mirror")); // ends in `rror` (lowercase e)
    assert!(!has_error_suffix("User"));
    assert!(!has_error_suffix(""));
}

// ---- ER002 tests ----

fn func(name: &str, return_type: Option<&str>) -> AirItem {
    AirItem::Function(locus_air::AirFunction {
        name: name.into(),
        symbol: format!("x::ops::{name}"),
        visibility: Visibility::Public,
        params: Vec::new(),
        return_type: return_type.map(str::to_string),
        span: AirSpan::new("src/ops.rs", 10, 20),
        line_count: 5,
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        doc: None,
    })
}

fn er002_section(patterns: &[&str]) -> ErSection {
    ErSection {
        forbidden_error_types: patterns.iter().map(|p| (*p).into()).collect(),
        ..Default::default()
    }
}

#[test]
fn er002_fires_on_string_error_when_string_is_forbidden() {
    let air = air_with_file_items("src/ops.rs", vec![func("save", Some("Result<(), String>"))]);
    let section = er002_section(&["String"]);
    let diags = er002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "ER002");
    assert_eq!(diags[0].severity, Severity::Fatal);
    assert!(diags[0].message.contains("save"));
    assert!(diags[0].message.contains("String"));
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("Result<(), String>")),
        "why list should include the rendered return type; got: {:?}",
        diags[0].why
    );
    assert!(
        diags[0]
            .suggested_fix
            .as_deref()
            .unwrap_or("")
            .contains("thiserror::Error"),
        "suggested fix should mention the typed-enum pattern; got: {:?}",
        diags[0].suggested_fix
    );
}

#[test]
fn er002_fires_on_anyhow_error_via_wildcard_pattern() {
    let air = air_with_file_items(
        "src/ops.rs",
        vec![func("load", Some("Result<User, anyhow::Error>"))],
    );
    let section = er002_section(&["anyhow::*"]);
    let diags = er002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("anyhow::Error"));
    assert!(diags[0].message.contains("anyhow::*"));
}

#[test]
fn er002_quiet_on_typed_error_not_in_forbidden_list() {
    let air = air_with_file_items(
        "src/ops.rs",
        vec![func("load", Some("Result<User, MyError>"))],
    );
    let section = er002_section(&["String", "anyhow::*", "Box<dyn *>"]);
    assert!(er002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn er002_silent_when_forbidden_list_is_empty() {
    // Default ErSection has no forbidden patterns → ER002 must stay
    // entirely quiet, even on the most string-shaped function in the
    // workspace. This is the mandatory "silent-on-default" contract.
    let air = air_with_file_items(
        "src/ops.rs",
        vec![
            func("save", Some("Result<(), String>")),
            func("load", Some("Result<User, anyhow::Error>")),
        ],
    );
    assert!(er002(&air, &ErSection::default(), CheckMode::Human).is_empty());
}

#[test]
fn er002_agent_strict_keeps_severity_fatal() {
    // Already Fatal in Human mode; AgentStrict must not change anything.
    let air = air_with_file_items("src/ops.rs", vec![func("save", Some("Result<(), String>"))]);
    let section = er002_section(&["String"]);
    let human = er002(&air, &section, CheckMode::Human);
    let strict = er002(&air, &section, CheckMode::AgentStrict);
    assert_eq!(human.len(), 1);
    assert_eq!(strict.len(), 1);
    assert_eq!(human[0].severity, Severity::Fatal);
    assert_eq!(strict[0].severity, Severity::Fatal);
}

#[test]
fn er002_handles_nested_generics_in_ok_position() {
    // `Result<Vec<T>, String>` — naive comma split would land on the
    // `T>, String` fragment. The angle-bracket-aware extractor must
    // recover `String` as the error type.
    let air = air_with_file_items(
        "src/ops.rs",
        vec![func("collect_all", Some("Result<Vec<User>, String>"))],
    );
    let section = er002_section(&["String"]);
    let diags = er002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(
        diags[0].message.contains("`String`"),
        "extracted error type should be String, not the Vec fragment; got: {}",
        diags[0].message
    );
}

#[test]
fn er002_matches_box_dyn_error_via_wildcard() {
    // `"Box<dyn *>"` is the recommended pattern for any type-erased
    // `dyn Error`, including `Box<dyn std::error::Error + Send + Sync>`.
    let air = air_with_file_items(
        "src/ops.rs",
        vec![func(
            "run",
            Some("Result<(), Box<dyn std::error::Error + Send + Sync>>"),
        )],
    );
    let section = er002_section(&["Box<dyn *>"]);
    let diags = er002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("Box<dyn *>"));
}

#[test]
fn er002_strips_leading_ampersand_for_str_match() {
    // A function returning `Result<(), &str>` should match the literal
    // `"&str"` pattern (the leading `&` is preserved in the rendering)
    // and also a bare `"str"` pattern after the `&` is peeled.
    let air = air_with_file_items("src/ops.rs", vec![func("save", Some("Result<(), &str>"))]);
    let amp_section = er002_section(&["&str"]);
    assert_eq!(er002(&air, &amp_section, CheckMode::Human).len(), 1);
    let bare_section = er002_section(&["str"]);
    assert_eq!(er002(&air, &bare_section, CheckMode::Human).len(), 1);
}

#[test]
fn er002_skips_non_result_returns() {
    let air = air_with_file_items(
        "src/ops.rs",
        vec![
            func("count", Some("u64")),
            func("noop", None),
            // Custom `Result<T>` alias with one type parameter — top-level
            // comma absent, so ER002 must skip it.
            func("custom_alias", Some("Result<User>")),
        ],
    );
    let section = er002_section(&["String", "anyhow::*", "*::Error"]);
    assert!(er002(&air, &section, CheckMode::Human).is_empty());
}

// ---- extract_result_error_type / matcher unit tests ----

#[test]
fn extract_result_error_type_basic_and_nested() {
    assert_eq!(
        extract_result_error_type("Result<User, String>"),
        Some("String")
    );
    assert_eq!(
        extract_result_error_type("Result<HashMap<UserId, User>, anyhow::Error>"),
        Some("anyhow::Error")
    );
    assert_eq!(extract_result_error_type("Result<User>"), None);
    assert_eq!(extract_result_error_type("u64"), None);
}

#[test]
fn matches_error_pattern_exact_and_glob() {
    // No `*` → exact match only.
    assert!(matches_error_pattern("String", "String"));
    assert!(!matches_error_pattern("String", "Strings"));
    assert!(!matches_error_pattern("String", "MyString"));

    // Suffix wildcard.
    assert!(matches_error_pattern("anyhow::*", "anyhow::Error"));
    assert!(matches_error_pattern("anyhow::*", "anyhow::Result"));
    assert!(!matches_error_pattern("anyhow::*", "eyre::Report"));

    // Prefix wildcard. `"*::Error"` requires `"::Error"` as a literal
    // suffix, so a bare `MyError` (no `::`) does not match.
    assert!(matches_error_pattern("*::Error", "std::io::Error"));
    assert!(matches_error_pattern("*::Error", "x::Error"));
    assert!(!matches_error_pattern("*::Error", "MyError"));
    assert!(!matches_error_pattern("*::Error", "Error"));

    // Mid-pattern wildcard.
    assert!(matches_error_pattern("Box<dyn *>", "Box<dyn Error>"));
    assert!(matches_error_pattern(
        "Box<dyn *>",
        "Box<dyn std::error::Error + Send + Sync>"
    ));
    assert!(!matches_error_pattern("Box<dyn *>", "Arc<dyn Error>"));
}

// ---- ER003 helpers + tests ----

fn enum_with_variants(name: &str, variants: Vec<(&str, Vec<&str>)>) -> AirItem {
    let air_variants: Vec<locus_air::AirVariant> = variants
        .into_iter()
        .map(|(vname, field_types)| locus_air::AirVariant {
            name: vname.into(),
            fields: field_types
                .into_iter()
                .enumerate()
                .map(|(i, t)| locus_air::AirField {
                    name: format!("f{i}"),
                    type_text: t.into(),
                    visibility: Visibility::Public,
                })
                .collect(),
        })
        .collect();
    AirItem::Type(AirType {
        kind: TypeKind::Enum,
        name: name.into(),
        symbol: format!("crate::errors::{name}"),
        visibility: Visibility::Public,
        fields: Vec::new(),
        variants: air_variants,
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        span: AirSpan::new("src/errors.rs", 1, 1),
        doc: None,
    })
}

fn er003_section() -> ErSection {
    ErSection {
        domain_paths: vec!["x::domain::*".into()],
        boundary_error_patterns: vec![
            "reqwest::Error".into(),
            "sqlx::*".into(),
            "std::io::Error".into(),
        ],
        ..Default::default()
    }
}

fn air_with_module(file_path: &str, module: &str, items: Vec<AirItem>) -> AirWorkspace {
    AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: file_path.into(),
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
fn er003_fires_on_boundary_field_in_domain_enum() {
    let air = air_with_module(
        "src/domain/user.rs",
        "x::domain::user",
        vec![enum_with_variants(
            "UserError",
            vec![("Network", vec!["reqwest::Error"]), ("NotFound", vec![])],
        )],
    );
    let diags = er003(&air, &er003_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected one diag, got {diags:?}");
    assert_eq!(diags[0].rule_id, "ER003");
    assert_eq!(diags[0].severity, Severity::Fatal);
    assert!(diags[0].message.contains("UserError"));
    assert!(diags[0].message.contains("Network"));
    assert!(diags[0].message.contains("reqwest::Error"));
}

#[test]
fn er003_fires_via_wildcard_boundary_pattern() {
    // `sqlx::*` matches `sqlx::postgres::PgError`.
    let air = air_with_module(
        "src/domain/orders.rs",
        "x::domain::orders",
        vec![enum_with_variants(
            "OrderError",
            vec![("Db", vec!["sqlx::postgres::PgError"])],
        )],
    );
    let diags = er003(&air, &er003_section(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("sqlx::postgres::PgError"));
    assert!(diags[0].message.contains("sqlx::*"));
}

#[test]
fn er003_quiet_on_domain_only_field_types() {
    let air = air_with_module(
        "src/domain/user.rs",
        "x::domain::user",
        vec![enum_with_variants(
            "UserError",
            vec![("NotFound", vec![]), ("Invalid", vec!["String"])],
        )],
    );
    assert!(er003(&air, &er003_section(), CheckMode::Human).is_empty());
}

#[test]
fn er003_quiet_outside_domain_paths() {
    // Same boundary error, but living in an adapter module — fine.
    let air = air_with_module(
        "src/adapters/http.rs",
        "x::adapters::http",
        vec![enum_with_variants(
            "HttpError",
            vec![("Network", vec!["reqwest::Error"])],
        )],
    );
    assert!(er003(&air, &er003_section(), CheckMode::Human).is_empty());
}

#[test]
fn er003_silent_when_lockfile_lists_empty() {
    let air = air_with_module(
        "src/domain/user.rs",
        "x::domain::user",
        vec![enum_with_variants(
            "UserError",
            vec![("Network", vec!["reqwest::Error"])],
        )],
    );
    // domain only
    let only_domain = ErSection {
        domain_paths: vec!["x::domain::*".into()],
        ..Default::default()
    };
    assert!(er003(&air, &only_domain, CheckMode::Human).is_empty());
    // boundary only
    let only_boundary = ErSection {
        boundary_error_patterns: vec!["reqwest::Error".into()],
        ..Default::default()
    };
    assert!(er003(&air, &only_boundary, CheckMode::Human).is_empty());
    // default (both empty)
    assert!(er003(&air, &ErSection::default(), CheckMode::Human).is_empty());
}

#[test]
fn er003_skips_struct_kinds() {
    // Only enum variants are inspected. A struct field with a boundary
    // type is out of ER003's scope (it would be flagged by other rules).
    let mut item = enum_with_variants("UserError", vec![("Network", vec!["reqwest::Error"])]);
    if let AirItem::Type(ref mut ty) = item {
        ty.kind = TypeKind::Struct;
    }
    let air = air_with_module("src/domain/user.rs", "x::domain::user", vec![item]);
    assert!(er003(&air, &er003_section(), CheckMode::Human).is_empty());
}

#[test]
fn er003_emits_one_diag_per_offending_field() {
    let air = air_with_module(
        "src/domain/user.rs",
        "x::domain::user",
        vec![enum_with_variants(
            "UserError",
            vec![
                ("Network", vec!["reqwest::Error"]),
                ("Db", vec!["sqlx::Error"]),
                ("Io", vec!["std::io::Error"]),
                ("NotFound", vec![]),
            ],
        )],
    );
    let diags = er003(&air, &er003_section(), CheckMode::Human);
    assert_eq!(diags.len(), 3);
    let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert!(messages.iter().any(|m| m.contains("Network")));
    assert!(messages.iter().any(|m| m.contains("Db")));
    assert!(messages.iter().any(|m| m.contains("Io")));
}

// ---- ER005 helpers + tests ----

fn match_arm(
    pattern: &str,
    pattern_has_wildcard: bool,
    body_shape: ArmBodyShape,
    function: Option<&str>,
) -> AirItem {
    AirItem::MatchArm(AirMatchArm {
        scrutinee: "result".into(),
        pattern: pattern.into(),
        pattern_has_wildcard,
        body_shape,
        function: function.map(str::to_string),
        span: AirSpan::new("src/ops.rs", 30, 35),
    })
}

fn er005_section(patterns: &[&str]) -> ErSection {
    ErSection {
        error_collapse_owner_paths: patterns.iter().map(|p| (*p).into()).collect(),
        ..Default::default()
    }
}

#[test]
fn er005_fires_on_err_underscore_arm_with_call_body() {
    let air = air_with_module(
        "src/domain/handlers.rs",
        "x::domain::handlers",
        vec![match_arm(
            "Err(_)",
            true,
            ArmBodyShape::Call,
            Some("x::domain::handlers::handle"),
        )],
    );
    let section = er005_section(&["*::cli::*"]);
    let diags = er005(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected one diag, got {diags:?}");
    assert_eq!(diags[0].rule_id, "ER005");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(
        diags[0]
            .message
            .contains("collapses distinct error variants"),
        "message should mention collapse; got: {}",
        diags[0].message,
    );
    assert!(
        diags[0].message.contains("x::domain::handlers"),
        "message should reference module; got: {}",
        diags[0].message,
    );
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("Err") && w.contains("matches every")),
        "why list should explain Err pattern; got: {:?}",
        diags[0].why,
    );
    assert!(
        diags[0]
            .suggested_fix
            .as_deref()
            .unwrap_or("")
            .contains("error_collapse_owner_paths"),
        "suggested fix should mention the lockfile field; got: {:?}",
        diags[0].suggested_fix,
    );
}

#[test]
fn er005_fires_on_err_underscore_arm_with_literal_body() {
    let air = air_with_module(
        "src/domain/handlers.rs",
        "x::domain::handlers",
        vec![match_arm(
            "Err(_)",
            true,
            ArmBodyShape::Literal,
            Some("x::domain::handlers::handle"),
        )],
    );
    let section = er005_section(&["*::cli::*"]);
    let diags = er005(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("literal"));
}

#[test]
fn er005_quiet_on_propagate_body_question_mark() {
    // `Err(e) => return Err(e.into())` uses `?` somewhere → Propagate.
    // That's not collapse — the error is being typed-and-propagated.
    let air = air_with_module(
        "src/domain/handlers.rs",
        "x::domain::handlers",
        vec![match_arm(
            "Err(_)",
            true,
            ArmBodyShape::ErrorPropagation,
            Some("x::domain::handlers::handle"),
        )],
    );
    let section = er005_section(&["*::cli::*"]);
    assert!(er005(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn er005_quiet_on_bare_wildcard_arm() {
    // Bare `_` is FL011's territory — pattern doesn't start with `Err`
    // and doesn't contain `Err(`, so ER005 must skip it.
    let air = air_with_module(
        "src/domain/handlers.rs",
        "x::domain::handlers",
        vec![match_arm(
            "_",
            true,
            ArmBodyShape::Call,
            Some("x::domain::handlers::handle"),
        )],
    );
    let section = er005_section(&["*::cli::*"]);
    assert!(er005(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn er005_quiet_when_file_in_collapse_owner_path() {
    // Same arm shape, but the file's module_path is on the
    // collapse-owner allowlist — must be silent.
    let air = air_with_module(
        "src/cli/handlers.rs",
        "x::cli::handlers",
        vec![match_arm(
            "Err(_)",
            true,
            ArmBodyShape::Call,
            Some("x::cli::handlers::handle"),
        )],
    );
    let section = er005_section(&["*::cli::*"]);
    assert!(er005(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn er005_quiet_when_function_in_collapse_owner_path_via_inline_test_mod() {
    // File's module_path doesn't include `::tests::`, but the function
    // symbol does (inline `mod tests {}` block). Segment-anywhere
    // matcher must catch the function symbol form.
    let air = air_with_module(
        "src/domain/handlers.rs",
        "x::domain::handlers",
        vec![match_arm(
            "Err(_)",
            true,
            ArmBodyShape::Call,
            Some("x::domain::handlers::tests::case"),
        )],
    );
    let section = er005_section(&["*::tests::*"]);
    assert!(er005(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn er005_silent_when_collapse_owner_paths_empty() {
    // Default ErSection has no collapse-owner patterns → ER005 stays
    // entirely quiet on the most blatant collapse arm. Mandatory
    // silent-on-default contract.
    let air = air_with_module(
        "src/domain/handlers.rs",
        "x::domain::handlers",
        vec![match_arm(
            "Err(_)",
            true,
            ArmBodyShape::Call,
            Some("x::domain::handlers::handle"),
        )],
    );
    assert!(er005(&air, &ErSection::default(), CheckMode::Human).is_empty());
}

#[test]
fn er005_agent_strict_elevates_warning_to_fatal() {
    let air = air_with_module(
        "src/domain/handlers.rs",
        "x::domain::handlers",
        vec![match_arm(
            "Err(_)",
            true,
            ArmBodyShape::Call,
            Some("x::domain::handlers::handle"),
        )],
    );
    let section = er005_section(&["*::cli::*"]);
    let diags = er005(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn er005_quiet_on_block_body_doing_real_work() {
    // Multi-statement block body — could be doing real work; ER005
    // must not pre-judge. Only Empty / Literal / Call qualify.
    let air = air_with_module(
        "src/domain/handlers.rs",
        "x::domain::handlers",
        vec![match_arm(
            "Err(_)",
            true,
            ArmBodyShape::Block,
            Some("x::domain::handlers::handle"),
        )],
    );
    let section = er005_section(&["*::cli::*"]);
    assert!(er005(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn er005_fires_on_nested_err_pattern_with_wildcard() {
    // `Err(MyError::Generic(_))` — pattern starts with `Err` and has
    // a wildcard somewhere. ER005 should fire the same way.
    let air = air_with_module(
        "src/domain/handlers.rs",
        "x::domain::handlers",
        vec![match_arm(
            "Err(MyError::Generic(_))",
            true,
            ArmBodyShape::Call,
            Some("x::domain::handlers::handle"),
        )],
    );
    let section = er005_section(&["*::cli::*"]);
    let diags = er005(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("collapses"));
}

// ---- ER007 tests ----

fn er007_air(files: Vec<(&str, Vec<AirItem>)>) -> AirWorkspace {
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
                    module_path: Some("crate".into()),
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
fn er007_fires_when_variant_name_repeats_across_error_enums() {
    let air = er007_air(vec![(
        "src/errors.rs",
        vec![
            enum_with_variants("UserError", vec![("NotFound", vec![]), ("Invalid", vec![])]),
            enum_with_variants(
                "OrderError",
                vec![("NotFound", vec![]), ("Cancelled", vec![])],
            ),
        ],
    )]);
    let diags = er007(&air, CheckMode::Human);
    // `NotFound` appears on UserError (incumbent) and OrderError → 1 diag.
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "ER007");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("NotFound"));
    assert!(diags[0].message.contains("OrderError"));
    assert!(diags[0].message.contains("UserError"));
}

#[test]
fn er007_quiet_when_each_variant_unique() {
    let air = er007_air(vec![(
        "src/errors.rs",
        vec![
            enum_with_variants("UserError", vec![("NotFound", vec![])]),
            enum_with_variants("OrderError", vec![("Cancelled", vec![])]),
            enum_with_variants("BillingError", vec![("Declined", vec![])]),
        ],
    )]);
    assert!(er007(&air, CheckMode::Human).is_empty());
}

#[test]
fn er007_skips_non_error_enums() {
    // `Status` enum shares variant names with `UserError` but isn't an
    // error type — must not trip ER007.
    let air = er007_air(vec![(
        "src/types.rs",
        vec![
            enum_with_variants("Status", vec![("Active", vec![]), ("NotFound", vec![])]),
            enum_with_variants("UserError", vec![("NotFound", vec![])]),
        ],
    )]);
    // Only `UserError::NotFound` is observed (Status is skipped); single
    // occurrence → no diagnostic.
    assert!(er007(&air, CheckMode::Human).is_empty());
}

#[test]
fn er007_detects_duplicates_across_files() {
    let air = er007_air(vec![
        (
            "src/users.rs",
            vec![enum_with_variants("UserError", vec![("Invalid", vec![])])],
        ),
        (
            "src/orders.rs",
            vec![enum_with_variants("OrderError", vec![("Invalid", vec![])])],
        ),
    ]);
    let diags = er007(&air, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("Invalid"));
    assert!(
        diags[0].why.iter().any(|w| w.contains("src/users.rs")),
        "why list should reference the incumbent file; got: {:?}",
        diags[0].why,
    );
}

#[test]
fn er007_emits_one_diag_per_extra_occurrence() {
    // `NotFound` appears on three error types → two extra occurrences,
    // one diagnostic each.
    let air = er007_air(vec![(
        "src/errors.rs",
        vec![
            enum_with_variants("UserError", vec![("NotFound", vec![])]),
            enum_with_variants("OrderError", vec![("NotFound", vec![])]),
            enum_with_variants("BillingError", vec![("NotFound", vec![])]),
        ],
    )]);
    let diags = er007(&air, CheckMode::Human);
    assert_eq!(diags.len(), 2);
    assert!(diags.iter().all(|d| d.rule_id == "ER007"));
}

#[test]
fn er007_agent_strict_elevates_to_fatal() {
    let air = er007_air(vec![(
        "src/errors.rs",
        vec![
            enum_with_variants("UserError", vec![("NotFound", vec![])]),
            enum_with_variants("OrderError", vec![("NotFound", vec![])]),
        ],
    )]);
    let diags = er007(&air, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}
