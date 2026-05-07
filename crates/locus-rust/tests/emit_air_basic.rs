//! Integration test: scan the in-tree sample-crate fixture.
//!
//! Asserts shape (counts and key facts) rather than snapshotting the full
//! AIR — the fixture is small enough to enumerate, and counts are a more
//! readable failure mode than a JSON diff.

use locus_air::{ActionKind, AirItem, ConversionMechanism, HintKind, TypeKind};

fn fixture_path() -> std::path::PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    std::path::PathBuf::from(manifest)
        .join("../../tests/fixtures/sample-crate")
        .canonicalize()
        .expect("fixture path resolves")
}

#[test]
fn scans_sample_crate() {
    let air = locus_rust::scan(&fixture_path()).expect("scan succeeds");

    assert_eq!(air.schema_version, locus_air::AIR_SCHEMA_VERSION);
    assert_eq!(air.packages.len(), 1, "expect exactly one package");

    let pkg = &air.packages[0];
    assert_eq!(pkg.name, "sample-crate");

    let by_path: std::collections::HashMap<_, _> = pkg
        .files
        .iter()
        .map(|f| (f.path.split('/').next_back().unwrap(), f))
        .collect();

    let identity = by_path.get("identity.rs").expect("identity.rs scanned");
    assert_eq!(
        identity.module_path.as_deref(),
        Some("sample_crate::identity"),
        "module path derived from src/identity.rs"
    );
    let identity_types: Vec<&str> = identity
        .items
        .iter()
        .filter_map(|i| match i {
            AirItem::Type(t) => Some(t.name.as_str()),
            _ => None,
        })
        .collect();
    assert!(identity_types.contains(&"User"));
    assert!(identity_types.contains(&"UserId"));
    assert!(identity_types.contains(&"UserStatus"));

    let user_status = identity
        .items
        .iter()
        .find_map(|i| match i {
            AirItem::Type(t) if t.name == "UserStatus" => Some(t),
            _ => None,
        })
        .expect("UserStatus emitted");
    assert_eq!(user_status.kind, TypeKind::Enum);
    assert_eq!(user_status.variants.len(), 3);

    // identity.rs has the canonical hint.
    assert!(
        identity
            .hints
            .iter()
            .any(|h| matches!(h.kind, HintKind::Canonical)),
        "expected `// ot: canonical` hint on identity.rs, got {:?}",
        identity.hints
    );

    let dto = by_path.get("dto.rs").expect("dto.rs scanned");
    assert_eq!(dto.module_path.as_deref(), Some("sample_crate::dto"));

    let conversions: Vec<&locus_air::AirConversion> = dto
        .items
        .iter()
        .filter_map(|i| match i {
            AirItem::Conversion(c) => Some(c),
            _ => None,
        })
        .collect();

    assert!(
        conversions
            .iter()
            .any(|c| c.mechanism == ConversionMechanism::TryFrom && c.to == "User"),
        "expect TryFrom<UserDto> for User; got {:?}",
        conversions
    );
    assert!(
        conversions
            .iter()
            .any(|c| c.mechanism == ConversionMechanism::FreeFn
                && c.symbol.ends_with("::map_user")),
        "expect map_user free-fn converter; got {:?}",
        conversions
    );

    // ot: boundary hint with concept + boundary args.
    let boundary_hint = dto
        .hints
        .iter()
        .find(|h| matches!(h.kind, HintKind::Boundary { .. }))
        .expect("boundary hint emitted");
    if let HintKind::Boundary { concept, boundary } = &boundary_hint.kind {
        assert_eq!(concept.as_deref(), Some("identity.user"));
        assert_eq!(boundary.as_deref(), Some("api.v1"));
    }
    assert!(
        boundary_hint.target_span.is_some(),
        "boundary hint should bind to a target line"
    );

    // Construct truth action from `User { ... }` literal in map_user.
    let constructs: Vec<&locus_air::AirTruthAction> = dto
        .items
        .iter()
        .filter_map(|i| match i {
            AirItem::TruthAction(a) if a.action == ActionKind::Construct => Some(a),
            _ => None,
        })
        .collect();
    assert!(
        constructs.iter().any(|a| a.target == "User"),
        "expect Construct(User) in map_user; got {:?}",
        constructs
    );

    // is_active_status compares status against a string literal.
    let str_compares: Vec<&locus_air::AirTruthAction> = dto
        .items
        .iter()
        .filter_map(|i| match i {
            AirItem::TruthAction(a) if a.action == ActionKind::StringCompare => Some(a),
            _ => None,
        })
        .collect();
    assert!(
        !str_compares.is_empty(),
        "expect at least one StringCompare truth action"
    );
}
