//! Tests for [`super`] policy_guard module.
//!
//! Sibling-attached via `#[path = "policy_guard_tests.rs"] mod tests;`
//! at the bottom of `policy_guard.rs`.

use super::*;
use crate::lockfile::Lockfile;

fn lockfile_with(paradigms: serde_json::Value, ack: Vec<String>) -> Lockfile {
    let mut lf = Lockfile::empty();
    if let Some(obj) = paradigms.as_object() {
        for (k, v) in obj {
            lf.paradigms.insert(k.clone(), v.clone());
        }
    }
    lf.acknowledged_empty = ack;
    lf
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
        vec!["BO".into()],
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

// ---- PG004 acknowledged_empty additions --------------------------

#[test]
fn pg004_fires_on_new_acknowledged_empty_entry() {
    let base = lockfile_with(serde_json::json!({}), vec![]);
    let cur = lockfile_with(serde_json::json!({}), vec!["BO".into(), "PA".into()]);
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
    let base = lockfile_with(serde_json::json!({}), vec!["BO".into()]);
    let cur = lockfile_with(serde_json::json!({}), vec!["BO".into()]);
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
