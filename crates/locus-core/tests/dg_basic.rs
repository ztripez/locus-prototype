//! Integration test: DG paradigm dispatches alongside OT through the same
//! registry, and DG001 fires on the fixture when a forbidden edge covers a
//! real import.

use locus_core::governance;
use locus_core::paradigms::dependency_graph::DG_PREFIX;
use locus_core::{CheckMode, Lockfile, Severity, registry};

fn fixture_path() -> std::path::PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    std::path::PathBuf::from(manifest)
        .join("../../tests/fixtures/sample-crate")
        .canonicalize()
        .expect("fixture path resolves")
}

#[test]
fn dg_paradigm_is_registered() {
    let registry = registry();
    let prefixes: Vec<_> = registry.iter().map(|p| p.rule_prefix()).collect();
    assert!(prefixes.contains(&"OT"), "OT registered: {prefixes:?}");
    assert!(prefixes.contains(&"DG"), "DG registered: {prefixes:?}");
}

#[test]
fn dg_silent_with_empty_section() {
    // No DG section → DG paradigm runs but emits nothing, even though the
    // fixture has imports that *could* be forbidden if declared.
    let air = locus_rust::scan(&fixture_path()).expect("scan succeeds");
    let lockfile = Lockfile::empty();
    let out = governance::run(&air, &lockfile, CheckMode::Human);
    assert!(
        out.diagnostics.iter().all(|d| d.rule_id != "DG001"),
        "DG001 must not fire without forbidden_edges; got {:?}",
        out.diagnostics
    );
}

#[test]
fn dg001_fires_when_handler_imports_dto() {
    // Declare: handler module must not reach the dto boundary directly. The
    // fixture's `handler.rs` does exactly that, so DG001 should fire on its
    // `use sample_crate::dto::UserDto` line.
    let air = locus_rust::scan(&fixture_path()).expect("scan succeeds");
    let mut lockfile = Lockfile::empty();
    let dg = serde_json::json!({
        "forbidden_edges": [
            {
                "from": "sample_crate::handler",
                "to": "sample_crate::dto::*",
                "reason": "handlers must convert DTOs at the edge, not consume them directly"
            }
        ]
    });
    lockfile.paradigms.insert(DG_PREFIX.to_string(), dg);

    let out = governance::run(&air, &lockfile, CheckMode::Human);
    let dg001: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "DG001")
        .collect();
    assert!(
        !dg001.is_empty(),
        "expected DG001 to fire on handler.rs's UserDto import; got nothing in {:?}",
        out.diagnostics
    );
    let target = dg001
        .iter()
        .find(|d| d.message.contains("UserDto"))
        .expect("UserDto-bound diagnostic");
    assert_eq!(target.severity, Severity::Fatal);
    assert!(
        target.span.file.ends_with("handler.rs"),
        "span should land on handler.rs, got {}",
        target.span.file
    );
    assert!(
        target
            .why
            .iter()
            .any(|w| w.contains("handlers must convert DTOs at the edge")),
        "reason should travel into `why`; got {:?}",
        target.why
    );
}

/// Two-feature fixture exercising DG003's central rule:
/// imports through a feature's declared `public_api` surface are allowed,
/// imports that bypass it into the feature's internals are rejected.
///
/// Fixture layout:
///   `dg_public_api::feature_one::api`        — public surface
///   `dg_public_api::feature_one::internals`  — private; not in `public_api`
///   `dg_public_api::feature_two::handler`    — consumer
///
/// `handler` imports both `feature_one::api::PublicThing` (legal) and
/// `feature_one::internals::secret` (illegal). With `feature_one`'s
/// `public_api` set to `dg_public_api::feature_one::api::*`, exactly one
/// DG003 must fire — on the internals reach.
#[test]
fn dg003_allows_public_api_blocks_internals_reach() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let fixture = std::path::PathBuf::from(manifest)
        .join("../../tests/fixtures/dg-public-api")
        .canonicalize()
        .expect("dg-public-api fixture resolves");
    let air = locus_rust::scan(&fixture).expect("scan succeeds");

    let mut lockfile = Lockfile::empty();
    lockfile.paradigms.insert(
        DG_PREFIX.to_string(),
        serde_json::json!({
            "features": [
                {
                    "name": "feature_one",
                    "module": "dg_public_api::feature_one::*",
                    "public_api": ["dg_public_api::feature_one::api::*"],
                },
                {
                    "name": "feature_two",
                    "module": "dg_public_api::feature_two::*",
                    "public_api": ["dg_public_api::feature_two::*"],
                },
            ],
        }),
    );

    let out = governance::run(&air, &lockfile, CheckMode::Human);
    let dg003: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "DG003")
        .collect();

    assert_eq!(
        dg003.len(),
        1,
        "expected exactly one DG003 (the internals reach), got {}: {:#?}",
        dg003.len(),
        dg003,
    );
    let d = dg003[0];
    assert!(
        d.message.contains("internals"),
        "DG003 should name the internals path; got `{}`",
        d.message,
    );
    assert!(
        d.span.file.ends_with("handler.rs"),
        "DG003 should land on handler.rs; got `{}`",
        d.span.file,
    );
}

#[test]
fn ot_and_dg_diagnostics_coexist() {
    // The fixture's handler.rs trips OT003 + OT004 (after OT init) AND DG001
    // (with our forbidden edge). One paradigm shouldn't suppress the other.
    let air = locus_rust::scan(&fixture_path()).expect("scan succeeds");
    let mut lockfile = Lockfile::empty();
    for p in registry() {
        let section = p.init(&air);
        if !section.is_null() {
            lockfile
                .paradigms
                .insert(p.rule_prefix().to_string(), section);
        }
    }
    // Inject a DG forbidden edge on top of init's output.
    lockfile.paradigms.insert(
        DG_PREFIX.to_string(),
        serde_json::json!({
            "forbidden_edges": [
                { "from": "sample_crate::handler", "to": "sample_crate::dto::*" }
            ]
        }),
    );

    let out = governance::run(&air, &lockfile, CheckMode::Human);
    let rule_ids: Vec<&str> = out.diagnostics.iter().map(|d| d.rule_id.as_str()).collect();
    assert!(
        rule_ids.contains(&"OT003"),
        "expected OT003 in {rule_ids:?}"
    );
    assert!(
        rule_ids.contains(&"OT004"),
        "expected OT004 in {rule_ids:?}"
    );
    assert!(
        rule_ids.contains(&"DG001"),
        "expected DG001 in {rule_ids:?}"
    );
}
