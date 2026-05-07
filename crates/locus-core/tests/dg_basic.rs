//! Integration test: DG paradigm dispatches alongside OT through the same
//! registry, and DG001 fires on the fixture when a forbidden edge covers a
//! real import.

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
    let mut diags = Vec::new();
    for paradigm in registry() {
        diags.extend(paradigm.check(&air, &lockfile, CheckMode::Human));
    }
    assert!(
        diags.iter().all(|d| d.rule_id != "DG001"),
        "DG001 must not fire without forbidden_edges; got {diags:?}"
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

    let mut diags = Vec::new();
    for paradigm in registry() {
        diags.extend(paradigm.check(&air, &lockfile, CheckMode::Human));
    }
    let dg001: Vec<_> = diags.iter().filter(|d| d.rule_id == "DG001").collect();
    assert!(
        !dg001.is_empty(),
        "expected DG001 to fire on handler.rs's UserDto import; got nothing in {diags:?}"
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

#[test]
fn ot_and_dg_diagnostics_coexist() {
    // The fixture's handler.rs trips OT003 + OT004 (after OT init) AND DG001
    // (with our forbidden edge). One paradigm shouldn't suppress the other.
    let air = locus_rust::scan(&fixture_path()).expect("scan succeeds");
    let registry = registry();
    let mut lockfile = Lockfile::empty();
    for p in &registry {
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

    let mut diags = Vec::new();
    for p in &registry {
        diags.extend(p.check(&air, &lockfile, CheckMode::Human));
    }
    let rule_ids: Vec<&str> = diags.iter().map(|d| d.rule_id.as_str()).collect();
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
