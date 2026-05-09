//! Tests for [`super`] rule implementations.
//!
//! Extracted from `rules.rs` to keep the production module within the
//! CX002 line budget. Re-attached via `#[path = "rules_tests.rs"] mod
//! tests;` at the bottom of `rules.rs`.

use super::*;
use locus_air::{AIR_SCHEMA_VERSION, AirFile, AirImplBlock, AirPackage, AirType, Visibility};

fn trait_decl(symbol: &str, name: &str) -> AirItem {
    AirItem::Type(AirType {
        kind: TypeKind::Trait,
        name: name.into(),
        symbol: symbol.into(),
        visibility: Visibility::Public,
        fields: Vec::new(),
        variants: Vec::new(),
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        span: AirSpan::new("t.rs", 10, 20),
        doc: None,
    })
}

fn struct_decl(symbol: &str, name: &str) -> AirItem {
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
        span: AirSpan::new("t.rs", 30, 40),
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
                module_path: Some("x::core".into()),
                items,
                hints: Vec::new(),
                parse_error: None,
                line_count: 100,
            }],
        }],
        facts: Vec::new(),
    }
}

#[test]
fn ab001_fires_on_trait_with_exactly_one_impl() {
    let air = air_with(vec![
        trait_decl("x::core::Manager", "Manager"),
        struct_decl("x::core::ConcreteManager", "ConcreteManager"),
        impl_for(Some("x::core::Manager"), "ConcreteManager"),
    ]);
    let section = AbSection::default();
    let diags = ab001(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected one diag, got {diags:?}");
    assert_eq!(diags[0].rule_id, "AB001");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("Manager"));
    assert!(diags[0].message.contains("ConcreteManager"));
    // Span anchored at the trait declaration, not the impl.
    assert_eq!(diags[0].span.line_start, 10);
}

#[test]
fn ab001_quiet_on_trait_with_zero_impls() {
    let air = air_with(vec![trait_decl("x::core::ApiSurface", "ApiSurface")]);
    let section = AbSection::default();
    assert!(ab001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ab001_quiet_on_trait_with_two_impls() {
    let air = air_with(vec![
        trait_decl("x::core::Clock", "Clock"),
        impl_for(Some("x::core::Clock"), "SystemClock"),
        impl_for(Some("x::core::Clock"), "TestClock"),
    ]);
    let section = AbSection::default();
    assert!(ab001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ab001_quiet_on_trait_with_three_impls() {
    let air = air_with(vec![
        trait_decl("x::core::Storage", "Storage"),
        impl_for(Some("x::core::Storage"), "MemStorage"),
        impl_for(Some("x::core::Storage"), "DiskStorage"),
        impl_for(Some("x::core::Storage"), "S3Storage"),
    ]);
    let section = AbSection::default();
    assert!(ab001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ab001_exempted_by_full_symbol_pattern() {
    let air = air_with(vec![
        trait_decl("x::ports::Clock", "Clock"),
        impl_for(Some("x::ports::Clock"), "SystemClock"),
    ]);
    let section = AbSection {
        accepted_single_impl_traits: vec!["x::ports::*".into()],
        ..AbSection::default()
    };
    assert!(ab001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ab001_exempted_by_short_name_pattern() {
    let air = air_with(vec![
        trait_decl("x::core::Manager", "Manager"),
        impl_for(Some("x::core::Manager"), "ConcreteManager"),
    ]);
    let section = AbSection {
        accepted_single_impl_traits: vec!["Manager".into()],
        ..AbSection::default()
    };
    assert!(ab001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ab001_agent_strict_elevates_to_fatal() {
    let air = air_with(vec![
        trait_decl("x::core::Manager", "Manager"),
        impl_for(Some("x::core::Manager"), "ConcreteManager"),
    ]);
    let section = AbSection::default();
    let diags = ab001(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn ab001_inherent_impls_do_not_count_as_trait_impls() {
    // Trait declared but never `impl`-ed. The struct has an inherent
    // impl (trait_path = None) — that must NOT be counted as the trait's
    // implementation, so the trait stays at 0 impls and AB001 stays
    // silent (the 0-impl skip rule).
    let air = air_with(vec![
        trait_decl("x::core::Manager", "Manager"),
        struct_decl("x::core::Thing", "Thing"),
        impl_for(None, "Thing"),
    ]);
    let section = AbSection::default();
    assert!(ab001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ab001_matches_via_short_name_when_impl_uses_shorter_path() {
    // Trait is declared as `x::core::Manager` but the impl references
    // it by the bare `Manager` name (e.g. via `use`). Should still be
    // counted as the trait's only impl.
    let air = air_with(vec![
        trait_decl("x::core::Manager", "Manager"),
        impl_for(Some("Manager"), "ConcreteManager"),
    ]);
    let section = AbSection::default();
    let diags = ab001(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1, "got {diags:?}");
    assert!(diags[0].message.contains("ConcreteManager"));
}

#[test]
fn ab001_diagnostic_includes_rule_id_and_suggested_fix() {
    let air = air_with(vec![
        trait_decl("x::core::Manager", "Manager"),
        impl_for(Some("x::core::Manager"), "ConcreteManager"),
    ]);
    let section = AbSection::default();
    let diags = ab001(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.rule_id, "AB001");
    assert!(d.suggested_fix.is_some());
    let fix = d.suggested_fix.as_ref().unwrap();
    assert!(fix.contains("ConcreteManager"));
    assert!(fix.contains("accepted_single_impl_traits"));
    // why list mentions impl count and the lone self_ty
    assert!(d.why.iter().any(|w| w.contains("impl count: 1")));
    assert!(d.why.iter().any(|w| w.contains("ConcreteManager")));
}

#[test]
fn ab001_does_not_fire_on_external_trait_impls() {
    // Implementing an external trait (e.g. `std::fmt::Display`) for a
    // local type adds an Impl with a trait_path that doesn't match any
    // declared trait. AB001 should ignore it — it only reasons about
    // traits declared in the workspace.
    let air = air_with(vec![
        struct_decl("x::core::Thing", "Thing"),
        impl_for(Some("std::fmt::Display"), "Thing"),
    ]);
    let section = AbSection::default();
    assert!(ab001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ab001_fires_once_per_single_impl_trait() {
    // Two single-impl traits → two diagnostics; one two-impl trait stays
    // silent. Confirms per-trait emission across a mixed workspace.
    let air = air_with(vec![
        trait_decl("x::core::A", "A"),
        impl_for(Some("x::core::A"), "AImpl"),
        trait_decl("x::core::B", "B"),
        impl_for(Some("x::core::B"), "BImpl1"),
        impl_for(Some("x::core::B"), "BImpl2"),
        trait_decl("x::core::C", "C"),
        impl_for(Some("x::core::C"), "CImpl"),
    ]);
    let section = AbSection::default();
    let diags = ab001(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 2, "got {diags:?}");
    let msgs: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert!(msgs.iter().any(|m| m.contains("x::core::A")));
    assert!(msgs.iter().any(|m| m.contains("x::core::C")));
    assert!(!msgs.iter().any(|m| m.contains("x::core::B")));
}

// --- AB002 tests ---------------------------------------------------

#[test]
fn ab002_fires_on_struct_named_with_suspect_suffix() {
    // `UserManager` matches the seeded `*Manager` pattern.
    let air = air_with(vec![struct_decl("x::core::UserManager", "UserManager")]);
    let section = AbSection::default();
    let diags = ab002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1, "got {diags:?}");
    assert_eq!(diags[0].rule_id, "AB002");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("x::core::UserManager"));
    assert!(diags[0].message.contains("*Manager"));
}

#[test]
fn ab002_fires_on_trait_named_with_suspect_suffix() {
    // Trait variant of the same suspect pattern.
    let air = air_with(vec![trait_decl(
        "x::core::OrderProcessor",
        "OrderProcessor",
    )]);
    let section = AbSection::default();
    let diags = ab002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1, "got {diags:?}");
    assert!(diags[0].message.contains("trait `x::core::OrderProcessor`"));
}

#[test]
fn ab002_quiet_on_domain_named_types() {
    // Names that don't match any seeded pattern → silent.
    let air = air_with(vec![
        struct_decl("x::core::User", "User"),
        struct_decl("x::core::OrderBook", "OrderBook"),
        trait_decl("x::ports::Clock", "Clock"),
    ]);
    let section = AbSection::default();
    assert!(ab002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ab002_exempted_by_accepted_abstraction_names() {
    // Suspect name but accepted via `accepted_abstraction_names`.
    let air = air_with(vec![struct_decl("x::core::AuthService", "AuthService")]);
    let section = AbSection {
        accepted_abstraction_names: vec!["x::core::AuthService".into()],
        ..AbSection::default()
    };
    assert!(ab002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ab002_exempted_by_accepted_single_impl_traits() {
    // AB001's exemption list is also honored by AB002 — a port trait
    // already accepted as legitimately single-impl shouldn't double-fire.
    let air = air_with(vec![trait_decl("x::ports::Clock", "Clock")]);
    // Clock isn't a seeded suspect name; force a suspect pattern that
    // matches it via `*lock` → simulate a custom seeded list.
    let section = AbSection {
        suspect_abstraction_patterns: vec!["*Clock".into()],
        accepted_single_impl_traits: vec!["x::ports::*".into()],
        ..AbSection::default()
    };
    assert!(ab002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ab002_silent_when_suspect_patterns_emptied() {
    // User explicitly empties the seeded list → silent regardless of names.
    let air = air_with(vec![struct_decl("x::core::UserManager", "UserManager")]);
    let section = AbSection {
        suspect_abstraction_patterns: Vec::new(),
        ..AbSection::default()
    };
    assert!(ab002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ab002_agent_strict_elevates_to_fatal() {
    let air = air_with(vec![struct_decl("x::core::PaymentEngine", "PaymentEngine")]);
    let section = AbSection::default();
    let diags = ab002(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}
