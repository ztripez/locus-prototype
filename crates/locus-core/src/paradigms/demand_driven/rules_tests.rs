//! Tests for [`super`] rule implementations.
//!
//! Extracted from `rules.rs` to keep the production module within the
//! CX002 line budget. Re-attached via `#[path = "rules_tests.rs"] mod
//! tests;` at the bottom of `rules.rs`.

use super::*;
use locus_air::{
    AIR_SCHEMA_VERSION, AirFile, AirImplBlock, AirItem, AirPackage, AirSpan, AirType, TypeKind,
    Visibility,
};

fn trait_decl(name: &str, symbol: &str) -> AirItem {
    AirItem::Type(AirType {
        kind: TypeKind::Trait,
        name: name.into(),
        symbol: symbol.into(),
        visibility: Visibility::Public,
        fields: Vec::new(),
        variants: Vec::new(),
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        span: AirSpan::new("t.rs", 1, 1),
        doc: None,
    })
}

fn struct_decl(name: &str, symbol: &str) -> AirItem {
    AirItem::Type(AirType {
        kind: TypeKind::Struct,
        name: name.into(),
        symbol: symbol.into(),
        visibility: Visibility::Public,
        fields: Vec::new(),
        variants: Vec::new(),
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        span: AirSpan::new("t.rs", 1, 1),
        doc: None,
    })
}

fn impl_for(trait_path: Option<&str>, self_ty: &str) -> AirItem {
    AirItem::Impl(AirImplBlock {
        interface: trait_path.map(str::to_string),
        target_type: self_ty.into(),
        method_names: Vec::new(),
        dispatch: locus_air::ImplDispatch::Static,
        span: AirSpan::new("t.rs", 1, 1),
    })
}

fn air_with(items: Vec<AirItem>) -> AirWorkspace {
    AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: "t.rs".into(),
                module_path: Some("x".into()),
                items,
                hints: Vec::new(),
                parse_error: None,
                line_count: 1,
            }],
        }],
        facts: Vec::new(),
    }
}

fn enabled() -> DaSection {
    DaSection {
        enabled: true,
        accepted_single_impl: Vec::new(),
        ..DaSection::default()
    }
}

// ---- positive case ----

#[test]
fn da001_fires_on_trait_with_one_implementation() {
    let air = air_with(vec![
        trait_decl("Clock", "x::Clock"),
        struct_decl("SystemClock", "x::SystemClock"),
        impl_for(Some("Clock"), "SystemClock"),
    ]);
    let diags = da001(&air, &enabled(), CheckMode::Human);
    assert_eq!(diags.len(), 1, "got {diags:?}");
    assert_eq!(diags[0].rule_id, "DA001");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("Clock"));
    assert_eq!(diags[0].concept.as_deref(), Some("Clock"));
}

// ---- negative cases ----

#[test]
fn da001_quiet_on_trait_with_two_implementations() {
    // Two impls = real variation; abstraction earns its rent.
    let air = air_with(vec![
        trait_decl("Notifier", "x::Notifier"),
        impl_for(Some("Notifier"), "EmailNotifier"),
        impl_for(Some("Notifier"), "SmsNotifier"),
    ]);
    assert!(da001(&air, &enabled(), CheckMode::Human).is_empty());
}

#[test]
fn da001_quiet_on_trait_with_zero_implementations() {
    // Zero impls is a separate failure mode (orphaned surface) — outside
    // DA001's scope. DA001 fires only on the single-impl shape.
    let air = air_with(vec![trait_decl("Untouched", "x::Untouched")]);
    assert!(da001(&air, &enabled(), CheckMode::Human).is_empty());
}

#[test]
fn da001_silent_when_section_disabled_default() {
    // No `enabled = true` → DA never fires, even with a slam-dunk
    // single-impl trait. Same lockfile-driven UX as DG/MO/CX.
    let air = air_with(vec![
        trait_decl("Clock", "x::Clock"),
        impl_for(Some("Clock"), "SystemClock"),
    ]);
    assert!(da001(&air, &DaSection::default(), CheckMode::Human).is_empty());
}

// ---- exemption ----

#[test]
fn da001_quiet_when_trait_in_accepted_list_by_short_name() {
    let air = air_with(vec![
        trait_decl("Clock", "x::Clock"),
        impl_for(Some("Clock"), "SystemClock"),
    ]);
    let section = DaSection {
        enabled: true,
        accepted_single_impl: vec!["Clock".into()],
        ..DaSection::default()
    };
    assert!(da001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn da001_quiet_when_trait_in_accepted_list_by_full_symbol() {
    let air = air_with(vec![
        trait_decl("Clock", "my_crate::ports::Clock"),
        impl_for(Some("my_crate::ports::Clock"), "SystemClock"),
    ]);
    let section = DaSection {
        enabled: true,
        accepted_single_impl: vec!["my_crate::ports::Clock".into()],
        ..DaSection::default()
    };
    assert!(da001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn da001_quiet_when_trait_matched_by_wildcard_namespace_pattern() {
    let air = air_with(vec![
        trait_decl("Cache", "my_crate::infra::Cache"),
        impl_for(Some("my_crate::infra::Cache"), "RedisCache"),
    ]);
    let section = DaSection {
        enabled: true,
        accepted_single_impl: vec!["my_crate::infra::*".into()],
        ..DaSection::default()
    };
    assert!(da001(&air, &section, CheckMode::Human).is_empty());
}

// ---- agent-strict elevation ----

#[test]
fn da001_agent_strict_elevates_warning_to_fatal() {
    let air = air_with(vec![
        trait_decl("Clock", "x::Clock"),
        impl_for(Some("Clock"), "SystemClock"),
    ]);
    let diags = da001(&air, &enabled(), CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(
        diags[0].severity,
        Severity::Fatal,
        "agent-strict should elevate Warning to Fatal"
    );
}

// ---- edge cases ----

#[test]
fn da001_recognises_full_symbol_and_short_name_impls_as_same_trait() {
    // One impl writes the full path, the other uses the short name after
    // a `use`. Both should resolve to the same trait → count == 2 → quiet.
    let air = air_with(vec![
        trait_decl("Notifier", "my_crate::Notifier"),
        impl_for(Some("my_crate::Notifier"), "Email"),
        impl_for(Some("Notifier"), "Sms"),
    ]);
    assert!(
        da001(&air, &enabled(), CheckMode::Human).is_empty(),
        "two impls — one by full symbol, one by short name — should both count"
    );
}

#[test]
fn da001_strips_generic_args_when_matching_trait_path() {
    // `impl Trait<X> for Y` should still be counted as an impl of `Trait`.
    let air = air_with(vec![
        trait_decl("Convert", "x::Convert"),
        impl_for(Some("Convert<u32>"), "Wrapper"),
    ]);
    let diags = da001(&air, &enabled(), CheckMode::Human);
    assert_eq!(
        diags.len(),
        1,
        "generic-arg impl should still match; got {diags:?}"
    );
}

#[test]
fn da001_ignores_inherent_impls() {
    // Inherent `impl SystemClock {}` (no `trait_path`) must not count
    // toward Clock's impl tally.
    let air = air_with(vec![
        trait_decl("Clock", "x::Clock"),
        impl_for(None, "SystemClock"), // inherent impl
        impl_for(Some("Clock"), "SystemClock"),
    ]);
    let diags = da001(&air, &enabled(), CheckMode::Human);
    assert_eq!(diags.len(), 1, "inherent impl should not raise the count");
}

#[test]
fn da001_counts_impls_across_files_and_packages() {
    // Trait declared in package x, single impl in package y. The rule
    // must walk all packages, not just the declaring one.
    let air = AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![
            AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/x".into(),
                files: vec![AirFile {
                    path: "x/lib.rs".into(),
                    module_path: Some("x".into()),
                    items: vec![trait_decl("Clock", "x::Clock")],
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                }],
            },
            AirPackage {
                name: "y".into(),
                version: "0".into(),
                root_dir: "/y".into(),
                files: vec![AirFile {
                    path: "y/lib.rs".into(),
                    module_path: Some("y".into()),
                    items: vec![impl_for(Some("x::Clock"), "SystemClock")],
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                }],
            },
        ],
        facts: Vec::new(),
    };
    let diags = da001(&air, &enabled(), CheckMode::Human);
    assert_eq!(diags.len(), 1, "cross-package impl should count");
    assert!(diags[0].why.iter().any(|w| w.contains("x::Clock")));
}

#[test]
fn da001_one_diagnostic_per_violating_trait() {
    // Two distinct single-impl traits → two diagnostics. Independent rule
    // application per trait.
    let air = air_with(vec![
        trait_decl("Clock", "x::Clock"),
        impl_for(Some("Clock"), "SystemClock"),
        trait_decl("Logger", "x::Logger"),
        impl_for(Some("Logger"), "StdoutLogger"),
    ]);
    let diags = da001(&air, &enabled(), CheckMode::Human);
    assert_eq!(diags.len(), 2, "got {diags:?}");
    let concepts: Vec<&str> = diags.iter().filter_map(|d| d.concept.as_deref()).collect();
    assert!(concepts.contains(&"Clock"));
    assert!(concepts.contains(&"Logger"));
}

#[test]
fn da001_diagnostic_anchors_on_trait_declaration_span() {
    // The diagnostic should point at the trait, not the impl — the
    // architectural decision lives at the declaration.
    let trait_span = AirSpan::new("traits.rs", 42, 47);
    let trait_item = AirItem::Type(AirType {
        kind: TypeKind::Trait,
        name: "Clock".into(),
        symbol: "x::Clock".into(),
        visibility: Visibility::Public,
        fields: Vec::new(),
        variants: Vec::new(),
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        span: trait_span.clone(),
        doc: None,
    });
    let air = air_with(vec![trait_item, impl_for(Some("Clock"), "SystemClock")]);
    let diags = da001(&air, &enabled(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].span, trait_span);
}

// ------------- DA002 / DA007 helpers -------------

use locus_air::{ActionKind, AirFunction, AirTruthAction, AirVariant};

fn fn_decl(name: &str, symbol: &str) -> AirItem {
    AirItem::Function(AirFunction {
        name: name.into(),
        symbol: symbol.into(),
        visibility: Visibility::Public,
        params: Vec::new(),
        return_type: None,
        span: AirSpan::new("t.rs", 1, 1),
        line_count: 1,
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        doc: None,
    })
}

fn construct_action(target: &str, function: &str) -> AirItem {
    AirItem::TruthAction(AirTruthAction {
        action: ActionKind::Construct,
        target: target.into(),
        function: Some(function.into()),
        span: AirSpan::new("t.rs", 2, 2),
        confidence: 1.0,
        reasons: Vec::new(),
    })
}

fn enum_decl(name: &str, symbol: &str, variants: &[&str]) -> AirItem {
    AirItem::Type(AirType {
        kind: TypeKind::Enum,
        name: name.into(),
        symbol: symbol.into(),
        visibility: Visibility::Public,
        fields: Vec::new(),
        variants: variants
            .iter()
            .map(|v| AirVariant {
                name: (*v).into(),
                fields: Vec::new(),
            })
            .collect(),
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        span: AirSpan::new("t.rs", 1, 1),
        doc: None,
    })
}

// ------------- DA002 tests -------------

#[test]
fn da002_fires_when_factory_constructs_one_target() {
    // `make_widget` constructs `Widget` once → DA002 fires.
    let air = air_with(vec![
        fn_decl("make_widget", "x::make_widget"),
        construct_action("x::Widget", "x::make_widget"),
    ]);
    let diags = da002(&air, &enabled(), CheckMode::Human);
    assert_eq!(diags.len(), 1, "got {diags:?}");
    assert_eq!(diags[0].rule_id, "DA002");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("make_widget"));
    assert!(diags[0].message.contains("exactly one type"));
    assert_eq!(diags[0].concept.as_deref(), Some("make_widget"));
}

#[test]
fn da002_quiet_when_factory_constructs_two_or_more_distinct_targets() {
    // Real variation justifies the factory.
    let air = air_with(vec![
        fn_decl("create_notifier", "x::create_notifier"),
        construct_action("x::EmailNotifier", "x::create_notifier"),
        construct_action("x::SmsNotifier", "x::create_notifier"),
    ]);
    assert!(da002(&air, &enabled(), CheckMode::Human).is_empty());
}

#[test]
fn da002_quiet_when_factory_has_zero_construct_actions() {
    // A pass-through factory with no Construct truth-action — out of
    // DA002's scope (a different rule could flag pure delegation).
    let air = air_with(vec![fn_decl("build_thing", "x::build_thing")]);
    assert!(da002(&air, &enabled(), CheckMode::Human).is_empty());
}

#[test]
fn da002_quiet_when_function_name_doesnt_match_pattern() {
    let air = air_with(vec![
        fn_decl("compute_widget", "x::compute_widget"),
        construct_action("x::Widget", "x::compute_widget"),
    ]);
    assert!(da002(&air, &enabled(), CheckMode::Human).is_empty());
}

#[test]
fn da002_silent_when_section_disabled() {
    let air = air_with(vec![
        fn_decl("make_widget", "x::make_widget"),
        construct_action("x::Widget", "x::make_widget"),
    ]);
    // `enabled = false` (the default) silences DA002 even with a
    // textbook violation in scope.
    assert!(da002(&air, &DaSection::default(), CheckMode::Human).is_empty());
}

#[test]
fn da002_silent_when_factory_name_patterns_empty() {
    let air = air_with(vec![
        fn_decl("make_widget", "x::make_widget"),
        construct_action("x::Widget", "x::make_widget"),
    ]);
    let section = DaSection {
        enabled: true,
        factory_name_patterns: Vec::new(),
        ..DaSection::default()
    };
    assert!(da002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn da002_agent_strict_elevates_to_fatal() {
    let air = air_with(vec![
        fn_decl("make_widget", "x::make_widget"),
        construct_action("x::Widget", "x::make_widget"),
    ]);
    let diags = da002(&air, &enabled(), CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ------------- DA007 tests -------------

#[test]
fn da007_fires_on_single_variant_strategy_enum() {
    let air = air_with(vec![enum_decl(
        "RetryStrategy",
        "x::RetryStrategy",
        &["Linear"],
    )]);
    let diags = da007(&air, &enabled(), CheckMode::Human);
    assert_eq!(diags.len(), 1, "got {diags:?}");
    assert_eq!(diags[0].rule_id, "DA007");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("RetryStrategy"));
    assert!(diags[0].message.contains("Linear"));
    assert_eq!(diags[0].concept.as_deref(), Some("RetryStrategy"));
}

#[test]
fn da007_quiet_when_strategy_enum_has_two_or_more_variants() {
    let air = air_with(vec![enum_decl(
        "RetryStrategy",
        "x::RetryStrategy",
        &["Linear", "Exponential"],
    )]);
    assert!(da007(&air, &enabled(), CheckMode::Human).is_empty());
}

#[test]
fn da007_quiet_when_enum_name_doesnt_match_pattern() {
    // `RetryConfig` is not a `*Strategy`/`*Mode`/`*Policy` — out of scope.
    let air = air_with(vec![enum_decl(
        "RetryConfig",
        "x::RetryConfig",
        &["Default"],
    )]);
    assert!(da007(&air, &enabled(), CheckMode::Human).is_empty());
}

#[test]
fn da007_silent_when_section_disabled() {
    let air = air_with(vec![enum_decl(
        "RetryStrategy",
        "x::RetryStrategy",
        &["Linear"],
    )]);
    assert!(da007(&air, &DaSection::default(), CheckMode::Human).is_empty());
}

#[test]
fn da007_agent_strict_elevates_to_fatal() {
    let air = air_with(vec![enum_decl(
        "AccessPolicy",
        "x::AccessPolicy",
        &["AllowAll"],
    )]);
    let diags = da007(&air, &enabled(), CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn da007_skips_struct_with_strategy_name() {
    // Only enums fire — a `RetryStrategy` *struct* is some other shape
    // and DA007 should ignore it.
    let air = air_with(vec![struct_decl("RetryStrategy", "x::RetryStrategy")]);
    assert!(da007(&air, &enabled(), CheckMode::Human).is_empty());
}
