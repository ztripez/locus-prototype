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

// ---- silent on no baseline ---------------------------------------

#[test]
fn no_baseline_yields_no_diagnostics() {
    let cur = lockfile_with(
        serde_json::json!({"CX": {"default_max_function_lines": 200}}),
        vec![],
    );
    assert!(check_policy_mutation(&cur, None, CheckMode::AgentStrict, false).is_empty());
}

#[test]
fn identical_lockfiles_yield_no_diagnostics() {
    let lf = lockfile_with(
        serde_json::json!({"CX": {"default_max_function_lines": 200}}),
        vec!["BO".into()],
    );
    let diags = check_policy_mutation(&lf, Some(&lf), CheckMode::AgentStrict, false);
    assert!(
        diags.is_empty(),
        "no drift; PG should stay quiet. got {diags:#?}"
    );
}

// ---- PG001 budget raised -----------------------------------------

#[test]
fn pg001_fires_when_default_max_function_lines_raised() {
    let base = lockfile_with(serde_json::json!({"CX": {}}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({"CX": {"default_max_function_lines": 120}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false);
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
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false);
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
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false);
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
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, true);
    let pg001 = diags.iter().find(|d| d.rule_id == "PG001").unwrap();
    assert_eq!(
        pg001.severity,
        Severity::Advisory,
        "calibration mode should downgrade PG to Advisory regardless of mode"
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
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false);
    let pg001 = diags.iter().find(|d| d.rule_id == "PG001").unwrap();
    assert!(pg001.message.contains("default_max_module_lines"));
}

// ---- PG002 new override without debt metadata --------------------

#[test]
fn pg002_fires_on_new_override_without_debt_metadata() {
    let base = lockfile_with(serde_json::json!({"CX": {"overrides": []}}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({"CX": {"overrides": [{"module": "foo::*", "max_function_lines": 200}]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false);
    let pg002: Vec<_> = diags.iter().filter(|d| d.rule_id == "PG002").collect();
    assert_eq!(pg002.len(), 1);
    let m = pg002[0].message.as_str();
    assert!(m.contains("foo::*"), "msg names module: {m}");
    assert!(m.contains("reason"));
    assert!(m.contains("expires"));
    assert!(m.contains("owner"));
}

#[test]
fn pg002_quiet_on_new_override_with_full_debt_metadata() {
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
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false);
    assert!(
        diags.iter().all(|d| d.rule_id != "PG002"),
        "override has full debt metadata; PG002 must stay quiet. got {diags:#?}"
    );
}

#[test]
fn pg002_fires_when_only_some_metadata_present() {
    let base = lockfile_with(serde_json::json!({"CX": {"overrides": []}}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({"CX": {"overrides": [{
            "module": "foo::*",
            "max_function_lines": 200,
            "reason": "yes"
            // missing expires + owner
        }]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false);
    let pg002 = diags.iter().find(|d| d.rule_id == "PG002").unwrap();
    let m = pg002.message.as_str();
    assert!(
        m.contains("expires"),
        "msg names missing field expires: {m}"
    );
    assert!(m.contains("owner"), "msg names missing field owner: {m}");
    assert!(
        !m.contains("reason"),
        "reason was supplied; should not appear: {m}"
    );
}

#[test]
fn pg002_quiet_for_pre_existing_override() {
    let base = lockfile_with(
        serde_json::json!({"CX": {"overrides": [{"module": "foo::*", "max_function_lines": 200}]}}),
        vec![],
    );
    let cur = base.clone();
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false);
    assert!(
        diags.iter().all(|d| d.rule_id != "PG002"),
        "override existed in baseline; PG002 must not fire. got {diags:#?}"
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
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false);
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
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false);
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
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false);
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
    let diags = check_policy_mutation(&lf, Some(&lf), CheckMode::Human, false);
    assert!(diags.iter().all(|d| d.rule_id != "PG003"));
}

#[test]
fn pg003_covers_dc_exempt_paths() {
    let base = lockfile_with(serde_json::json!({"DC": {"exempt_paths": []}}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({"DC": {"exempt_paths": ["*::generated::*"]}}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false);
    let pg003 = diags.iter().find(|d| d.rule_id == "PG003").unwrap();
    assert!(pg003.message.contains("DC.exempt_paths"));
    assert!(pg003.message.contains("*::generated::*"));
}

// ---- PG004 acknowledged_empty additions --------------------------

#[test]
fn pg004_fires_on_new_acknowledged_empty_entry() {
    let base = lockfile_with(serde_json::json!({}), vec![]);
    let cur = lockfile_with(serde_json::json!({}), vec!["BO".into(), "PA".into()]);
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false);
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
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::Human, false);
    assert!(diags.iter().all(|d| d.rule_id != "PG004"));
}

// ---- combined / dogfood-shape scenario ---------------------------

#[test]
fn pg_catches_the_failure_mode_from_closed_pr_42() {
    // Reproduce the PR #42 cheat: agent raises default budget + adds
    // overrides without debt metadata to silence CX001 hits.
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
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, false);

    assert!(
        diags.iter().any(|d| d.rule_id == "PG001"),
        "PG001 must fire on the budget bump"
    );
    assert!(
        diags.iter().any(|d| d.rule_id == "PG002"),
        "PG002 must fire on the override-without-metadata"
    );
    assert!(
        diags
            .iter()
            .filter(|d| matches!(d.rule_id.as_str(), "PG001" | "PG002"))
            .all(|d| d.severity == Severity::Fatal),
        "PG diagnostics under --agent-strict without calibration must be Fatal"
    );
}

#[test]
fn pg_advisory_under_calibration_for_the_same_scenario() {
    // Same cheat, but now the agent passes --allow-policy-calibration.
    // PG diagnostics still fire (visibility), but as Advisory.
    let base = lockfile_with(serde_json::json!({"CX": {}}), vec![]);
    let cur = lockfile_with(
        serde_json::json!({"CX": {
            "default_max_function_lines": 120,
            "overrides": [
                {"module": "locus_rust::visitor", "max_function_lines": 300,
                 "reason": "AST dispatcher", "expires": "2026-06-01",
                 "owner": "architecture"}
            ]
        }}),
        vec![],
    );
    let diags = check_policy_mutation(&cur, Some(&base), CheckMode::AgentStrict, true);
    let pg: Vec<_> = diags
        .iter()
        .filter(|d| d.rule_id.starts_with("PG"))
        .collect();
    assert!(
        !pg.is_empty(),
        "calibration shouldn't suppress; just downgrade"
    );
    assert!(
        pg.iter().all(|d| d.severity == Severity::Advisory),
        "calibration mode renders PG diagnostics as Advisory"
    );
    // PG002 should NOT fire because debt metadata is now present.
    assert!(
        pg.iter().all(|d| d.rule_id != "PG002"),
        "fully-tagged override should not trigger PG002"
    );
}
