//! Integration test: CL paradigm fires CL001 on the cl-claims fixture
//! exactly on the doc comments designed to be orphan references.

use locus_core::paradigms::claim_ownership::CL_PREFIX;
use locus_core::{CheckMode, Lockfile, Severity, registry};

fn fixture_path() -> std::path::PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    std::path::PathBuf::from(manifest)
        .join("../../tests/fixtures/cl-claims")
        .canonicalize()
        .expect("cl-claims fixture path resolves")
}

#[test]
fn cl_paradigm_is_registered() {
    let registry = registry();
    let prefixes: Vec<_> = registry.iter().map(|p| p.rule_prefix()).collect();
    assert!(
        prefixes.contains(&"CL"),
        "CL must be registered: {prefixes:?}"
    );
}

#[test]
fn cl001_silent_without_toggle_on_fixture() {
    // Without `paradigms.CL.require_local_rationale = true`, CL001 stays
    // silent even though the fixture has obvious orphan references.
    let air = locus_rust::scan(&fixture_path()).expect("scan succeeds");
    let lockfile = Lockfile::empty();
    let mut cl_diags = Vec::new();
    for paradigm in registry() {
        let diags = paradigm.check(&air, &lockfile, CheckMode::Human);
        cl_diags.extend(diags.into_iter().filter(|d| d.rule_id == "CL001"));
    }
    assert!(
        cl_diags.is_empty(),
        "CL001 must be silent until `require_local_rationale = true`; got {cl_diags:#?}",
    );
}

#[test]
fn cl001_fires_on_orphan_references_in_fixture() {
    // With the toggle on, CL001 should fire on the orphan-shaped doc
    // comments and stay quiet on the rationalised ones.
    let air = locus_rust::scan(&fixture_path()).expect("scan succeeds");
    let mut lockfile = Lockfile::empty();
    lockfile.paradigms.insert(
        CL_PREFIX.to_string(),
        serde_json::json!({"require_local_rationale": true}),
    );

    let mut cl_diags = Vec::new();
    for paradigm in registry() {
        let diags = paradigm.check(&air, &lockfile, CheckMode::Human);
        cl_diags.extend(diags.into_iter().filter(|d| d.rule_id == "CL001"));
    }

    let messages: Vec<&str> = cl_diags.iter().map(|d| d.message.as_str()).collect();

    // Each orphan item — three structs and one function — should fire.
    assert!(
        messages.iter().any(|m| m.contains("OrphanIssueRef")),
        "expected CL001 on OrphanIssueRef; got {messages:#?}",
    );
    assert!(
        messages.iter().any(|m| m.contains("OrphanUrlRef")),
        "expected CL001 on OrphanUrlRef; got {messages:#?}",
    );
    assert!(
        messages.iter().any(|m| m.contains("orphan_function_ref")),
        "expected CL001 on orphan_function_ref; got {messages:#?}",
    );

    // The rationalised items must stay quiet.
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("ReferenceWithRationale")),
        "ReferenceWithRationale has local rationale; CL001 must not fire. got {messages:#?}",
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("ReferenceWithLongRationale")),
        "ReferenceWithLongRationale has local rationale; CL001 must not fire. got {messages:#?}",
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("function_with_rationale")),
        "function_with_rationale has local rationale; CL001 must not fire. got {messages:#?}",
    );
    assert!(
        !messages.iter().any(|m| m.contains("PlainDoc")),
        "PlainDoc has no references at all; CL001 must not fire. got {messages:#?}",
    );

    // Severity sanity check on at least one diagnostic.
    if let Some(d) = cl_diags.first() {
        assert_eq!(d.severity, Severity::Warning);
    }
}

#[test]
fn cl001_agent_strict_elevates_on_fixture() {
    let air = locus_rust::scan(&fixture_path()).expect("scan succeeds");
    let mut lockfile = Lockfile::empty();
    lockfile.paradigms.insert(
        CL_PREFIX.to_string(),
        serde_json::json!({"require_local_rationale": true}),
    );

    let mut cl_diags = Vec::new();
    for paradigm in registry() {
        let diags = paradigm.check(&air, &lockfile, CheckMode::AgentStrict);
        cl_diags.extend(diags.into_iter().filter(|d| d.rule_id == "CL001"));
    }
    assert!(!cl_diags.is_empty());
    assert!(
        cl_diags.iter().all(|d| d.severity == Severity::Fatal),
        "every CL001 must elevate to Fatal under agent-strict; got {cl_diags:#?}",
    );
}
