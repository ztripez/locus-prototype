//! Policy Guard — detect policy widening that erases diagnostics.
//!
//! Spec: `docs/superpowers/specs/2026-05-09-policy-guard-paradigm.md`
//! (issue #44).
//!
//! An agent or human can clear `locus check --agent-strict` output by
//! changing the **measurement surface** instead of fixing code: raise a
//! budget, add an override, mark a paradigm `acknowledged_empty`, or
//! drop an entry into `exempt_paths`. None of those improve the
//! underlying architecture; they suppress the rule. Policy Guard
//! detects exactly those mutations by comparing the current lockfile
//! against a **baseline** (typically `git show origin/main:locus.lock`).
//!
//! The principle (#44):
//!
//! > Severity controls whether debt blocks normal work.
//! > Policy mutation controls whether agents are allowed to erase debt.
//!
//! Cross-paradigm advisory — fits the same shape as `LOCUS001` (expired
//! exception) and `LOCUS002` (vacant paradigm). Wired into the CLI's
//! check pipeline; consumes a current `Lockfile` plus an optional
//! baseline `Lockfile` (the CLI is responsible for reading the baseline
//! via git).
//!
//! Rules implemented today:
//! - [`PG001_BUDGET_RAISED`] — a numeric budget field increased.
//! - [`PG002_OVERRIDE_ADDED`] — a new override exists in current that
//!   wasn't in baseline AND the override is missing structured debt
//!   metadata (`reason` + `expires` + `owner`).
//! - [`PG003_EXEMPT_PATH_ADDED`] — a new entry in any `exempt_paths`
//!   list vs baseline.
//! - [`PG004_ACKNOWLEDGED_EMPTY_ADDED`] — a new entry in
//!   top-level `acknowledged_empty`.
//!
//! Deferred:
//! - **PG005** (severity lowered) — needs a severity-override schema in
//!   the lockfile, which doesn't exist yet.
//! - **Expired-debt** (override past `expires` date) — schema is in
//!   place; rule body is a follow-up.
//! - **Dead-debt** (override whose target no longer violates) —
//!   non-trivial, needs override-application tracking through the
//!   check pipeline.
//!
//! Calibration mode: when the CLI passes `calibration = true`, PG
//! diagnostics fire as `Severity::Advisory` regardless of mode. The
//! CLI is expected to also print a structured calibration report;
//! this module just produces the diagnostics.

// locus: ot canonical

use serde::Deserialize;

use crate::diagnostics::{CheckMode, Diagnostic, Severity};
use crate::lockfile::Lockfile;
use crate::paradigms::complexity_budget::lockfile_schema::{
    CxModuleOverride, CxOverride, CxSection,
};
use crate::paradigms::module_ownership::lockfile_schema::{MoOverride, MoSection};
use locus_air::AirSpan;

/// PG001 — a numeric budget field in the lockfile increased vs the
/// baseline. Includes `default_max_function_lines`,
/// `default_max_module_lines`, `max_public_items`, `max_fan_out`,
/// `default_max_public_types`, `entropy_threshold`.
pub const PG001_BUDGET_RAISED: &str = "PG001";

/// PG002 — a new override exists in the current lockfile vs baseline
/// AND the override lacks structured debt metadata.
pub const PG002_OVERRIDE_ADDED: &str = "PG002";

/// PG003 — a new entry exists in some `exempt_paths` list vs baseline.
pub const PG003_EXEMPT_PATH_ADDED: &str = "PG003";

/// PG004 — a new entry exists in top-level `acknowledged_empty`
/// vs baseline.
pub const PG004_ACKNOWLEDGED_EMPTY_ADDED: &str = "PG004";

/// Run all PG checks against `current` vs `baseline`. Returns no
/// diagnostics when `baseline` is `None` (e.g., first commit, no git,
/// baseline ref doesn't carry a `locus.lock`).
///
/// Severity:
/// - When `calibration = true`, PG diagnostics fire as Advisory.
/// - Otherwise, Warning by default; Fatal under `--agent-strict` via
///   [`CheckMode::elevate`].
pub fn check_policy_mutation(
    current: &Lockfile,
    baseline: Option<&Lockfile>,
    mode: CheckMode,
    calibration: bool,
) -> Vec<Diagnostic> {
    let Some(baseline) = baseline else {
        return Vec::new();
    };
    let mut out = Vec::new();
    out.extend(check_budget_changes(current, baseline, mode, calibration));
    out.extend(check_new_overrides(current, baseline, mode, calibration));
    out.extend(check_new_exempt_paths(current, baseline, mode, calibration));
    out.extend(check_new_acknowledged_empty(
        current,
        baseline,
        mode,
        calibration,
    ));
    out
}

fn pg_severity(mode: CheckMode, calibration: bool) -> Severity {
    if calibration {
        Severity::Advisory
    } else {
        mode.elevate(Severity::Warning)
    }
}

fn lockfile_span() -> AirSpan {
    AirSpan::new("locus.lock", 1, 1)
}

// ---- PG001 budget raised ------------------------------------------

fn check_budget_changes(
    current: &Lockfile,
    baseline: &Lockfile,
    mode: CheckMode,
    calibration: bool,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let cur_cx: CxSection = current.paradigm_section("CX").unwrap_or_default();
    let base_cx: CxSection = baseline.paradigm_section("CX").unwrap_or_default();
    diff_optional_budget(
        "paradigms.CX.default_max_function_lines",
        base_cx.default_max_function_lines,
        cur_cx.default_max_function_lines,
        crate::paradigms::complexity_budget::lockfile_schema::DEFAULT_MAX_FUNCTION_LINES,
        mode,
        calibration,
        &mut out,
    );
    diff_optional_budget(
        "paradigms.CX.default_max_module_lines",
        base_cx.default_max_module_lines,
        cur_cx.default_max_module_lines,
        crate::paradigms::complexity_budget::lockfile_schema::DEFAULT_MAX_MODULE_LINES,
        mode,
        calibration,
        &mut out,
    );
    diff_required_budget(
        "paradigms.CX.max_public_items",
        base_cx.max_public_items,
        cur_cx.max_public_items,
        mode,
        calibration,
        &mut out,
    );
    diff_required_budget(
        "paradigms.CX.max_fan_out",
        base_cx.max_fan_out,
        cur_cx.max_fan_out,
        mode,
        calibration,
        &mut out,
    );

    let cur_mo: MoSection = current.paradigm_section("MO").unwrap_or_default();
    let base_mo: MoSection = baseline.paradigm_section("MO").unwrap_or_default();
    diff_optional_budget(
        "paradigms.MO.default_max_public_types",
        base_mo.default_max_public_types,
        cur_mo.default_max_public_types,
        crate::paradigms::module_ownership::lockfile_schema::DEFAULT_MAX_PUBLIC_TYPES,
        mode,
        calibration,
        &mut out,
    );
    diff_optional_budget(
        "paradigms.MO.entropy_threshold",
        base_mo.entropy_threshold,
        cur_mo.entropy_threshold,
        crate::paradigms::module_ownership::lockfile_schema::DEFAULT_ENTROPY_THRESHOLD,
        mode,
        calibration,
        &mut out,
    );
    out
}

/// Compare an `Option<u32>` budget across baseline → current. The
/// effective value is `Some(n)` when set, else the built-in
/// `fallback`. Fires PG001 when current's effective value is greater
/// than baseline's effective value.
fn diff_optional_budget(
    field: &str,
    base: Option<u32>,
    cur: Option<u32>,
    fallback: u32,
    mode: CheckMode,
    calibration: bool,
    out: &mut Vec<Diagnostic>,
) {
    let base_eff = base.unwrap_or(fallback);
    let cur_eff = cur.unwrap_or(fallback);
    if cur_eff > base_eff {
        out.push(budget_raised_diagnostic(
            field,
            base_eff,
            cur_eff,
            base.is_none(),
            cur.is_none(),
            mode,
            calibration,
        ));
    }
}

fn diff_required_budget(
    field: &str,
    base: u32,
    cur: u32,
    mode: CheckMode,
    calibration: bool,
    out: &mut Vec<Diagnostic>,
) {
    if cur > base {
        out.push(budget_raised_diagnostic(
            field,
            base,
            cur,
            false,
            false,
            mode,
            calibration,
        ));
    }
}

fn budget_raised_diagnostic(
    field: &str,
    base: u32,
    cur: u32,
    base_was_fallback: bool,
    cur_is_fallback: bool,
    mode: CheckMode,
    calibration: bool,
) -> Diagnostic {
    let delta = cur as i64 - base as i64;
    let mut why = vec![
        format!("{field} raised from {base} to {cur} (Δ {delta:+})"),
        "policy widening can hide diagnostics that real refactor would expose; \
         prefer fixing the underlying code"
            .into(),
    ];
    if base_was_fallback {
        why.push(format!("baseline used the built-in fallback ({base})"));
    }
    if cur_is_fallback {
        why.push(format!("current uses the built-in fallback ({cur})"));
    }
    Diagnostic {
        rule_id: PG001_BUDGET_RAISED.to_string(),
        severity: pg_severity(mode, calibration),
        span: lockfile_span(),
        concept: None,
        message: format!("policy budget `{field}` raised from {base} to {cur}"),
        why,
        suggested_fix: Some(
            "if this is a deliberate calibration, re-run `locus check` with \
             `--allow-policy-calibration` and ensure the change has a paired \
             debt-metadata entry; otherwise revert and address the underlying \
             architectural issue"
                .into(),
        ),
    }
}

// ---- PG002 new override added without debt metadata ---------------

fn check_new_overrides(
    current: &Lockfile,
    baseline: &Lockfile,
    mode: CheckMode,
    calibration: bool,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let cur_cx: CxSection = current.paradigm_section("CX").unwrap_or_default();
    let base_cx: CxSection = baseline.paradigm_section("CX").unwrap_or_default();
    let base_cx_modules: std::collections::HashSet<&str> = base_cx
        .overrides
        .iter()
        .map(|o| o.module.as_str())
        .collect();
    for o in &cur_cx.overrides {
        if base_cx_modules.contains(o.module.as_str()) {
            continue;
        }
        if let Some(d) = check_override_debt(
            "paradigms.CX.overrides",
            &o.module,
            cx_override_debt(o),
            mode,
            calibration,
        ) {
            out.push(d);
        }
    }
    let base_cx_module_overrides: std::collections::HashSet<&str> = base_cx
        .module_overrides
        .iter()
        .map(|o| o.module.as_str())
        .collect();
    for o in &cur_cx.module_overrides {
        if base_cx_module_overrides.contains(o.module.as_str()) {
            continue;
        }
        if let Some(d) = check_override_debt(
            "paradigms.CX.module_overrides",
            &o.module,
            cx_module_override_debt(o),
            mode,
            calibration,
        ) {
            out.push(d);
        }
    }

    let cur_mo: MoSection = current.paradigm_section("MO").unwrap_or_default();
    let base_mo: MoSection = baseline.paradigm_section("MO").unwrap_or_default();
    let base_mo_modules: std::collections::HashSet<&str> = base_mo
        .overrides
        .iter()
        .map(|o| o.module.as_str())
        .collect();
    for o in &cur_mo.overrides {
        if base_mo_modules.contains(o.module.as_str()) {
            continue;
        }
        if let Some(d) = check_override_debt(
            "paradigms.MO.overrides",
            &o.module,
            mo_override_debt(o),
            mode,
            calibration,
        ) {
            out.push(d);
        }
    }
    out
}

#[derive(Debug, Default)]
struct DebtMetadata<'a> {
    reason: Option<&'a str>,
    expires: Option<&'a str>,
    owner: Option<&'a str>,
}

fn cx_override_debt(o: &CxOverride) -> DebtMetadata<'_> {
    DebtMetadata {
        reason: o.reason.as_deref(),
        expires: o.expires.as_deref(),
        owner: o.owner.as_deref(),
    }
}

fn cx_module_override_debt(o: &CxModuleOverride) -> DebtMetadata<'_> {
    DebtMetadata {
        reason: o.reason.as_deref(),
        expires: o.expires.as_deref(),
        owner: o.owner.as_deref(),
    }
}

fn mo_override_debt(o: &MoOverride) -> DebtMetadata<'_> {
    DebtMetadata {
        reason: o.reason.as_deref(),
        expires: o.expires.as_deref(),
        owner: o.owner.as_deref(),
    }
}

fn check_override_debt(
    list_field: &str,
    module: &str,
    debt: DebtMetadata<'_>,
    mode: CheckMode,
    calibration: bool,
) -> Option<Diagnostic> {
    let mut missing: Vec<&'static str> = Vec::new();
    if debt.reason.is_none_or(str::is_empty) {
        missing.push("reason");
    }
    if debt.expires.is_none_or(str::is_empty) {
        missing.push("expires");
    }
    if debt.owner.is_none_or(str::is_empty) {
        missing.push("owner");
    }
    if missing.is_empty() {
        return None;
    }
    let missing_label = missing.join(", ");
    Some(Diagnostic {
        rule_id: PG002_OVERRIDE_ADDED.to_string(),
        severity: pg_severity(mode, calibration),
        span: lockfile_span(),
        concept: None,
        message: format!(
            "new override on `{module}` in `{list_field}` lacks debt metadata \
             ({missing_label})"
        ),
        why: vec![
            "an override silences a rule for a module; without `reason` / \
             `expires` / `owner` it becomes invisible debt"
                .into(),
            format!("missing field(s): {missing_label}"),
        ],
        suggested_fix: Some(
            "populate `reason` (why the override exists), `expires` \
             (`YYYY-MM-DD` review date), and `owner` (team/individual). \
             Add via the lockfile editor or hand-edit `locus.lock`."
                .into(),
        ),
    })
}

// ---- PG003 new exempt_paths entry --------------------------------

fn check_new_exempt_paths(
    current: &Lockfile,
    baseline: &Lockfile,
    mode: CheckMode,
    calibration: bool,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let cur_cx: CxSection = current.paradigm_section("CX").unwrap_or_default();
    let base_cx: CxSection = baseline.paradigm_section("CX").unwrap_or_default();
    let base_set: std::collections::HashSet<&str> =
        base_cx.exempt_paths.iter().map(String::as_str).collect();
    for entry in &cur_cx.exempt_paths {
        if base_set.contains(entry.as_str()) {
            continue;
        }
        out.push(Diagnostic {
            rule_id: PG003_EXEMPT_PATH_ADDED.to_string(),
            severity: pg_severity(mode, calibration),
            span: lockfile_span(),
            concept: None,
            message: format!("new exempt path `{entry}` in `paradigms.CX.exempt_paths`"),
            why: vec![
                "exemption silences the rule entirely for matching modules; \
                 prefer narrowing the rule via overrides with debt metadata, \
                 or fixing the underlying code"
                    .into(),
            ],
            suggested_fix: Some(
                "if this is a deliberate calibration, re-run `locus check` \
                 with `--allow-policy-calibration`. The exempt-paths schema \
                 will gain debt metadata in a follow-up; until then exemptions \
                 are visible additions."
                    .into(),
            ),
        });
    }
    // Other paradigms with exempt_paths (DC, etc.) follow the same pattern;
    // for the MVP we cover CX. Future paradigms register their exempt-path
    // diff via a similar block.
    let cur_dc: serde_json::Value = current
        .paradigm_section("DC")
        .unwrap_or(serde_json::Value::Null);
    let base_dc: serde_json::Value = baseline
        .paradigm_section("DC")
        .unwrap_or(serde_json::Value::Null);
    let cur_dc_paths = json_string_array(&cur_dc, "exempt_paths");
    let base_dc_paths = json_string_array(&base_dc, "exempt_paths");
    let base_dc_set: std::collections::HashSet<&str> =
        base_dc_paths.iter().map(String::as_str).collect();
    for entry in &cur_dc_paths {
        if base_dc_set.contains(entry.as_str()) {
            continue;
        }
        out.push(Diagnostic {
            rule_id: PG003_EXEMPT_PATH_ADDED.to_string(),
            severity: pg_severity(mode, calibration),
            span: lockfile_span(),
            concept: None,
            message: format!("new exempt path `{entry}` in `paradigms.DC.exempt_paths`"),
            why: vec![
                "exemption silences the rule entirely for matching modules; \
                 prefer narrowing the rule via overrides with debt metadata, \
                 or fixing the underlying code"
                    .into(),
            ],
            suggested_fix: Some("if deliberate, re-run with `--allow-policy-calibration`.".into()),
        });
    }
    out
}

fn json_string_array(value: &serde_json::Value, field: &str) -> Vec<String> {
    value
        .get(field)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

// ---- PG004 new acknowledged_empty entry --------------------------

fn check_new_acknowledged_empty(
    current: &Lockfile,
    baseline: &Lockfile,
    mode: CheckMode,
    calibration: bool,
) -> Vec<Diagnostic> {
    let base: std::collections::HashSet<&str> = baseline
        .acknowledged_empty
        .iter()
        .map(String::as_str)
        .collect();
    current
        .acknowledged_empty
        .iter()
        .filter(|p| !base.contains(p.as_str()))
        .map(|prefix| Diagnostic {
            rule_id: PG004_ACKNOWLEDGED_EMPTY_ADDED.to_string(),
            severity: pg_severity(mode, calibration),
            span: lockfile_span(),
            concept: Some(prefix.clone()),
            message: format!("paradigm `{prefix}` newly added to `acknowledged_empty`"),
            why: vec![
                "acknowledging a paradigm as empty silences its `LOCUS002` \
                 vacancy nudge; the paradigm's rules cannot fire until the \
                 user populates declarations or removes the acknowledgement"
                    .into(),
            ],
            suggested_fix: Some(
                "if deliberate, re-run with `--allow-policy-calibration`. \
                 Otherwise populate the paradigm's section in `locus.lock`."
                    .into(),
            ),
        })
        .collect()
}

// Allow Lockfile field-only deserialization when the JSON is a bare value;
// we don't need it directly since `paradigm_section::<T>` already does this.
// Kept as a use marker for clarity if T's bound surfaces in error messages.
#[allow(dead_code)]
fn _ensure_deserialize<T: for<'de> Deserialize<'de> + Default>() {}

#[cfg(test)]
#[path = "policy_guard_tests.rs"]
mod tests;
