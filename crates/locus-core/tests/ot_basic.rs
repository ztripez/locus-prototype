//! Integration test: scan the sample-crate fixture and run the OT paradigm.
//! Asserts exactly one OT002 diagnostic, fired on `UserModel`.

use locus_core::paradigms::one_truth::OT_PREFIX;
use locus_core::paradigms::one_truth::lockfile_schema::OtSection;
use locus_core::{CheckMode, Lockfile, Severity, registry};

fn fixture_path() -> std::path::PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    std::path::PathBuf::from(manifest)
        .join("../../tests/fixtures/sample-crate")
        .canonicalize()
        .expect("fixture path resolves")
}

#[test]
fn ot002_fires_on_user_model_only() {
    let air = locus_rust::scan(&fixture_path()).expect("scan succeeds");
    let lockfile = Lockfile::empty();
    let mut diags = Vec::new();
    for paradigm in registry() {
        diags.extend(paradigm.check(&air, &lockfile, CheckMode::Human));
    }

    let ot002: Vec<_> = diags.iter().filter(|d| d.rule_id == "OT002").collect();
    assert_eq!(
        ot002.len(),
        1,
        "expected exactly one OT002 diagnostic; got {} ({:?})",
        ot002.len(),
        ot002
    );

    let d = ot002[0];
    assert!(
        d.message.contains("UserModel"),
        "OT002 should target UserModel; message: {}",
        d.message
    );
    assert_eq!(d.concept.as_deref(), Some("user"));
    assert_eq!(d.severity, Severity::Warning, "fatal only in agent-strict");
    assert!(
        d.span.file.ends_with("shadow.rs"),
        "span should point at shadow.rs, got {}",
        d.span.file
    );
    assert!(
        d.suggested_fix
            .as_ref()
            .is_some_and(|f| f.contains("// locus: ot boundary")),
        "fix should suggest the boundary annotation; got {:?}",
        d.suggested_fix
    );
}

#[test]
fn agent_strict_makes_ot002_fatal() {
    let air = locus_rust::scan(&fixture_path()).expect("scan succeeds");
    let lockfile = Lockfile::empty();
    let mut diags = Vec::new();
    for paradigm in registry() {
        diags.extend(paradigm.check(&air, &lockfile, CheckMode::AgentStrict));
    }
    let ot002: Vec<_> = diags.iter().filter(|d| d.rule_id == "OT002").collect();
    assert_eq!(ot002.len(), 1);
    assert_eq!(ot002[0].severity, Severity::Fatal);
}

#[test]
fn init_promotes_annotated_canonical_and_boundary_into_lockfile() {
    let air = locus_rust::scan(&fixture_path()).expect("scan succeeds");
    let registry = registry();
    let ot = registry
        .iter()
        .find(|p| p.rule_prefix() == OT_PREFIX)
        .expect("OT registered");
    let value = ot.init(&air);
    let section: OtSection = serde_json::from_value(value).expect("OT section deserializes");

    let user = section.concepts.get("user").expect("`user` concept");
    assert_eq!(user.canonical.symbol, "sample_crate::identity::User");
    let boundary_symbols: Vec<_> = user.boundaries.iter().map(|b| b.symbol.as_str()).collect();
    assert!(
        boundary_symbols.contains(&"sample_crate::dto::UserDto"),
        "UserDto must be accepted as boundary; got {boundary_symbols:?}"
    );
    assert!(
        !boundary_symbols.contains(&"sample_crate::shadow::UserModel"),
        "UserModel is unannotated; must NOT be in the lockfile"
    );
    assert!(
        user.boundaries
            .iter()
            .any(|b| b.boundary.as_deref() == Some("api.v1")),
        "boundary label `api.v1` should be carried over from the hint"
    );
    assert!(
        !user.converters.is_empty(),
        "expected at least one converter (TryFrom<UserDto> for User)"
    );
}

#[test]
fn check_against_populated_lockfile_still_flags_user_model() {
    // After `locus init`, the lockfile knows about User + UserDto. UserModel
    // remains Unknown and must still trigger OT002.
    let air = locus_rust::scan(&fixture_path()).expect("scan succeeds");
    let registry = registry();
    let mut lockfile = Lockfile::empty();
    for p in &registry {
        let section = p.init(&air);
        lockfile
            .paradigms
            .insert(p.rule_prefix().to_string(), section);
    }

    let mut diags = Vec::new();
    for p in &registry {
        diags.extend(p.check(&air, &lockfile, CheckMode::Human));
    }
    let ot002: Vec<_> = diags.iter().filter(|d| d.rule_id == "OT002").collect();
    assert_eq!(ot002.len(), 1, "expected exactly one OT002 (UserModel)");
    assert!(ot002[0].message.contains("UserModel"));
}

#[test]
fn lockfile_only_acceptance_blocks_ot002_even_without_hint() {
    // Build a lockfile that pretends UserModel is the canonical for `user`,
    // and User is a boundary. This is contrived, but proves the check
    // consults the lockfile and not just hints.
    let air = locus_rust::scan(&fixture_path()).expect("scan succeeds");
    let mut lockfile = Lockfile::empty();
    let section = serde_json::json!({
        "concepts": {
            "user": {
                "canonical": { "symbol": "sample_crate::shadow::UserModel", "source": "accepted" },
                "boundaries": [
                    { "symbol": "sample_crate::identity::User", "boundary": null, "source": "accepted" },
                    { "symbol": "sample_crate::dto::UserDto", "boundary": "api.v1", "source": "accepted" }
                ],
                "converters": []
            }
        }
    });
    lockfile.paradigms.insert(OT_PREFIX.to_string(), section);

    let registry = registry();
    let mut diags = Vec::new();
    for p in &registry {
        diags.extend(p.check(&air, &lockfile, CheckMode::Human));
    }
    let ot002: Vec<_> = diags.iter().filter(|d| d.rule_id == "OT002").collect();
    assert!(
        ot002.is_empty(),
        "lockfile-accepted symbols must not appear in OT002; got {ot002:?}"
    );
}

#[test]
fn no_ot002_for_accepted_canonical_or_boundary() {
    // User (canonical) and UserDto (boundary) are both annotated, so they
    // must not appear in OT002 diagnostics.
    let air = locus_rust::scan(&fixture_path()).expect("scan succeeds");
    let lockfile = Lockfile::empty();
    let mut diags = Vec::new();
    for paradigm in registry() {
        diags.extend(paradigm.check(&air, &lockfile, CheckMode::Human));
    }
    for d in diags.iter().filter(|d| d.rule_id == "OT002") {
        assert!(
            !d.message.contains("crate::User\""),
            "User must not be flagged: {}",
            d.message
        );
        assert!(
            !d.message.contains("UserDto"),
            "UserDto is accepted boundary, must not be flagged: {}",
            d.message
        );
    }
}

#[test]
fn baseline_fixture_has_no_ot001() {
    // The fixture has exactly one `// locus: ot canonical` annotation. OT001 must
    // not fire on it; OT001 firing here would mean the cluster has accidentally
    // promoted a second canonical, which is a regression in inference.
    let air = locus_rust::scan(&fixture_path()).expect("scan succeeds");
    let lockfile = Lockfile::empty();
    let mut diags = Vec::new();
    for paradigm in registry() {
        diags.extend(paradigm.check(&air, &lockfile, CheckMode::Human));
    }
    let ot001: Vec<_> = diags.iter().filter(|d| d.rule_id == "OT001").collect();
    assert!(
        ot001.is_empty(),
        "OT001 must not fire on the baseline fixture; got {ot001:?}"
    );
}

#[test]
fn ot006_fires_on_unaccepted_conversion_after_partial_lockfile() {
    // Build a lockfile that has User + UserDto accepted but NO converters.
    // The fixture's `impl TryFrom<UserDto> for User` and `map_user` both go
    // between accepted endpoints, so OT006 should fire on each.
    let air = locus_rust::scan(&fixture_path()).expect("scan succeeds");
    let mut lockfile = Lockfile::empty();
    let section = serde_json::json!({
        "concepts": {
            "user": {
                "canonical": { "symbol": "sample_crate::identity::User", "source": "accepted" },
                "boundaries": [
                    { "symbol": "sample_crate::dto::UserDto", "boundary": "api.v1", "source": "accepted" }
                ],
                "converters": []
            }
        }
    });
    lockfile.paradigms.insert(OT_PREFIX.to_string(), section);

    let mut diags = Vec::new();
    for p in registry() {
        diags.extend(p.check(&air, &lockfile, CheckMode::Human));
    }
    let ot006: Vec<_> = diags.iter().filter(|d| d.rule_id == "OT006").collect();
    assert_eq!(
        ot006.len(),
        2,
        "expected OT006 on both fixture conversions when the lockfile has no converters; got {ot006:?}"
    );
    assert!(ot006.iter().any(|d| d.message.contains("TryFrom")));
    assert!(ot006.iter().any(|d| d.message.contains("map_user")));
    assert!(ot006.iter().all(|d| d.severity == Severity::Warning));
}

#[test]
fn ot003_fires_on_handler_after_init() {
    // After `locus init`, the lockfile knows User canonical + UserDto boundary.
    // The fixture's `handler::create_user` lives in a non-boundary file,
    // isn't an accepted converter, and takes UserDto in its signature.
    let air = locus_rust::scan(&fixture_path()).expect("scan succeeds");
    let registry = registry();
    let mut lockfile = Lockfile::empty();
    for p in &registry {
        lockfile
            .paradigms
            .insert(p.rule_prefix().to_string(), p.init(&air));
    }

    let mut diags = Vec::new();
    for p in &registry {
        diags.extend(p.check(&air, &lockfile, CheckMode::Human));
    }
    let ot003: Vec<_> = diags.iter().filter(|d| d.rule_id == "OT003").collect();
    assert!(
        ot003
            .iter()
            .any(|d| d.message.contains("create_user") && d.message.contains("UserDto")),
        "expected OT003 on handler::create_user for UserDto; got {ot003:?}"
    );
    assert!(ot003.iter().all(|d| d.severity == Severity::Fatal));
}

#[test]
fn ot004_fires_on_handler_after_init() {
    // Same setup as OT003: the User { ... } literal inside `handler::create_user`
    // is not in the owner module (identity.rs) and the function isn't an
    // accepted converter — Fatal OT004.
    let air = locus_rust::scan(&fixture_path()).expect("scan succeeds");
    let registry = registry();
    let mut lockfile = Lockfile::empty();
    for p in &registry {
        lockfile
            .paradigms
            .insert(p.rule_prefix().to_string(), p.init(&air));
    }

    let mut diags = Vec::new();
    for p in &registry {
        diags.extend(p.check(&air, &lockfile, CheckMode::Human));
    }
    let ot004: Vec<_> = diags.iter().filter(|d| d.rule_id == "OT004").collect();
    assert!(
        ot004.iter().any(|d| d.span.file.ends_with("handler.rs")
            && d.message.contains("sample_crate::identity::User")),
        "expected OT004 on handler::create_user constructing User; got {ot004:?}"
    );
}

#[test]
fn baseline_fixture_no_ot003_or_ot004_pre_init() {
    // Without a populated lockfile, OT003/OT004 are silent regardless of how
    // bad the source looks. This is the deliberate "stay out of the user's way
    // until they've onboarded" behavior.
    let air = locus_rust::scan(&fixture_path()).expect("scan succeeds");
    let lockfile = Lockfile::empty();
    let mut diags = Vec::new();
    for p in registry() {
        diags.extend(p.check(&air, &lockfile, CheckMode::Human));
    }
    assert!(
        diags
            .iter()
            .all(|d| d.rule_id != "OT003" && d.rule_id != "OT004"),
        "OT003/OT004 must stay silent without a populated lockfile; got {diags:?}"
    );
}

#[test]
fn ot006_quiet_after_init_promotes_converters() {
    // When init is run, both fixture converters get promoted into the
    // lockfile, so OT006 must NOT fire on the fixture.
    let air = locus_rust::scan(&fixture_path()).expect("scan succeeds");
    let registry = registry();
    let mut lockfile = Lockfile::empty();
    for p in &registry {
        let section = p.init(&air);
        lockfile
            .paradigms
            .insert(p.rule_prefix().to_string(), section);
    }

    let mut diags = Vec::new();
    for p in &registry {
        diags.extend(p.check(&air, &lockfile, CheckMode::Human));
    }
    let ot006: Vec<_> = diags.iter().filter(|d| d.rule_id == "OT006").collect();
    assert!(
        ot006.is_empty(),
        "init should auto-accept the fixture converters; got {ot006:?}"
    );
}
