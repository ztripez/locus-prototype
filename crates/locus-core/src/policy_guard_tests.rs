//! Tests for [`super`] policy_guard module.
//!
//! Sibling-attached via `#[path = "policy_guard_tests.rs"] mod tests;`
//! at the bottom of `policy_guard.rs`.

use super::*;
use crate::lockfile::{AcknowledgedEmpty, AcknowledgedEmptyEntry, Lockfile};

fn lockfile_with(paradigms: serde_json::Value, ack: Vec<AcknowledgedEmptyEntry>) -> Lockfile {
    let mut lf = Lockfile::empty();
    if let Some(obj) = paradigms.as_object() {
        for (k, v) in obj {
            lf.paradigms.insert(k.clone(), v.clone());
        }
    }
    lf.acknowledged_empty = ack;
    lf
}

/// Helper: build a `Vec<AcknowledgedEmptyEntry>` of legacy strings.
fn ack_legacy(prefixes: &[&str]) -> Vec<AcknowledgedEmptyEntry> {
    prefixes
        .iter()
        .map(|s| AcknowledgedEmptyEntry::Legacy(s.to_string()))
        .collect()
}

/// Helper: build a single Full entry with all metadata.
fn ack_full(prefix: &str) -> AcknowledgedEmptyEntry {
    AcknowledgedEmptyEntry::Full(AcknowledgedEmpty {
        prefix: prefix.to_string(),
        expires: Some("2027-05-09".to_string()),
        reason: Some("test reason".to_string()),
        owner: Some("@test".to_string()),
        debt_id: Some(format!("ack-empty-{prefix}")),
        introduced_by: Some("PR #49".to_string()),
    })
}

/// Helper: build a Full entry without any metadata.
fn ack_full_no_metadata(prefix: &str) -> AcknowledgedEmptyEntry {
    AcknowledgedEmptyEntry::Full(AcknowledgedEmpty {
        prefix: prefix.to_string(),
        ..Default::default()
    })
}

// ---- PG000 baseline missing --------------------------------------

#[test]
fn pg000_fires_when_baseline_is_none() {
    let cur = lockfile_with(serde_json::json!({}), vec![]);
    let diags = check_policy_mutation(&cur, None, CheckMode::Human, false, false);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "PG000");
}

#[test]
fn pg000_silent_when_allow_missing_baseline() {
    let cur = lockfile_with(serde_json::json!({}), vec![]);
    let diags = check_policy_mutation(&cur, None, CheckMode::AgentStrict, false, true);
    assert!(
        diags.is_empty(),
        "explicit acknowledgement should silence PG000; got {diags:#?}",
    );
}

#[test]
fn pg000_is_fatal_under_agent_strict() {
    let cur = lockfile_with(serde_json::json!({}), vec![]);
    let diags = check_policy_mutation(&cur, None, CheckMode::AgentStrict, false, false);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn pg000_is_fatal_under_agent_strict_regardless_of_calibration() {
    // Calibration is for legitimate widening, not for accepting a
    // missing audit. PG000 must stay Fatal under strict even when
    // calibration is set.
    let cur = lockfile_with(serde_json::json!({}), vec![]);
    let diags = check_policy_mutation(&cur, None, CheckMode::AgentStrict, true, false);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "PG000");
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn identical_lockfiles_yield_no_diagnostics() {
    let lf = lockfile_with(
        serde_json::json!({"CX": {"default_max_function_lines": 200}}),
        ack_legacy(&["BO"]),
    );
    let diags = check_policy_mutation(&lf, Some(&lf), CheckMode::AgentStrict, false, false);
    assert!(
        diags.is_empty(),
        "no drift; PG should stay quiet. got {diags:#?}"
    );
}

// ---- PG001 default-budget raise ----------------------------------

#[test]
fn pg001_fires_when_default_max_function_lines_raised() {
    let base = lockfile_with(serde_json::json!({"CX": {}}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({"CX": {"default_max_function_lines": 120}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false, false);
    let pg001: Vec<_> = diags.iter().filter(|d| d.rule_id == "PG001").collect();
    assert_eq!(pg001.len(), 1);
    assert!(
        pg001[0].message.contains("default_max_function_lines"),
        "got {}",
        pg001[0].message
    );
    assert!(
        pg001[0].message.contains("50") && pg001[0].message.contains("120"),
        "msg should show 50 → 120, got {}",
        pg001[0].message
    );
}

#[test]
fn pg001_does_not_fire_when_budget_lowered() {
    let base = lockfile_with(
        serde_json::json!({"CX": {"default_max_function_lines": 120}}),
        vec![],
    );
    let cur = lockfile_with(
        serde_json::json!({"CX": {"default_max_function_lines": 80}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false, false);
    assert!(
        diags.iter().all(|d| d.rule_id != "PG001"),
        "tightening shouldn't fire PG001; got {diags:#?}"
    );
}

#[test]
fn pg001_fires_under_agent_strict_as_fatal() {
    let base = lockfile_with(serde_json::json!({"CX": {}}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({"CX": {"default_max_function_lines": 120}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);
    let pg001 = diags.iter().find(|d| d.rule_id == "PG001").unwrap();
    assert_eq!(pg001.severity, Severity::Fatal);
}

#[test]
fn pg001_advisory_under_calibration() {
    let base = lockfile_with(serde_json::json!({"CX": {}}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({"CX": {"default_max_function_lines": 120}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, true, false);
    let pg001 = diags.iter().find(|d| d.rule_id == "PG001").unwrap();
    assert_eq!(
        pg001.severity,
        Severity::Advisory,
        "calibration mode should downgrade PG001 to Advisory regardless of mode"
    );
}

#[test]
fn pg001_fires_for_module_line_budget_raise() {
    let base = lockfile_with(
        serde_json::json!({"CX": {"default_max_module_lines": 400}}),
        vec![],
    );
    let cur = lockfile_with(
        serde_json::json!({"CX": {"default_max_module_lines": 700}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false, false);
    let pg001 = diags.iter().find(|d| d.rule_id == "PG001").unwrap();
    assert!(pg001.message.contains("default_max_module_lines"));
}

// ---- PG001 existing-override budget raise ------------------------

#[test]
fn pg001_fires_when_existing_cx_override_budget_raised() {
    // The slipperier cheat: the override module is already in the
    // lockfile, only the budget number changes. PG002 keys by module,
    // so it stays quiet — PG001 must catch the budget delta.
    let existing = serde_json::json!({"CX": {"overrides": [
        {"module": "locus_rust::visitor", "max_function_lines": 300,
         "reason": "AST", "expires": "2026-06-01", "owner": "@core"}
    ]}});
    let cur_paradigms = serde_json::json!({"CX": {"overrides": [
        {"module": "locus_rust::visitor", "max_function_lines": 10000,
         "reason": "AST", "expires": "2026-06-01", "owner": "@core"}
    ]}});
    let base = lockfile_with(existing, vec![]);
    let cur = lockfile_with(cur_paradigms, vec![]);
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);
    let pg001: Vec<_> = diags.iter().filter(|d| d.rule_id == "PG001").collect();
    assert_eq!(pg001.len(), 1, "expected one PG001; got {pg001:#?}");
    let m = pg001[0].message.as_str();
    assert!(m.contains("locus_rust::visitor"), "msg names module: {m}");
    assert!(m.contains("max_function_lines"));
    assert!(m.contains("300"));
    assert!(m.contains("10000"));
    assert_eq!(
        pg001[0].severity,
        Severity::Fatal,
        "existing-override budget raise must be Fatal under strict",
    );
}

#[test]
fn pg001_fires_when_existing_module_override_budget_raised() {
    let base = lockfile_with(
        serde_json::json!({"CX": {"module_overrides": [
            {"module": "foo::*", "max_module_lines": 1000,
             "reason": "x", "expires": "2026-06-01", "owner": "@x"}
        ]}}),
        vec![],
    );
    let cur = lockfile_with(
        serde_json::json!({"CX": {"module_overrides": [
            {"module": "foo::*", "max_module_lines": 5000,
             "reason": "x", "expires": "2026-06-01", "owner": "@x"}
        ]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false, false);
    let pg001 = diags.iter().find(|d| d.rule_id == "PG001").unwrap();
    assert!(pg001.message.contains("max_module_lines"));
    assert!(pg001.message.contains("foo::*"));
}

#[test]
fn pg001_fires_when_existing_mo_override_budget_raised() {
    let base = lockfile_with(
        serde_json::json!({"MO": {"overrides": [
            {"module": "x::*", "max_public_types": 10,
             "reason": "y", "expires": "2026-06-01", "owner": "@y"}
        ]}}),
        vec![],
    );
    let cur = lockfile_with(
        serde_json::json!({"MO": {"overrides": [
            {"module": "x::*", "max_public_types": 100,
             "reason": "y", "expires": "2026-06-01", "owner": "@y"}
        ]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false, false);
    let pg001 = diags.iter().find(|d| d.rule_id == "PG001").unwrap();
    assert!(pg001.message.contains("max_public_types"));
}

#[test]
fn pg001_quiet_when_existing_override_budget_lowered() {
    let base = lockfile_with(
        serde_json::json!({"CX": {"overrides": [
            {"module": "foo::*", "max_function_lines": 300,
             "reason": "x", "expires": "2026-06-01", "owner": "@x"}
        ]}}),
        vec![],
    );
    let cur = lockfile_with(
        serde_json::json!({"CX": {"overrides": [
            {"module": "foo::*", "max_function_lines": 200,
             "reason": "x", "expires": "2026-06-01", "owner": "@x"}
        ]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false, false);
    assert!(
        diags.iter().all(|d| d.rule_id != "PG001"),
        "tightening an override shouldn't fire; got {diags:#?}",
    );
}

// ---- PG002 (visibility) + PG006 (metadata) -----------------------

#[test]
fn pg002_fires_on_every_new_override_even_with_full_metadata() {
    // Reviewer concern: even with metadata, adding an override is
    // policy widening that should be visible. PG002 fires on the
    // addition; PG006 stays quiet because metadata is complete.
    let base = lockfile_with(serde_json::json!({"CX": {"overrides": []}}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({"CX": {"overrides": [{
            "module": "foo::*",
            "max_function_lines": 200,
            "reason": "AST dispatcher",
            "expires": "2026-06-01",
            "owner": "architecture"
        }]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);
    let pg002: Vec<_> = diags.iter().filter(|d| d.rule_id == "PG002").collect();
    assert_eq!(
        pg002.len(),
        1,
        "PG002 must fire on every new override regardless of metadata; got {diags:#?}"
    );
    assert_eq!(
        pg002[0].severity,
        Severity::Fatal,
        "without calibration, PG002 should be Fatal under strict",
    );
    // Metadata is complete, so PG006 stays quiet.
    assert!(
        diags.iter().all(|d| d.rule_id != "PG006"),
        "complete metadata; PG006 must stay quiet. got {diags:#?}",
    );
}

#[test]
fn pg002_advisory_under_calibration_when_metadata_complete() {
    let base = lockfile_with(serde_json::json!({"CX": {"overrides": []}}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({"CX": {"overrides": [{
            "module": "foo::*", "max_function_lines": 200,
            "reason": "x", "expires": "2026-06-01", "owner": "@core"
        }]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, true, false);
    let pg002 = diags.iter().find(|d| d.rule_id == "PG002").unwrap();
    assert_eq!(
        pg002.severity,
        Severity::Advisory,
        "calibration downgrades PG002 (the visibility rule) to Advisory",
    );
}

#[test]
fn pg006_fires_when_new_override_lacks_metadata() {
    let base = lockfile_with(serde_json::json!({"CX": {"overrides": []}}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({"CX": {"overrides": [{
            "module": "foo::*", "max_function_lines": 200
        }]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);
    // Both PG002 (addition) and PG006 (metadata gap).
    assert!(diags.iter().any(|d| d.rule_id == "PG002"));
    let pg006: Vec<_> = diags.iter().filter(|d| d.rule_id == "PG006").collect();
    assert_eq!(pg006.len(), 1);
    let m = pg006[0].message.as_str();
    assert!(m.contains("foo::*"));
    assert!(m.contains("reason"));
    assert!(m.contains("expires"));
    assert!(m.contains("owner"));
}

#[test]
fn pg006_stays_fatal_under_calibration() {
    // Reviewer-spec'd: calibration accepts the *act* of adding an
    // override, but does NOT waive metadata. PG006 must stay Fatal
    // under strict even with calibration set.
    let base = lockfile_with(serde_json::json!({"CX": {"overrides": []}}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({"CX": {"overrides": [{
            "module": "foo::*", "max_function_lines": 200
        }]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, true, false);
    let pg006 = diags.iter().find(|d| d.rule_id == "PG006").unwrap();
    assert_eq!(
        pg006.severity,
        Severity::Fatal,
        "PG006 must stay Fatal under strict even when calibration is set; metadata is non-negotiable"
    );
    // PG002 itself becomes Advisory under calibration:
    let pg002 = diags.iter().find(|d| d.rule_id == "PG002").unwrap();
    assert_eq!(pg002.severity, Severity::Advisory);
}

#[test]
fn pg006_lists_only_actually_missing_fields() {
    let base = lockfile_with(serde_json::json!({"CX": {"overrides": []}}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({"CX": {"overrides": [{
            "module": "foo::*",
            "max_function_lines": 200,
            "reason": "yes"
        }]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false, false);
    let pg006 = diags.iter().find(|d| d.rule_id == "PG006").unwrap();
    let m = pg006.message.as_str();
    assert!(m.contains("expires"));
    assert!(m.contains("owner"));
    assert!(
        !m.contains(", reason") && !m.starts_with("(reason"),
        "reason was supplied; should not appear in missing list: {m}"
    );
}

#[test]
fn pg002_pg006_quiet_for_pre_existing_override() {
    let base = lockfile_with(
        serde_json::json!({"CX": {"overrides": [
            {"module": "foo::*", "max_function_lines": 200}
        ]}}),
        vec![],
    );
    let cur = base.clone();
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false, false);
    assert!(
        diags
            .iter()
            .all(|d| !matches!(d.rule_id.as_str(), "PG002" | "PG006")),
        "pre-existing override; PG002/PG006 must not fire. got {diags:#?}"
    );
}

#[test]
fn pg002_covers_module_overrides() {
    let base = lockfile_with(serde_json::json!({"CX": {"module_overrides": []}}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({"CX": {"module_overrides": [{
            "module": "foo::*", "max_module_lines": 1500
        }]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false, false);
    let pg002 = diags.iter().find(|d| d.rule_id == "PG002").unwrap();
    assert!(pg002.message.contains("module_overrides"));
}

#[test]
fn pg002_covers_mo_overrides() {
    let base = lockfile_with(serde_json::json!({"MO": {"overrides": []}}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({"MO": {"overrides": [{
            "module": "foo::*", "max_public_types": 50
        }]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false, false);
    let pg002 = diags.iter().find(|d| d.rule_id == "PG002").unwrap();
    assert!(pg002.message.contains("MO.overrides"));
}

#[test]
fn pg002_covers_mo_lib_rs_kinds() {
    // Adding a `lib_rs_kinds` entry with kind `canonical-data` silences
    // MO005 for a whole crate root — the same policy-widening shape as a
    // new override. PG002 must surface the addition regardless of
    // metadata; PG006 stays quiet because metadata is complete.
    let base = lockfile_with(serde_json::json!({"MO": {"lib_rs_kinds": []}}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({"MO": {"lib_rs_kinds": [{
            "module": "some_pkg",
            "kind": "canonical-data",
            "reason": "intentional flat data contract",
            "expires": "2027-01-01",
            "owner": "@locus-core"
        }]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);
    let pg002 = diags
        .iter()
        .find(|d| d.rule_id == "PG002")
        .expect("PG002 must fire on a new lib_rs_kinds entry");
    assert!(
        pg002.message.contains("MO.lib_rs_kinds"),
        "PG002 message should name the lib_rs_kinds surface; got: {}",
        pg002.message
    );
    assert!(pg002.message.contains("some_pkg"));
    assert_eq!(
        pg002.severity,
        Severity::Fatal,
        "without calibration, PG002 should be Fatal under strict",
    );
    assert!(
        diags.iter().all(|d| d.rule_id != "PG006"),
        "complete metadata; PG006 must stay quiet. got {diags:#?}",
    );
}

#[test]
fn pg002_lib_rs_kinds_advisory_under_calibration() {
    let base = lockfile_with(serde_json::json!({"MO": {"lib_rs_kinds": []}}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({"MO": {"lib_rs_kinds": [{
            "module": "some_pkg",
            "kind": "composition-root",
            "reason": "wiring crate",
            "expires": "2027-01-01",
            "owner": "@locus-core"
        }]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, true, false);
    let pg002 = diags
        .iter()
        .find(|d| d.rule_id == "PG002" && d.message.contains("MO.lib_rs_kinds"))
        .expect("PG002 must fire for the lib_rs_kinds surface");
    assert_eq!(
        pg002.severity,
        Severity::Advisory,
        "calibration downgrades PG002 (visibility) to Advisory, just like \
         it does for `paradigms.MO.overrides`",
    );
}

#[test]
fn pg006_fires_on_lib_rs_kinds_entry_without_metadata() {
    // An agent cannot bypass PG006 by stashing the silencer in
    // `lib_rs_kinds` instead of `overrides`. Missing reason/expires/owner
    // must fire PG006 with the lib_rs_kinds field name.
    let base = lockfile_with(serde_json::json!({"MO": {"lib_rs_kinds": []}}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({"MO": {"lib_rs_kinds": [{
            "module": "some_pkg",
            "kind": "canonical-data"
        }]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);
    assert!(
        diags
            .iter()
            .any(|d| d.rule_id == "PG002" && d.message.contains("MO.lib_rs_kinds")),
        "PG002 must fire on the addition; got {diags:#?}",
    );
    let pg006 = diags
        .iter()
        .find(|d| d.rule_id == "PG006" && d.message.contains("MO.lib_rs_kinds"))
        .expect("PG006 must fire for the lib_rs_kinds surface");
    let m = pg006.message.as_str();
    assert!(m.contains("some_pkg"));
    assert!(m.contains("reason"));
    assert!(m.contains("expires"));
    assert!(m.contains("owner"));
    assert_eq!(
        pg006.severity,
        Severity::Fatal,
        "PG006 stays Fatal under strict — metadata is non-negotiable",
    );
}

#[test]
fn pg006_lib_rs_kinds_stays_fatal_under_calibration() {
    let base = lockfile_with(serde_json::json!({"MO": {"lib_rs_kinds": []}}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({"MO": {"lib_rs_kinds": [{
            "module": "some_pkg",
            "kind": "canonical-data"
        }]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, true, false);
    let pg006 = diags
        .iter()
        .find(|d| d.rule_id == "PG006" && d.message.contains("MO.lib_rs_kinds"))
        .expect("PG006 fires even with calibration");
    assert_eq!(pg006.severity, Severity::Fatal);
    let pg002 = diags
        .iter()
        .find(|d| d.rule_id == "PG002" && d.message.contains("MO.lib_rs_kinds"))
        .expect("PG002 also fires");
    assert_eq!(
        pg002.severity,
        Severity::Advisory,
        "calibration downgrades PG002 to Advisory but does not waive PG006",
    );
}

#[test]
fn pg002_pg006_quiet_for_pre_existing_lib_rs_kinds_entry() {
    // Identical baseline and current — the entry was always there, so
    // PG002/PG006 must stay quiet even though metadata is incomplete.
    let lf = lockfile_with(
        serde_json::json!({"MO": {"lib_rs_kinds": [{
            "module": "locus_air",
            "kind": "canonical-data"
        }]}}),
        vec![],
    );
    let diags = check_policy_mutation(&lf, Some(&lf), CheckMode::AgentStrict, false, false);
    assert!(
        diags
            .iter()
            .all(|d| !matches!(d.rule_id.as_str(), "PG002" | "PG006")
                || !d.message.contains("MO.lib_rs_kinds")),
        "pre-existing lib_rs_kinds entry; no PG002/PG006 against it. got {diags:#?}",
    );
}

// ---- PG003 new exempt_paths --------------------------------------

#[test]
fn pg003_fires_on_new_cx_exempt_path() {
    let base = lockfile_with(
        serde_json::json!({"CX": {"exempt_paths": ["*::tests::*"]}}),
        vec![],
    );
    let cur = lockfile_with(
        serde_json::json!({"CX": {"exempt_paths": ["*::tests::*", "locus_air::*"]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false, false);
    let pg003 = diags.iter().find(|d| d.rule_id == "PG003").unwrap();
    assert!(pg003.message.contains("locus_air::*"));
    assert!(pg003.message.contains("CX.exempt_paths"));
}

#[test]
fn pg003_quiet_when_exempt_paths_unchanged() {
    let lf = lockfile_with(
        serde_json::json!({"CX": {"exempt_paths": ["*::tests::*"]}}),
        vec![],
    );
    let diags = check_policy_mutation(&lf, Some(&lf), CheckMode::Human, false, false);
    assert!(diags.iter().all(|d| d.rule_id != "PG003"));
}

#[test]
fn pg003_covers_dc_exempt_paths() {
    let base = lockfile_with(serde_json::json!({"DC": {"exempt_paths": []}}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({"DC": {"exempt_paths": ["*::generated::*"]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false, false);
    let pg003 = diags.iter().find(|d| d.rule_id == "PG003").unwrap();
    assert!(pg003.message.contains("DC.exempt_paths"));
    assert!(pg003.message.contains("*::generated::*"));
}

/// Regression: when baseline has explicit CX configuration but current
/// has no CX section at all, PG003/PG007 must NOT fire. Defaults injected
/// by `paradigm_section_explicit` are not user policy; PG only audits
/// sections the user explicitly set. Closes the follow-up from #91.
#[test]
fn pg003_pg007_quiet_when_current_has_no_cx_section() {
    let base = lockfile_with(
        serde_json::json!({"CX": {"exempt_paths": ["*::tests::*", "locus_air::*"]}}),
        vec![],
    );
    // Current has zero paradigm sections — user hasn't configured anything.
    let cur = lockfile_with(serde_json::json!({}), vec![]);
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false, false);
    assert!(
        diags.iter().all(|d| d.rule_id != "PG003"),
        "PG003 must stay silent when current has no CX section; got {diags:#?}"
    );
    assert!(
        diags.iter().all(|d| d.rule_id != "PG007"),
        "PG007 must stay silent when current has no CX section; got {diags:#?}"
    );
}

// ---- PG004 acknowledged_empty additions --------------------------

#[test]
fn pg004_fires_on_new_acknowledged_empty_entry() {
    let base = lockfile_with(serde_json::json!({}), vec![]);
    let cur = lockfile_with(serde_json::json!({}), ack_legacy(&["BO", "PA"]));
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false, false);
    let pg004: Vec<_> = diags.iter().filter(|d| d.rule_id == "PG004").collect();
    assert_eq!(
        pg004.len(),
        2,
        "two new prefixes; expected two PG004; got {pg004:#?}"
    );
    let prefixes: Vec<_> = pg004.iter().filter_map(|d| d.concept.clone()).collect();
    assert!(prefixes.contains(&"BO".into()));
    assert!(prefixes.contains(&"PA".into()));
}

#[test]
fn pg004_quiet_for_pre_existing_acknowledged_entry() {
    let base = lockfile_with(serde_json::json!({}), ack_legacy(&["BO"]));
    let cur = lockfile_with(serde_json::json!({}), ack_legacy(&["BO"]));
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false, false);
    assert!(diags.iter().all(|d| d.rule_id != "PG004"));
}

// ---- combined / dogfood-shape scenarios --------------------------

#[test]
fn pg_catches_the_failure_mode_from_closed_pr_42() {
    // The closed-PR-#42 cheat: agent raises default budget + adds
    // overrides without debt metadata. After the review fixes, this
    // produces PG001 + PG002 + PG006 — all Fatal under strict.
    let base = lockfile_with(serde_json::json!({"CX": {}}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({"CX": {
            "default_max_function_lines": 120,
            "overrides": [
                {"module": "locus_rust::visitor", "max_function_lines": 300}
            ]
        }}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);

    let by_rule: std::collections::HashSet<&str> =
        diags.iter().map(|d| d.rule_id.as_str()).collect();
    assert!(
        by_rule.contains("PG001"),
        "PG001 must fire on the budget bump; got {diags:#?}"
    );
    assert!(
        by_rule.contains("PG002"),
        "PG002 must fire on the new override addition"
    );
    assert!(
        by_rule.contains("PG006"),
        "PG006 must fire on the missing debt metadata"
    );
    assert!(
        diags
            .iter()
            .filter(|d| matches!(d.rule_id.as_str(), "PG001" | "PG002" | "PG006"))
            .all(|d| d.severity == Severity::Fatal),
        "all three must be Fatal under strict without calibration"
    );
}

#[test]
fn pg_catches_the_tagged_override_bypass_attempt() {
    // The slightly-smarter cheat the reviewer flagged: agent adds an
    // override WITH metadata (so PG006 stays quiet) and bumps an
    // existing override's budget. PG001 (override budget) + PG002
    // (new override) must still fire Fatal.
    let base = lockfile_with(
        serde_json::json!({"CX": {"overrides": [
            {"module": "locus_rust::visitor", "max_function_lines": 300,
             "reason": "AST", "expires": "2026-06-01", "owner": "@core"}
        ]}}),
        vec![],
    );
    let cur = lockfile_with(
        serde_json::json!({"CX": {"overrides": [
            // Same module as baseline, budget bumped — PG001.
            {"module": "locus_rust::visitor", "max_function_lines": 10000,
             "reason": "AST", "expires": "2026-06-01", "owner": "@core"},
            // New module with full metadata — PG002 (no PG006).
            {"module": "locus_core::paradigms::failure_lineage::rules", "max_function_lines": 1500,
             "reason": "FL has the most rules of any paradigm", "expires": "2026-09-01", "owner": "@core"}
        ]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);

    let pg001: Vec<_> = diags.iter().filter(|d| d.rule_id == "PG001").collect();
    assert_eq!(
        pg001.len(),
        1,
        "PG001 must fire on the existing override budget bump; got {diags:#?}"
    );
    let pg002: Vec<_> = diags.iter().filter(|d| d.rule_id == "PG002").collect();
    assert_eq!(
        pg002.len(),
        1,
        "PG002 must fire on the new override addition; got {diags:#?}"
    );
    assert!(
        diags.iter().all(|d| d.rule_id != "PG006"),
        "metadata is complete; PG006 must stay quiet"
    );
    // Both PG001 and PG002 are Fatal under strict without calibration.
    for d in pg001.iter().chain(pg002.iter()) {
        assert_eq!(d.severity, Severity::Fatal);
    }
}

/// Every PG diagnostic must anchor its span at `.locus/lock.json`. The CLI
/// pipeline appends PG diagnostics AFTER the `--changed` file filter
/// (see `crates/locus-cli/src/main.rs::check`), so PG bypasses
/// `--changed` entirely. This test pins the contract: if a future PG
/// rule emits a span outside `.locus/lock.json`, the bypass invariant
/// breaks silently because the CLI's pipeline-order guard relies on
/// PG being appended unfiltered.
#[test]
fn all_pg_diagnostics_anchor_to_lockfile_span() {
    let base = lockfile_with(serde_json::json!({}), vec![]);
    // Trigger every PG variant simultaneously.
    let cur = lockfile_with(
        serde_json::json!({
            "CX": {
                "default_max_function_lines": 999,           // PG001 (default)
                "overrides": [{
                    "module": "x::*", "max_function_lines": 999,
                    // PG002 (visibility) + PG006 (no metadata)
                }],
                "exempt_paths": ["nope::*"],                  // PG003
            }
        }),
        ack_legacy(&["BO"]), // PG004 + PG009 (no metadata on new entry)
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);
    let pg_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.rule_id.starts_with("PG"))
        .collect();
    assert!(
        pg_diags
            .iter()
            .all(|d| d.span.file == crate::lockfile::LOCKFILE_RELATIVE_PATH),
        "every PG diagnostic must anchor to `.locus/lock.json`; found: {:?}",
        pg_diags
            .iter()
            .map(|d| (&d.rule_id, &d.span.file))
            .collect::<Vec<_>>()
    );
    // PG000 also: covered by the `baseline = None` path.
    let baseline_missing = check_policy_mutation(&cur, None, CheckMode::AgentStrict, false, false);
    assert!(
        baseline_missing
            .iter()
            .all(|d| d.span.file == crate::lockfile::LOCKFILE_RELATIVE_PATH),
        "PG000 must also anchor to `.locus/lock.json`",
    );
}

#[test]
fn pg_advisory_under_calibration_keeps_pg006_strict() {
    // Calibration mode: PG001/PG002/PG003/PG004 → Advisory.
    // PG006 → still Fatal under strict (metadata is mandatory).
    let base = lockfile_with(serde_json::json!({"CX": {}}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({"CX": {
            "default_max_function_lines": 120,
            "overrides": [
                {"module": "locus_rust::visitor", "max_function_lines": 300}
                // metadata omitted → PG006 fires.
            ]
        }}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, true, false);

    let pg001 = diags.iter().find(|d| d.rule_id == "PG001").unwrap();
    let pg002 = diags.iter().find(|d| d.rule_id == "PG002").unwrap();
    let pg006 = diags.iter().find(|d| d.rule_id == "PG006").unwrap();
    assert_eq!(pg001.severity, Severity::Advisory);
    assert_eq!(pg002.severity, Severity::Advisory);
    assert_eq!(
        pg006.severity,
        Severity::Fatal,
        "PG006 must stay Fatal under strict even with calibration"
    );
}

// ---- PG007 new exempt_paths struct entry missing metadata --------

#[test]
fn pg007_fires_on_new_cx_exempt_path_struct_lacking_metadata() {
    // New struct-form entry without reason/expires/owner → PG003 + PG007.
    let base = lockfile_with(
        serde_json::json!({"CX": {"exempt_paths": ["*::tests::*"]}}),
        vec![],
    );
    let cur = lockfile_with(
        serde_json::json!({"CX": {"exempt_paths": [
            "*::tests::*",
            {"pattern": "locus_air::*"}  // struct form, no metadata
        ]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);

    // PG003 fires on any new addition.
    assert!(
        diags.iter().any(|d| d.rule_id == "PG003"),
        "PG003 must fire on new exempt path; got {diags:#?}"
    );
    // PG007 fires because the struct entry lacks metadata.
    let pg007: Vec<_> = diags.iter().filter(|d| d.rule_id == "PG007").collect();
    assert_eq!(pg007.len(), 1, "expected exactly one PG007; got {diags:#?}");
    let m = pg007[0].message.as_str();
    assert!(
        m.contains("locus_air::*"),
        "message should name the pattern: {m}"
    );
    assert!(m.contains("reason"), "should list missing 'reason': {m}");
    assert!(m.contains("expires"), "should list missing 'expires': {m}");
    assert!(m.contains("owner"), "should list missing 'owner': {m}");
    assert_eq!(
        pg007[0].severity,
        Severity::Fatal,
        "PG007 must be Fatal under strict"
    );
}

// ---- PG008 new OT.converter_paths entry --------------------------

fn ot_lockfile(converter_paths: &[&str]) -> Lockfile {
    lockfile_with(
        serde_json::json!({"OT": {"concepts": {}, "converter_paths": converter_paths}}),
        vec![],
    )
}

#[test]
fn pg008_fires_on_new_converter_path() {
    // baseline: one existing path; current adds a second.
    let base = ot_lockfile(&["locus_rust::*"]);
    let cur = ot_lockfile(&["locus_rust::*", "*::rules_tests::*"]);
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false, false);
    let pg008: Vec<_> = diags.iter().filter(|d| d.rule_id == "PG008").collect();
    assert_eq!(pg008.len(), 1, "expected one PG008; got {diags:#?}");
    assert!(
        pg008[0].message.contains("*::rules_tests::*"),
        "message should name the new path; got {}",
        pg008[0].message
    );
    assert!(
        pg008[0].message.contains("OT.converter_paths"),
        "message should name the lockfile field; got {}",
        pg008[0].message
    );
}

#[test]
fn pg008_silent_on_unchanged_converter_paths() {
    let lf = ot_lockfile(&["locus_rust::*", "*::tests::*"]);
    let diags = check_policy_mutation(&lf, Some(&lf), CheckMode::AgentStrict, false, false);
    assert!(
        diags.iter().all(|d| d.rule_id != "PG008"),
        "unchanged converter_paths must not fire PG008; got {diags:#?}"
    );
}

#[test]
fn pg008_silent_on_removal() {
    // Removing a path is not widening — PG008 stays quiet.
    let base = ot_lockfile(&["a", "b"]);
    let cur = ot_lockfile(&["a"]);
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);
    assert!(
        diags.iter().all(|d| d.rule_id != "PG008"),
        "removal is not widening; PG008 must stay quiet; got {diags:#?}"
    );
}

#[test]
fn pg008_downgrades_to_advisory_under_calibration() {
    let base = ot_lockfile(&["a"]);
    let cur = ot_lockfile(&["a", "b"]);
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, true, false);
    let pg008 = diags.iter().find(|d| d.rule_id == "PG008").unwrap();
    assert_eq!(
        pg008.severity,
        Severity::Advisory,
        "calibration mode should downgrade PG008 to Advisory; got {:?}",
        pg008.severity
    );
}

#[test]
fn pg008_fires_fatal_under_strict() {
    let base = ot_lockfile(&["a"]);
    let cur = ot_lockfile(&["a", "b"]);
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);
    let pg008 = diags.iter().find(|d| d.rule_id == "PG008").unwrap();
    assert_eq!(
        pg008.severity,
        Severity::Fatal,
        "PG008 must be Fatal under --agent-strict without calibration"
    );
}

#[test]
fn pg008_fires_warning_under_default_mode() {
    let base = ot_lockfile(&["a"]);
    let cur = ot_lockfile(&["a", "b"]);
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false, false);
    let pg008 = diags.iter().find(|d| d.rule_id == "PG008").unwrap();
    assert_eq!(
        pg008.severity,
        Severity::Warning,
        "PG008 must be Warning under default (Human) mode"
    );
}

#[test]
fn pg007_quiet_when_new_struct_entry_has_complete_metadata() {
    // New struct-form entry WITH complete metadata → PG003 only, no PG007.
    let base = lockfile_with(
        serde_json::json!({"CX": {"exempt_paths": ["*::tests::*"]}}),
        vec![],
    );
    let cur = lockfile_with(
        serde_json::json!({"CX": {"exempt_paths": [
            "*::tests::*",
            {
                "pattern": "locus_air::*",
                "reason": "canonical data crate — all public types are the AIR contract",
                "expires": "2027-05-09",
                "owner": "@core"
            }
        ]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);

    assert!(
        diags.iter().any(|d| d.rule_id == "PG003"),
        "PG003 still fires on the addition; got {diags:#?}"
    );
    assert!(
        diags.iter().all(|d| d.rule_id != "PG007"),
        "complete metadata — PG007 must stay quiet; got {diags:#?}"
    );
}

// ---- PG007 grandfather-by-pattern: new legacy strings -------------

#[test]
fn pg007_fires_on_new_legacy_string_entry_not_in_baseline() {
    // Reviewer-identified loophole fix: a new bare-string entry that was NOT
    // in the baseline must fire PG007. Using legacy form is no longer a
    // PG007 bypass — only patterns that were already in the baseline are
    // grandfathered.
    let base = lockfile_with(
        serde_json::json!({"CX": {"exempt_paths": ["*::tests::*", "locus_air::*"]}}),
        vec![],
    );
    let cur = lockfile_with(
        serde_json::json!({"CX": {"exempt_paths": ["*::tests::*", "locus_air::*", "new_bypass::*"]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);

    // PG003 fires on the new addition.
    assert!(
        diags.iter().any(|d| d.rule_id == "PG003"),
        "PG003 must fire on the new legacy string entry; got {diags:#?}"
    );
    // PG007 also fires — the pattern is new and lacks metadata.
    let pg007: Vec<_> = diags.iter().filter(|d| d.rule_id == "PG007").collect();
    assert_eq!(
        pg007.len(),
        1,
        "PG007 must fire on a new legacy string (no metadata); got {diags:#?}"
    );
    assert!(
        pg007[0].message.contains("new_bypass::*"),
        "PG007 message should name the pattern: {}",
        pg007[0].message
    );
    assert_eq!(
        pg007[0].severity,
        Severity::Fatal,
        "PG007 must be Fatal under strict"
    );
}

#[test]
fn pg008_not_suppressed_by_lockfile_exception() {
    // PG runs after apply_exceptions. A lockfile exception targeting
    // PG008 must NOT silence it — PG is meta-policy. This test verifies
    // that check_policy_mutation itself always fires PG008 (the CLI
    // pipeline order enforces non-suppressibility end-to-end; here we
    // confirm the rule fires regardless of any exception field).
    let base = ot_lockfile(&["a"]);
    let mut cur = ot_lockfile(&["a", "b"]);
    // Add a lockfile exception targeting PG008 — simulates what a
    // `// locus: allow PG008` hint or a lockfile exceptions entry would
    // produce after apply_exceptions has run (i.e., it has already been
    // applied and PG runs afterwards, so the exception has no effect).
    cur.exceptions.push(crate::lockfile::Exception {
        rule: "PG008".to_string(),
        target: "*".to_string(),
        reason: "test suppression attempt".to_string(),
        expires: "9999-12-31".to_string(),
    });
    // check_policy_mutation is called after apply_exceptions in the CLI
    // pipeline. The exceptions in cur.exceptions are irrelevant to PG.
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);
    assert!(
        diags.iter().any(|d| d.rule_id == "PG008"),
        "PG008 must fire even when a lockfile exception targets it; got {diags:#?}"
    );
}

#[test]
fn pg008_fires_on_multiple_new_paths() {
    // Two new paths in one PR — both should be flagged.
    let base = ot_lockfile(&["locus_rust::*"]);
    let cur = ot_lockfile(&["locus_rust::*", "*::rules_tests", "*::rules_tests::*"]);
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false, false);
    let pg008: Vec<_> = diags.iter().filter(|d| d.rule_id == "PG008").collect();
    assert_eq!(
        pg008.len(),
        2,
        "two new paths should produce two PG008 diagnostics; got {pg008:#?}"
    );
    let msgs: Vec<&str> = pg008.iter().map(|d| d.message.as_str()).collect();
    // Messages use backtick quoting: `*::rules_tests` and `*::rules_tests::*`.
    // The first entry has no trailing `::*` so we match it by its unique suffix.
    assert!(
        msgs.iter().any(|m| m.contains("`*::rules_tests`")),
        "first new path should appear in a PG008 message; got {msgs:#?}"
    );
    assert!(
        msgs.iter().any(|m| m.contains("`*::rules_tests::*`")),
        "second new path should appear in a PG008 message; got {msgs:#?}"
    );
}

#[test]
fn pg008_span_anchors_to_lockfile() {
    let base = ot_lockfile(&["a"]);
    let cur = ot_lockfile(&["a", "b"]);
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false, false);
    let pg008 = diags.iter().find(|d| d.rule_id == "PG008").unwrap();
    assert_eq!(
        pg008.span.file,
        crate::lockfile::LOCKFILE_RELATIVE_PATH,
        "PG008 must anchor its span to `.locus/lock.json`"
    );
}

#[test]
fn pg007_silent_for_existing_legacy_string_entry_in_baseline() {
    // Grandfathered: if the pattern was already in the baseline (as a legacy
    // string), PG007 must stay quiet. The entry appears in `locus debt` as
    // "legacy-no-metadata" via a separate code path — not PG007's concern.
    let base = lockfile_with(
        serde_json::json!({"CX": {"exempt_paths": ["*::tests::*", "locus_air::*"]}}),
        vec![],
    );
    let cur = base.clone();
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);

    assert!(
        diags.iter().all(|d| d.rule_id != "PG007"),
        "baseline legacy strings are grandfathered — PG007 must stay quiet; got {diags:#?}"
    );
    // PG003 also stays quiet (no new entries).
    assert!(
        diags.iter().all(|d| d.rule_id != "PG003"),
        "no new entries — PG003 must also stay quiet; got {diags:#?}"
    );
}

#[test]
fn pg007_silent_when_legacy_string_is_removed() {
    // Removing an entry is never subject to PG007. Policy Guard cares about
    // widening (new suppressions), not tightening (removed suppressions).
    let base = lockfile_with(
        serde_json::json!({"CX": {"exempt_paths": ["*::tests::*", "locus_air::*"]}}),
        vec![],
    );
    let cur = lockfile_with(
        serde_json::json!({"CX": {"exempt_paths": ["*::tests::*"]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);

    assert!(
        diags.iter().all(|d| d.rule_id != "PG007"),
        "removal is not widening — PG007 must stay quiet; got {diags:#?}"
    );
    assert!(
        diags.iter().all(|d| d.rule_id != "PG003"),
        "removal is not widening — PG003 must stay quiet too; got {diags:#?}"
    );
}

#[test]
fn pg007_silent_when_upgrading_baseline_legacy_string_to_full_struct_with_metadata() {
    // Upgrading a baseline legacy string to a Full struct with complete
    // metadata is exactly the desired migration path. PG007 must stay quiet
    // because the pattern was already in the baseline (grandfathered).
    // PG003 also stays quiet — the pattern is not new; only its form changed.
    let base = lockfile_with(
        serde_json::json!({"CX": {"exempt_paths": ["*::tests::*"]}}),
        vec![],
    );
    let cur = lockfile_with(
        serde_json::json!({"CX": {"exempt_paths": [
            {
                "pattern": "*::tests::*",
                "reason": "test modules legitimately expose helpers",
                "expires": "2027-01-01",
                "owner": "@core"
            }
        ]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);

    assert!(
        diags.iter().all(|d| d.rule_id != "PG007"),
        "upgrading a baseline pattern to full struct with metadata — PG007 must stay quiet; got {diags:#?}"
    );
    // PG003 stays quiet too: the pattern was in the baseline, only the form
    // changed. Upgrading from legacy-string to full-struct is not a new
    // exempt_path addition.
    assert!(
        diags.iter().all(|d| d.rule_id != "PG003"),
        "pattern already in baseline — PG003 must stay quiet on form-only upgrade; got {diags:#?}"
    );
}

#[test]
fn pg007_silent_for_preexisting_full_struct_without_metadata_grandfathered() {
    // If the baseline already contained a Full struct entry lacking metadata,
    // that entry is grandfathered (per-pattern keying). PG007 must not re-fire
    // on it in current. Only NEW patterns without metadata trigger PG007.
    let base = lockfile_with(
        serde_json::json!({"CX": {"exempt_paths": [
            {"pattern": "old::*"}  // Full struct, no metadata — pre-existing
        ]}}),
        vec![],
    );
    let cur = base.clone();
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);

    assert!(
        diags.iter().all(|d| d.rule_id != "PG007"),
        "pre-existing Full struct without metadata is grandfathered — PG007 must stay quiet; got {diags:#?}"
    );
}

#[test]
fn pg007_fires_on_new_full_struct_lacking_metadata_not_in_baseline() {
    // Negative case for the above: adding a NEW Full struct entry (pattern not
    // in baseline) that is missing metadata must fire PG007.
    let base = lockfile_with(
        serde_json::json!({"CX": {"exempt_paths": [
            {"pattern": "old::*"}  // pre-existing Full struct
        ]}}),
        vec![],
    );
    let cur = lockfile_with(
        serde_json::json!({"CX": {"exempt_paths": [
            {"pattern": "old::*"},           // grandfathered, no PG007
            {"pattern": "brand_new::*"}      // new, no metadata — PG007 fires
        ]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);

    let pg007: Vec<_> = diags.iter().filter(|d| d.rule_id == "PG007").collect();
    assert_eq!(
        pg007.len(),
        1,
        "only the new pattern should trigger PG007; got {diags:#?}"
    );
    assert!(
        pg007[0].message.contains("brand_new::*"),
        "PG007 must name the new pattern: {}",
        pg007[0].message
    );
}

#[test]
fn pg007_silent_when_upgrading_baseline_legacy_to_full_struct_without_metadata() {
    // An agent upgrading a baseline legacy string to a Full struct WITHOUT
    // metadata is still grandfathered — the pattern itself was in the baseline.
    // PG007 uses the pattern as the identity key, not the entry form.
    // (The entry would surface as "legacy-no-metadata" in `locus debt` for
    // human attention, but Policy Guard does not re-fire PG007 on it.)
    let base = lockfile_with(
        serde_json::json!({"CX": {"exempt_paths": ["*::tests::*"]}}),
        vec![],
    );
    let cur = lockfile_with(
        serde_json::json!({"CX": {"exempt_paths": [
            {"pattern": "*::tests::*"}  // Full struct but no metadata — same pattern as baseline
        ]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);

    assert!(
        diags.iter().all(|d| d.rule_id != "PG007"),
        "*::tests::* was in baseline — grandfathered regardless of form; PG007 must stay quiet; got {diags:#?}"
    );
}

#[test]
fn pg007_stays_fatal_under_calibration() {
    // Calibration accepts the addition (PG003 → Advisory) but does NOT
    // waive the metadata requirement (PG007 → still Fatal under strict).
    let base = lockfile_with(serde_json::json!({"CX": {"exempt_paths": []}}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({"CX": {"exempt_paths": [
            {"pattern": "foo::*"}  // struct, no metadata
        ]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, true, false);

    let pg003 = diags.iter().find(|d| d.rule_id == "PG003").unwrap();
    assert_eq!(
        pg003.severity,
        Severity::Advisory,
        "PG003 should be Advisory under calibration"
    );
    let pg007 = diags.iter().find(|d| d.rule_id == "PG007").unwrap();
    assert_eq!(
        pg007.severity,
        Severity::Fatal,
        "PG007 must stay Fatal under strict even with calibration"
    );
}

#[test]
fn pg007_lists_only_actually_missing_fields() {
    // Only 'expires' and 'owner' missing — message must mention them
    // but not 'reason'.
    let base = lockfile_with(serde_json::json!({"CX": {"exempt_paths": []}}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({"CX": {"exempt_paths": [
            {"pattern": "bar::*", "reason": "some reason"}  // missing expires + owner
        ]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false, false);
    let pg007 = diags.iter().find(|d| d.rule_id == "PG007").unwrap();
    let m = pg007.message.as_str();
    assert!(
        m.contains("expires"),
        "should mention missing 'expires': {m}"
    );
    assert!(m.contains("owner"), "should mention missing 'owner': {m}");
    assert!(
        !m.contains(", reason") && !m.starts_with("(reason"),
        "reason was supplied; should not appear in missing list: {m}"
    );
}

#[test]
fn pg007_quiet_for_preexisting_struct_entry_without_metadata() {
    // If the struct entry (even without metadata) was already in baseline,
    // PG007 must not re-fire — it only covers *new* additions.
    let base = lockfile_with(
        serde_json::json!({"CX": {"exempt_paths": [
            {"pattern": "foo::*"}  // struct, no metadata — pre-existing
        ]}}),
        vec![],
    );
    let cur = base.clone();
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);
    assert!(
        diags.iter().all(|d| d.rule_id != "PG007"),
        "pre-existing entry — PG007 must not fire; got {diags:#?}"
    );
}

// ---- PG009 acknowledged_empty new entry lacking metadata ----------

#[test]
fn pg009_fires_on_new_legacy_string_prefix() {
    // baseline has "BO"; current adds "CR" as a bare string → PG004 + PG009.
    let base = lockfile_with(serde_json::json!({}), ack_legacy(&["BO"]));
    let cur = lockfile_with(serde_json::json!({}), ack_legacy(&["BO", "CR"]));
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);

    // PG004 fires on the new addition.
    assert!(
        diags.iter().any(|d| d.rule_id == "PG004"),
        "PG004 must fire on new prefix; got {diags:#?}"
    );
    // PG009 also fires — new legacy string has no metadata.
    let pg009: Vec<_> = diags.iter().filter(|d| d.rule_id == "PG009").collect();
    assert_eq!(pg009.len(), 1, "expected one PG009; got {diags:#?}");
    let m = pg009[0].message.as_str();
    assert!(
        m.contains("CR"),
        "PG009 message should name the prefix: {m}"
    );
    assert!(m.contains("reason"), "should list missing 'reason': {m}");
    assert!(m.contains("expires"), "should list missing 'expires': {m}");
    assert!(m.contains("owner"), "should list missing 'owner': {m}");
    assert_eq!(
        pg009[0].severity,
        Severity::Fatal,
        "PG009 must be Fatal under strict"
    );
}

#[test]
fn pg009_silent_on_grandfathered_legacy_string() {
    // baseline has "BO"; current still has just "BO" → silent.
    let base = lockfile_with(serde_json::json!({}), ack_legacy(&["BO"]));
    let cur = lockfile_with(serde_json::json!({}), ack_legacy(&["BO"]));
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);
    assert!(
        diags.iter().all(|d| d.rule_id != "PG009"),
        "grandfathered entry — PG009 must stay quiet; got {diags:#?}"
    );
}

#[test]
fn pg009_fires_on_new_full_entry_lacking_metadata() {
    // baseline is empty; current adds a Full struct with no metadata → PG004 + PG009.
    let base = lockfile_with(serde_json::json!({}), vec![]);
    let cur = lockfile_with(serde_json::json!({}), vec![ack_full_no_metadata("DA")]);
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);

    assert!(
        diags.iter().any(|d| d.rule_id == "PG004"),
        "PG004 must fire on new entry; got {diags:#?}"
    );
    let pg009: Vec<_> = diags.iter().filter(|d| d.rule_id == "PG009").collect();
    assert_eq!(pg009.len(), 1, "expected one PG009; got {diags:#?}");
    let m = pg009[0].message.as_str();
    assert!(
        m.contains("DA"),
        "PG009 message should name the prefix: {m}"
    );
    assert_eq!(
        pg009[0].severity,
        Severity::Fatal,
        "PG009 must be Fatal under strict"
    );
}

#[test]
fn pg009_silent_on_new_full_entry_with_complete_metadata() {
    // baseline is empty; current adds a Full struct with all metadata → PG004 only, no PG009.
    let base = lockfile_with(serde_json::json!({}), vec![]);
    let cur = lockfile_with(serde_json::json!({}), vec![ack_full("DA")]);
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);

    assert!(
        diags.iter().any(|d| d.rule_id == "PG004"),
        "PG004 still fires on the addition; got {diags:#?}"
    );
    assert!(
        diags.iter().all(|d| d.rule_id != "PG009"),
        "complete metadata — PG009 must stay quiet; got {diags:#?}"
    );
}

#[test]
fn pg009_silent_on_form_only_upgrade_same_prefix() {
    // baseline has legacy "BO"; current upgrades it to Full struct without
    // metadata. The prefix is grandfathered — PG009 stays silent.
    let base = lockfile_with(serde_json::json!({}), ack_legacy(&["BO"]));
    let cur = lockfile_with(serde_json::json!({}), vec![ack_full_no_metadata("BO")]);
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);
    assert!(
        diags.iter().all(|d| d.rule_id != "PG009"),
        "prefix already in baseline — grandfathered; PG009 must stay quiet; got {diags:#?}"
    );
    // PG004 also stays quiet — the prefix was already in baseline.
    assert!(
        diags.iter().all(|d| d.rule_id != "PG004"),
        "prefix already in baseline — PG004 must stay quiet too; got {diags:#?}"
    );
}

#[test]
fn pg009_fires_under_strict_silent_with_calibration_downgrade() {
    // Calibration downgrades PG004 to Advisory but does NOT waive PG009.
    let base = lockfile_with(serde_json::json!({}), vec![]);
    let cur = lockfile_with(serde_json::json!({}), ack_legacy(&["ER"]));
    // Without calibration: PG004 = Fatal, PG009 = Fatal.
    let diags_strict =
        check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);
    let pg004_strict = diags_strict.iter().find(|d| d.rule_id == "PG004").unwrap();
    let pg009_strict = diags_strict.iter().find(|d| d.rule_id == "PG009").unwrap();
    assert_eq!(pg004_strict.severity, Severity::Fatal);
    assert_eq!(pg009_strict.severity, Severity::Fatal);

    // With calibration: PG004 → Advisory; PG009 stays Fatal.
    let diags_cal = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, true, false);
    let pg004_cal = diags_cal.iter().find(|d| d.rule_id == "PG004").unwrap();
    let pg009_cal = diags_cal.iter().find(|d| d.rule_id == "PG009").unwrap();
    assert_eq!(
        pg004_cal.severity,
        Severity::Advisory,
        "PG004 should be Advisory under calibration"
    );
    assert_eq!(
        pg009_cal.severity,
        Severity::Fatal,
        "PG009 must stay Fatal under strict even with calibration"
    );
}

#[test]
fn pg009_not_suppressed_by_lockfile_exception_or_allow_hint() {
    // PG runs after apply_exceptions. A lockfile exception targeting
    // PG009 must NOT silence it — PG is meta-policy.
    let base = lockfile_with(serde_json::json!({}), vec![]);
    let mut cur = lockfile_with(serde_json::json!({}), ack_legacy(&["FL"]));
    cur.exceptions.push(crate::lockfile::Exception {
        rule: "PG009".to_string(),
        target: "*".to_string(),
        reason: "test suppression attempt".to_string(),
        expires: "9999-12-31".to_string(),
    });
    // check_policy_mutation is called after apply_exceptions in the CLI
    // pipeline, so the exceptions entry has no effect on PG.
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);
    assert!(
        diags.iter().any(|d| d.rule_id == "PG009"),
        "PG009 must fire even when a lockfile exception targets it; got {diags:#?}"
    );
}

#[test]
fn pg009_lists_only_actually_missing_fields() {
    // Only 'expires' and 'owner' missing — message should mention them
    // but not 'reason'.
    let base = lockfile_with(serde_json::json!({}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({}),
        vec![AcknowledgedEmptyEntry::Full(AcknowledgedEmpty {
            prefix: "TA".to_string(),
            reason: Some("some reason".to_string()),
            expires: None,
            owner: None,
            ..Default::default()
        })],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false, false);
    let pg009 = diags.iter().find(|d| d.rule_id == "PG009").unwrap();
    let m = pg009.message.as_str();
    assert!(
        m.contains("expires"),
        "should mention missing 'expires': {m}"
    );
    assert!(m.contains("owner"), "should mention missing 'owner': {m}");
    assert!(
        !m.contains(", reason") && !m.starts_with("(reason"),
        "reason was supplied; should not appear in missing list: {m}"
    );
}

#[test]
fn pg009_silent_when_full_entry_with_complete_metadata_already_in_baseline() {
    // Full entry with complete metadata in baseline — still grandfathered.
    let base = lockfile_with(serde_json::json!({}), vec![ack_full("RM")]);
    let cur = lockfile_with(serde_json::json!({}), vec![ack_full("RM")]);
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false, false);
    assert!(
        diags.iter().all(|d| d.rule_id != "PG009"),
        "pre-existing full entry — PG009 must stay quiet; got {diags:#?}"
    );
}
