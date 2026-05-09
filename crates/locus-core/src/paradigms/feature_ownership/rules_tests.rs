//! Tests for [`super`] rule implementations.
//!
//! Extracted from `rules.rs` to keep the production module within the
//! CX002 line budget. Re-attached via `#[path = "rules_tests.rs"] mod
//! tests;` at the bottom of `rules.rs`.

use super::*;
use locus_air::{
    AIR_SCHEMA_VERSION, AirField, AirFile, AirPackage, AirSpan, AirType, TypeKind, Visibility,
};

fn ty(name: &str, symbol: &str, vis: Visibility) -> AirItem {
    AirItem::Type(AirType {
        kind: TypeKind::Struct,
        name: name.into(),
        symbol: symbol.into(),
        visibility: vis,
        fields: Vec::new(),
        variants: Vec::new(),
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        span: AirSpan::new("t.rs", 1, 1),
        doc: None,
    })
}

/// Helper for FO004: a Public struct with the given fields, each
/// field expressed as `(field_name, type_text)`.
fn ty_with_fields(name: &str, symbol: &str, fields: &[(&str, &str)]) -> AirItem {
    AirItem::Type(AirType {
        kind: TypeKind::Struct,
        name: name.into(),
        symbol: symbol.into(),
        visibility: Visibility::Public,
        fields: fields
            .iter()
            .map(|(n, t)| AirField {
                name: (*n).into(),
                type_text: (*t).into(),
                visibility: Visibility::Public,
            })
            .collect(),
        variants: Vec::new(),
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        span: AirSpan::new("t.rs", 1, 1),
        doc: None,
    })
}

type FileSpec<'a> = (&'a str, Option<&'a str>, Vec<AirItem>);

fn air_with_files(files: Vec<FileSpec<'_>>) -> AirWorkspace {
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
                    module_path: module.map(str::to_owned),
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

fn feature(name: &str, module: &str) -> FoFeature {
    FoFeature {
        name: name.into(),
        module: module.into(),
    }
}

#[test]
fn fo001_fires_on_duplicate_public_type_across_features() {
    let air = air_with_files(vec![
        (
            "billing/user.rs",
            Some("crate::billing::user"),
            vec![ty("User", "x::billing::user::User", Visibility::Public)],
        ),
        (
            "identity/user.rs",
            Some("crate::identity::user"),
            vec![ty("User", "x::identity::user::User", Visibility::Public)],
        ),
    ]);
    let section = FoSection {
        features: vec![
            feature("billing", "crate::billing::*"),
            feature("identity", "crate::identity::*"),
        ],
        ..Default::default()
    };
    let diags = fo001(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "FO001");
    assert_eq!(diags[0].severity, Severity::Fatal);
    assert!(diags[0].message.contains("User"));
    assert!(diags[0].message.contains("billing"));
    assert!(diags[0].message.contains("identity"));
    assert_eq!(diags[0].concept.as_deref(), Some("User"));
    // why mentions both symbols and the incumbent feature.
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("x::identity::user::User"))
    );
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("x::billing::user::User"))
    );
}

#[test]
fn fo001_emits_one_diag_per_non_incumbent() {
    // 3 features defining `User` → 2 diagnostics (the incumbent is
    // whichever one we encounter first in iteration order).
    let air = air_with_files(vec![
        (
            "billing/user.rs",
            Some("crate::billing::user"),
            vec![ty("User", "x::billing::user::User", Visibility::Public)],
        ),
        (
            "identity/user.rs",
            Some("crate::identity::user"),
            vec![ty("User", "x::identity::user::User", Visibility::Public)],
        ),
        (
            "ops/user.rs",
            Some("crate::ops::user"),
            vec![ty("User", "x::ops::user::User", Visibility::Public)],
        ),
    ]);
    let section = FoSection {
        features: vec![
            feature("billing", "crate::billing::*"),
            feature("identity", "crate::identity::*"),
            feature("ops", "crate::ops::*"),
        ],
        ..Default::default()
    };
    let diags = fo001(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 2, "got {diags:?}");
    // Both subsequent definitions reference billing as the incumbent.
    for d in &diags {
        assert_eq!(d.rule_id, "FO001");
        assert!(d.message.contains("billing"));
        assert!(d.message.contains("User"));
    }
    // The two non-incumbent features should each appear once.
    let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert!(messages.iter().any(|m| m.contains("identity")));
    assert!(messages.iter().any(|m| m.contains("ops")));
}

#[test]
fn fo001_quiet_when_same_name_lives_in_same_feature() {
    // Two files inside `billing` both define `User` (a duplicate-symbol
    // problem for OT, not FO — this rule cares about ownership across
    // features, not within one).
    let air = air_with_files(vec![
        (
            "billing/user.rs",
            Some("crate::billing::user"),
            vec![ty("User", "x::billing::user::User", Visibility::Public)],
        ),
        (
            "billing/account.rs",
            Some("crate::billing::account"),
            vec![ty("User", "x::billing::account::User", Visibility::Public)],
        ),
    ]);
    let section = FoSection {
        features: vec![
            feature("billing", "crate::billing::*"),
            feature("identity", "crate::identity::*"),
        ],
        ..Default::default()
    };
    assert!(fo001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn fo001_quiet_when_duplicates_live_in_unfeatured_files() {
    // Neither file matches any feature — out of FO's jurisdiction.
    let air = air_with_files(vec![
        (
            "scripts/a.rs",
            Some("scripts::a"),
            vec![ty("User", "x::scripts::a::User", Visibility::Public)],
        ),
        (
            "scripts/b.rs",
            Some("scripts::b"),
            vec![ty("User", "x::scripts::b::User", Visibility::Public)],
        ),
        // One feature exists but doesn't include either file.
        (
            "billing/order.rs",
            Some("crate::billing::order"),
            vec![ty("Order", "x::billing::order::Order", Visibility::Public)],
        ),
    ]);
    let section = FoSection {
        features: vec![feature("billing", "crate::billing::*")],
        ..Default::default()
    };
    assert!(fo001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn fo001_silent_when_features_empty() {
    let air = air_with_files(vec![
        (
            "billing/user.rs",
            Some("crate::billing::user"),
            vec![ty("User", "x::billing::user::User", Visibility::Public)],
        ),
        (
            "identity/user.rs",
            Some("crate::identity::user"),
            vec![ty("User", "x::identity::user::User", Visibility::Public)],
        ),
    ]);
    let section = FoSection::default();
    assert!(fo001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn fo001_skips_private_types() {
    // Private types in either feature are out of scope: feature
    // ownership only applies to types another feature could plausibly
    // import.
    let air = air_with_files(vec![
        (
            "billing/user.rs",
            Some("crate::billing::user"),
            vec![ty("User", "x::billing::user::User", Visibility::Private)],
        ),
        (
            "identity/user.rs",
            Some("crate::identity::user"),
            vec![ty("User", "x::identity::user::User", Visibility::Public)],
        ),
    ]);
    let section = FoSection {
        features: vec![
            feature("billing", "crate::billing::*"),
            feature("identity", "crate::identity::*"),
        ],
        ..Default::default()
    };
    // Only one Public definition exists, so nothing fires.
    assert!(fo001(&air, &section, CheckMode::Human).is_empty());

    // And when both are private, still nothing fires.
    let air_both_private = air_with_files(vec![
        (
            "billing/user.rs",
            Some("crate::billing::user"),
            vec![ty("User", "x::billing::user::User", Visibility::Private)],
        ),
        (
            "identity/user.rs",
            Some("crate::identity::user"),
            vec![ty("User", "x::identity::user::User", Visibility::Private)],
        ),
    ]);
    assert!(fo001(&air_both_private, &section, CheckMode::Human).is_empty());
}

#[test]
fn fo001_agent_strict_keeps_fatal() {
    let air = air_with_files(vec![
        (
            "billing/user.rs",
            Some("crate::billing::user"),
            vec![ty("User", "x::billing::user::User", Visibility::Public)],
        ),
        (
            "identity/user.rs",
            Some("crate::identity::user"),
            vec![ty("User", "x::identity::user::User", Visibility::Public)],
        ),
    ]);
    let section = FoSection {
        features: vec![
            feature("billing", "crate::billing::*"),
            feature("identity", "crate::identity::*"),
        ],
        ..Default::default()
    };
    let diags = fo001(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(
        diags[0].severity,
        Severity::Fatal,
        "FO001 must remain Fatal under --agent-strict"
    );
}

#[test]
fn fo001_quiet_when_only_one_feature_defines_the_name() {
    // `Order` lives only in billing; `User` only in identity. No
    // collisions across features → no diagnostics.
    let air = air_with_files(vec![
        (
            "billing/order.rs",
            Some("crate::billing::order"),
            vec![ty("Order", "x::billing::order::Order", Visibility::Public)],
        ),
        (
            "identity/user.rs",
            Some("crate::identity::user"),
            vec![ty("User", "x::identity::user::User", Visibility::Public)],
        ),
    ]);
    let section = FoSection {
        features: vec![
            feature("billing", "crate::billing::*"),
            feature("identity", "crate::identity::*"),
        ],
        ..Default::default()
    };
    assert!(fo001(&air, &section, CheckMode::Human).is_empty());
}

// ---- FO004: shared type field references a feature-internal symbol ----

#[test]
fn fo004_fires_when_shared_field_names_feature_internal_type() {
    // `shared::dto::Receipt.line_items: Vec<crate::billing::Invoice>`
    // — billing leaks into shared.
    let air = air_with_files(vec![(
        "shared/dto.rs",
        Some("shared::dto"),
        vec![ty_with_fields(
            "Receipt",
            "shared::dto::Receipt",
            &[("line_items", "Vec<crate::billing::Invoice>")],
        )],
    )]);
    let section = FoSection {
        features: vec![feature("billing", "crate::billing::*")],
        shared_paths: vec!["shared::dto::*".into()],
    };
    let diags = fo004(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1, "got {diags:?}");
    assert_eq!(diags[0].rule_id, "FO004");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("Receipt"));
    assert!(diags[0].message.contains("billing"));
    assert!(diags[0].message.contains("line_items"));
    assert_eq!(diags[0].concept.as_deref(), Some("Receipt"));
}

#[test]
fn fo004_silent_when_shared_paths_empty() {
    let air = air_with_files(vec![(
        "shared/dto.rs",
        Some("shared::dto"),
        vec![ty_with_fields(
            "Receipt",
            "shared::dto::Receipt",
            &[("line_items", "Vec<crate::billing::Invoice>")],
        )],
    )]);
    let section = FoSection {
        features: vec![feature("billing", "crate::billing::*")],
        shared_paths: Vec::new(),
    };
    assert!(fo004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn fo004_silent_when_features_empty() {
    let air = air_with_files(vec![(
        "shared/dto.rs",
        Some("shared::dto"),
        vec![ty_with_fields(
            "Receipt",
            "shared::dto::Receipt",
            &[("line_items", "Vec<crate::billing::Invoice>")],
        )],
    )]);
    let section = FoSection {
        features: Vec::new(),
        shared_paths: vec!["shared::dto::*".into()],
    };
    assert!(fo004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn fo004_quiet_when_shared_field_uses_only_neutral_types() {
    // `Vec<u32>` and `String` don't match any feature's module pattern.
    let air = air_with_files(vec![(
        "shared/dto.rs",
        Some("shared::dto"),
        vec![ty_with_fields(
            "Receipt",
            "shared::dto::Receipt",
            &[("amount", "u64"), ("memo", "String")],
        )],
    )]);
    let section = FoSection {
        features: vec![feature("billing", "crate::billing::*")],
        shared_paths: vec!["shared::dto::*".into()],
    };
    assert!(fo004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn fo004_quiet_when_type_lives_outside_shared_paths() {
    // The type lives in `crate::billing` (a feature, not shared) — not
    // FO004's jurisdiction.
    let air = air_with_files(vec![(
        "billing/receipt.rs",
        Some("crate::billing::receipt"),
        vec![ty_with_fields(
            "Receipt",
            "crate::billing::receipt::Receipt",
            &[("invoice", "crate::billing::Invoice")],
        )],
    )]);
    let section = FoSection {
        features: vec![feature("billing", "crate::billing::*")],
        shared_paths: vec!["shared::dto::*".into()],
    };
    assert!(fo004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn fo004_agent_strict_elevates_to_fatal() {
    let air = air_with_files(vec![(
        "shared/dto.rs",
        Some("shared::dto"),
        vec![ty_with_fields(
            "Receipt",
            "shared::dto::Receipt",
            &[("invoice", "crate::billing::Invoice")],
        )],
    )]);
    let section = FoSection {
        features: vec![feature("billing", "crate::billing::*")],
        shared_paths: vec!["shared::dto::*".into()],
    };
    let diags = fo004(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn fo004_fires_per_field_with_feature_mention() {
    // Two fields, both leaking different feature-internal types.
    // Should fire twice (once per field).
    let air = air_with_files(vec![(
        "shared/dto.rs",
        Some("shared::dto"),
        vec![ty_with_fields(
            "Snapshot",
            "shared::dto::Snapshot",
            &[
                ("invoice", "crate::billing::Invoice"),
                ("user", "crate::identity::User"),
            ],
        )],
    )]);
    let section = FoSection {
        features: vec![
            feature("billing", "crate::billing::*"),
            feature("identity", "crate::identity::*"),
        ],
        shared_paths: vec!["shared::dto::*".into()],
    };
    let diags = fo004(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 2, "got {diags:?}");
    let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("invoice") && m.contains("billing"))
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("user") && m.contains("identity"))
    );
}
