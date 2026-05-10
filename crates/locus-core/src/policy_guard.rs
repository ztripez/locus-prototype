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
//! **Non-suppressible by lockfile exceptions.** The CLI runs
//! `apply_exceptions` BEFORE adding PG diagnostics. Without that
//! ordering, a `{rule: "*", target: "*"}` exception would erase the
//! audit. PG is meta-policy; it is the one rule family
//! `Lockfile.exceptions[]` must not be able to silence.
//!
//! Rules implemented today:
//! - [`PG000_BASELINE_MISSING`] — no baseline lockfile resolved (e.g.
//!   shallow clone, missing ref, untracked baseline file). Fires
//!   Warning by default; Fatal under `--agent-strict` unless the user
//!   passes `--allow-missing-policy-baseline`.
//! - [`PG001_BUDGET_RAISED`] — a numeric budget field increased,
//!   either at workspace level (`default_max_*`, `max_*`) or on an
//!   existing override entry (`overrides[*].max_*_lines`,
//!   `module_overrides[*].max_module_lines`,
//!   `MO.overrides[*].max_public_types`).
//! - [`PG002_OVERRIDE_ADDED`] — a new override exists in current that
//!   wasn't in baseline. Fires regardless of metadata. Calibration
//!   mode downgrades to Advisory (legitimate, acknowledged
//!   calibration) but the addition stays visible.
//! - [`PG003_EXEMPT_PATH_ADDED`] — a new entry in any `exempt_paths`
//!   list vs baseline.
//! - [`PG004_ACKNOWLEDGED_EMPTY_ADDED`] — a new entry in
//!   top-level `acknowledged_empty`.
//! - [`PG006_OVERRIDE_LACKS_DEBT_METADATA`] — a new override is
//!   missing `reason` / `expires` / `owner` debt metadata. Always
//!   Fatal under `--agent-strict`, even with
//!   `--allow-policy-calibration` — calibration legitimizes the
//!   *act* of adding an override, not the absence of justification.
//! - [`PG007_EXEMPT_PATH_LACKS_DEBT_METADATA`] — a new
//!   `CX.exempt_paths` struct entry is missing `reason` / `expires` /
//!   `owner`. Mirrors PG006 for the exempt-paths surface. Legacy
//!   string entries (pre-schema) do not trigger PG007; they surface
//!   in `locus debt` as legacy-no-metadata rows instead.
//! - [`PG008_CONVERTER_PATH_ADDED`] — a new entry in
//!   `paradigms.OT.converter_paths` vs baseline. Widens the
//!   architectural-authority surface for OT004 (converter authority).
//!   Calibration mode downgrades to Advisory.
//! - [`PG009_ACKNOWLEDGED_EMPTY_LACKS_DEBT_METADATA`] — a new
//!   `acknowledged_empty` entry is missing `reason` / `expires` /
//!   `owner`. Mirrors PG007 for the acknowledged-empty surface.
//!   Grandfather-by-prefix: any prefix in the baseline (legacy-string OR
//!   Full-struct form) is grandfathered. Always Fatal under
//!   `--agent-strict`; calibration does NOT downgrade — metadata is
//!   non-negotiable.
//!
//! Deferred:
//! - **PG005** (severity lowered) — needs a severity-override schema
//!   in the lockfile, which doesn't exist yet.
//! - **Expired-debt** (override past `expires` date) — schema is in
//!   place; rule body is a follow-up.
//! - **Dead-debt** (override whose target no longer violates) —
//!   non-trivial, needs override-application tracking through the
//!   check pipeline.
//!
//! Calibration mode: `--allow-policy-calibration` downgrades
//! PG001/PG002/PG003/PG004/PG008 to `Severity::Advisory`. PG000,
//! PG006, PG007, and PG009 ignore calibration — missing baseline and missing
//! debt metadata aren't legitimately calibratable.

// locus: ot canonical

use crate::diagnostics::{CheckMode, Diagnostic, Severity};
use crate::lockfile::{AcknowledgedEmpty, AcknowledgedEmptyEntry, Lockfile};
use crate::paradigms::complexity_budget::lockfile_schema::{
    CxExemptPath, CxModuleOverride, CxOverride, CxSection,
};
use crate::paradigms::module_ownership::lockfile_schema::{MoOverride, MoSection};
use crate::paradigms::one_truth::lockfile_schema::OtSection;
use locus_air::AirSpan;

/// PG000 — no baseline lockfile available; the policy audit could not
/// run. Fires Warning by default; Fatal under `--agent-strict` unless
/// the caller opts into the missing-baseline state via
/// `allow_missing_baseline = true`.
pub const PG000_BASELINE_MISSING: &str = "PG000";

/// PG001 — a numeric budget field in the lockfile increased vs the
/// baseline. Covers workspace defaults (`default_max_function_lines`,
/// `default_max_module_lines`, `max_public_items`, `max_fan_out`,
/// `default_max_public_types`, `entropy_threshold`) AND existing
/// override budgets (`overrides[*].max_function_lines`,
/// `module_overrides[*].max_module_lines`,
/// `MO.overrides[*].max_public_types`) keyed by `module`.
pub const PG001_BUDGET_RAISED: &str = "PG001";

/// PG002 — a new override exists in current that wasn't in baseline.
/// Fires on the *addition* — independent of debt metadata. Calibration
/// mode downgrades to Advisory; PG006 covers the metadata gap.
pub const PG002_OVERRIDE_ADDED: &str = "PG002";

/// PG003 — a new entry exists in some `exempt_paths` list vs baseline.
pub const PG003_EXEMPT_PATH_ADDED: &str = "PG003";

/// PG004 — a new entry exists in top-level `acknowledged_empty`
/// vs baseline.
pub const PG004_ACKNOWLEDGED_EMPTY_ADDED: &str = "PG004";

/// PG006 — a new override lacks structured debt metadata
/// (`reason` + `expires` + `owner`). Independent of PG002, fires
/// alongside it when fields are missing. Always Fatal under
/// `--agent-strict`; calibration mode does NOT downgrade — calibration
/// legitimizes the act of adding an override, not the absence of
/// justification.
pub const PG006_OVERRIDE_LACKS_DEBT_METADATA: &str = "PG006";

/// PG007 — a new `CX.exempt_paths` entry lacks structured debt metadata
/// (`reason` + `expires` + `owner`). Mirrors `PG006` for the exempt-paths
/// surface: calibration legitimizes the addition (`PG003` → Advisory), but
/// it does NOT waive the metadata requirement. Always Fatal under
/// `--agent-strict`; PG003 itself is what calibration can downgrade.
///
/// **Grandfather-by-pattern:** if the pattern already exists in the baseline
/// lockfile (in any form — `Legacy` string or `Full` struct), PG007 stays
/// silent for it. Only patterns that are genuinely *new* vs the baseline are
/// required to arrive with complete metadata.
///
/// This applies to both entry forms:
/// - New `CxExemptPathEntry::Legacy` (bare string) whose pattern is not in
///   the baseline → PG007 fires. An agent cannot bypass PG007 by using the
///   legacy string form for new additions.
/// - New `CxExemptPathEntry::Full` (struct) missing `reason`/`expires`/`owner`
///   → PG007 fires (existing behavior).
/// - Any entry (either form) whose pattern was already in the baseline →
///   PG007 stays silent; it surfaces in `locus debt` as "legacy-no-metadata"
///   if it's a `Legacy` form.
pub const PG007_EXEMPT_PATH_LACKS_DEBT_METADATA: &str = "PG007";

/// PG008 — a new entry exists in `paradigms.OT.converter_paths` vs
/// baseline. `converter_paths` patterns grant architectural authority
/// for OT004 (cross-boundary converter construction); adding a new
/// pattern widens that surface without fixing code. Calibration mode
/// downgrades to Advisory.
pub const PG008_CONVERTER_PATH_ADDED: &str = "PG008";

/// PG009 — a new `acknowledged_empty` entry lacks structured debt metadata
/// (`reason` + `expires` + `owner`). Mirrors `PG007` for the
/// acknowledged-empty surface: calibration legitimizes the addition
/// (`PG004` → Advisory under calibration), but it does NOT waive the
/// metadata requirement. Always Fatal under `--agent-strict`; PG004 itself
/// is what calibration can downgrade.
///
/// **Grandfather-by-prefix:** if the prefix already exists in the baseline
/// lockfile (in any form — `Legacy` string or `Full` struct), PG009 stays
/// silent for it. Only prefixes that are genuinely *new* vs the baseline are
/// required to arrive with complete metadata.
///
/// This applies to both entry forms:
/// - New `AcknowledgedEmptyEntry::Legacy` (bare string) whose prefix is not
///   in the baseline → PG009 fires. An agent cannot bypass PG009 by using
///   the legacy string form for new additions.
/// - New `AcknowledgedEmptyEntry::Full` (struct) missing `reason`/`expires`/`owner`
///   → PG009 fires.
/// - Any entry (either form) whose prefix was already in the baseline →
///   PG009 stays silent; it surfaces in `locus debt` as "legacy-no-metadata"
///   if it's a `Legacy` form.
pub const PG009_ACKNOWLEDGED_EMPTY_LACKS_DEBT_METADATA: &str = "PG009";

/// Run all PG checks against `current` vs `baseline`.
///
/// When `baseline` is `None`, emits a single `PG000` diagnostic unless
/// `allow_missing_baseline` is set. PG000's severity matches `mode`
/// (Warning by default; Fatal under `--agent-strict`); it is **not**
/// affected by `calibration` — calibration acknowledges intentional
/// widening, but missing-baseline means we couldn't audit at all.
///
/// Severity for PG001/PG002/PG003/PG004:
/// - When `calibration = true`, fire as `Severity::Advisory`.
/// - Otherwise, Warning by default; Fatal under `--agent-strict`.
///
/// Severity for PG000 and PG006: ignores `calibration`; always Warning
/// by default and Fatal under `--agent-strict`.
pub fn check_policy_mutation(
    current: &Lockfile,
    baseline: Option<&Lockfile>,
    mode: CheckMode,
    calibration: bool,
    allow_missing_baseline: bool,
) -> Vec<Diagnostic> {
    let Some(baseline) = baseline else {
        if allow_missing_baseline {
            return Vec::new();
        }
        return vec![baseline_missing_diagnostic(mode)];
    };
    let mut out = Vec::new();
    out.extend(check_default_budget_changes(
        current,
        baseline,
        mode,
        calibration,
    ));
    out.extend(check_existing_override_budget_changes(
        current,
        baseline,
        mode,
        calibration,
    ));
    out.extend(check_new_overrides(current, baseline, mode, calibration));
    out.extend(check_new_exempt_paths(current, baseline, mode, calibration));
    out.extend(check_new_acknowledged_empty(
        current,
        baseline,
        mode,
        calibration,
    ));
    out.extend(check_new_converter_paths(
        current,
        baseline,
        mode,
        calibration,
    ));
    out.extend(check_acknowledged_empty_metadata(current, baseline, mode));
    out
}

fn pg_severity(mode: CheckMode, calibration: bool) -> Severity {
    if calibration {
        Severity::Advisory
    } else {
        mode.elevate(Severity::Warning)
    }
}

/// Severity for rules that ignore calibration (PG000, PG006).
fn pg_strict_severity(mode: CheckMode) -> Severity {
    mode.elevate(Severity::Warning)
}

fn lockfile_span() -> AirSpan {
    AirSpan::new("locus.lock", 1, 1)
}

// ---- PG000 baseline missing --------------------------------------

fn baseline_missing_diagnostic(mode: CheckMode) -> Diagnostic {
    Diagnostic {
        rule_id: PG000_BASELINE_MISSING.to_string(),
        severity: pg_strict_severity(mode),
        span: lockfile_span(),
        concept: None,
        message:
            "Policy Guard could not resolve a baseline lockfile; policy widening cannot be audited"
                .to_string(),
        why: vec![
            "tried to read the baseline `locus.lock` via `git show <baseline>:locus.lock`"
                .to_string(),
            "the baseline ref / file / git itself was unavailable; PG001-PG004/PG006 cannot \
             compare against a baseline"
                .into(),
            "without an audit, an agent could quietly widen policy and pass \
             `--agent-strict`; PG000 makes that visible"
                .into(),
        ],
        suggested_fix: Some(
            "ensure the workspace is a git repo with a reachable baseline ref \
             (default chain: `origin/main` → `origin/master` → `main` → `master` → `HEAD~1`); \
             pass `--baseline <ref>` to set explicitly. If this is the first commit \
             before `locus.lock` existed, pass `--allow-missing-policy-baseline` to \
             explicitly accept the audit gap."
                .into(),
        ),
    }
}

// ---- PG001 default-budget raise ----------------------------------

fn check_default_budget_changes(
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

// ---- PG001 existing-override budget raise ------------------------

/// Walk the intersection of current and baseline override lists keyed
/// by `module`, and flag any override whose budget value increased.
/// Without this, an agent could pre-populate an override at a small
/// budget, ship it, then quietly bump it later — the override "module"
/// already exists so PG002 stays quiet.
fn check_existing_override_budget_changes(
    current: &Lockfile,
    baseline: &Lockfile,
    mode: CheckMode,
    calibration: bool,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let cur_cx: CxSection = current.paradigm_section("CX").unwrap_or_default();
    let base_cx: CxSection = baseline.paradigm_section("CX").unwrap_or_default();

    // CX function-line overrides.
    for cur_o in &cur_cx.overrides {
        if let Some(base_o) = base_cx.overrides.iter().find(|o| o.module == cur_o.module)
            && cur_o.max_function_lines > base_o.max_function_lines
        {
            out.push(override_budget_raised_diagnostic(
                "paradigms.CX.overrides",
                &cur_o.module,
                "max_function_lines",
                base_o.max_function_lines,
                cur_o.max_function_lines,
                mode,
                calibration,
            ));
        }
    }
    // CX module-line overrides.
    for cur_o in &cur_cx.module_overrides {
        if let Some(base_o) = base_cx
            .module_overrides
            .iter()
            .find(|o| o.module == cur_o.module)
            && cur_o.max_module_lines > base_o.max_module_lines
        {
            out.push(override_budget_raised_diagnostic(
                "paradigms.CX.module_overrides",
                &cur_o.module,
                "max_module_lines",
                base_o.max_module_lines,
                cur_o.max_module_lines,
                mode,
                calibration,
            ));
        }
    }

    let cur_mo: MoSection = current.paradigm_section("MO").unwrap_or_default();
    let base_mo: MoSection = baseline.paradigm_section("MO").unwrap_or_default();
    for cur_o in &cur_mo.overrides {
        if let Some(base_o) = base_mo.overrides.iter().find(|o| o.module == cur_o.module)
            && cur_o.max_public_types > base_o.max_public_types
        {
            out.push(override_budget_raised_diagnostic(
                "paradigms.MO.overrides",
                &cur_o.module,
                "max_public_types",
                base_o.max_public_types,
                cur_o.max_public_types,
                mode,
                calibration,
            ));
        }
    }
    out
}

fn override_budget_raised_diagnostic(
    list_field: &str,
    module: &str,
    budget_field: &str,
    base: u32,
    cur: u32,
    mode: CheckMode,
    calibration: bool,
) -> Diagnostic {
    let delta = cur as i64 - base as i64;
    Diagnostic {
        rule_id: PG001_BUDGET_RAISED.to_string(),
        severity: pg_severity(mode, calibration),
        span: lockfile_span(),
        concept: None,
        message: format!(
            "override `{module}` in `{list_field}`: `{budget_field}` raised from {base} to {cur}"
        ),
        why: vec![
            format!("override on `{module}` had `{budget_field}={base}` in baseline"),
            format!("current value is `{budget_field}={cur}` (Δ {delta:+})"),
            "raising an existing override budget hides diagnostics that real refactor \
             would expose; keep the budget steady or tighten it"
                .into(),
        ],
        suggested_fix: Some(
            "if this is a deliberate calibration, re-run `locus check` with \
             `--allow-policy-calibration`; otherwise revert and fix the underlying code"
                .into(),
        ),
    }
}

/// Compare an `Option<u32>` workspace budget across baseline → current.
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
        out.push(default_budget_raised_diagnostic(
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
        out.push(default_budget_raised_diagnostic(
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

fn default_budget_raised_diagnostic(
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

// ---- PG002 new override + PG006 missing metadata -----------------

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
        out.push(override_added_diagnostic(
            "paradigms.CX.overrides",
            &o.module,
            mode,
            calibration,
        ));
        if let Some(d) = override_metadata_diagnostic(
            "paradigms.CX.overrides",
            &o.module,
            cx_override_debt(o),
            mode,
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
        out.push(override_added_diagnostic(
            "paradigms.CX.module_overrides",
            &o.module,
            mode,
            calibration,
        ));
        if let Some(d) = override_metadata_diagnostic(
            "paradigms.CX.module_overrides",
            &o.module,
            cx_module_override_debt(o),
            mode,
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
        out.push(override_added_diagnostic(
            "paradigms.MO.overrides",
            &o.module,
            mode,
            calibration,
        ));
        if let Some(d) = override_metadata_diagnostic(
            "paradigms.MO.overrides",
            &o.module,
            mo_override_debt(o),
            mode,
        ) {
            out.push(d);
        }
    }
    out
}

fn override_added_diagnostic(
    list_field: &str,
    module: &str,
    mode: CheckMode,
    calibration: bool,
) -> Diagnostic {
    Diagnostic {
        rule_id: PG002_OVERRIDE_ADDED.to_string(),
        severity: pg_severity(mode, calibration),
        span: lockfile_span(),
        concept: None,
        message: format!("new override on `{module}` added to `{list_field}`"),
        why: vec![
            format!("`{list_field}` did not contain `{module}` in baseline"),
            "an override silences the rule for a module; even with debt metadata, \
             the addition is policy widening that should be acknowledged"
                .into(),
        ],
        suggested_fix: Some(
            "if this is a deliberate calibration, re-run `locus check` with \
             `--allow-policy-calibration` (PG002 will fire as Advisory). Without \
             calibration, address the underlying code or revert the override."
                .into(),
        ),
    }
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

/// PG006 — fires when a new override lacks `reason` / `expires` /
/// `owner`. Always uses strict severity (calibration does NOT
/// downgrade): calibration legitimizes the act of adding the
/// override (PG002 → Advisory under calibration), but it does NOT
/// excuse missing justification.
fn override_metadata_diagnostic(
    list_field: &str,
    module: &str,
    debt: DebtMetadata<'_>,
    mode: CheckMode,
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
        rule_id: PG006_OVERRIDE_LACKS_DEBT_METADATA.to_string(),
        severity: pg_strict_severity(mode),
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
            "PG006 is unaffected by `--allow-policy-calibration`: calibration \
             accepts the addition itself (PG002), but does not waive the \
             requirement to record why, when to revisit, and who owns it"
                .into(),
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
    // Key the baseline set by the raw pattern string. Both legacy `String`
    // entries and struct entries use `.pattern()` as their identity.
    let base_set: std::collections::HashSet<&str> =
        base_cx.exempt_paths.iter().map(|e| e.pattern()).collect();
    for entry in &cur_cx.exempt_paths {
        let pattern = entry.pattern();
        if base_set.contains(pattern) {
            continue;
        }
        out.push(exempt_path_added_diagnostic(
            "paradigms.CX.exempt_paths",
            pattern,
            mode,
            calibration,
        ));
        // PG007 — new entries must carry debt metadata regardless of form.
        // Grandfather-by-pattern: only patterns NOT present in the baseline
        // are subject to PG007. Patterns already in the baseline (in any
        // form) are silently grandfathered; legacy baseline strings also
        // surface in `locus debt` as "legacy-no-metadata" rows.
        //
        // For new Legacy-string entries (not in baseline), we synthesise a
        // metadata-check against a pattern-only struct — all fields will be
        // None, so PG007 fires listing all three missing fields.
        let pg007_target: crate::paradigms::complexity_budget::lockfile_schema::CxExemptPath =
            match entry {
                crate::paradigms::complexity_budget::lockfile_schema::CxExemptPathEntry::Full(
                    ep,
                ) => ep.clone(),
                crate::paradigms::complexity_budget::lockfile_schema::CxExemptPathEntry::Legacy(
                    s,
                ) => crate::paradigms::complexity_budget::lockfile_schema::CxExemptPath {
                    pattern: s.clone(),
                    ..Default::default()
                },
            };
        if let Some(d) =
            exempt_path_metadata_diagnostic("paradigms.CX.exempt_paths", &pg007_target, mode)
        {
            out.push(d);
        }
    }
    // DC's exempt_paths is the only other paradigm with one today.
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
        out.push(exempt_path_added_diagnostic(
            "paradigms.DC.exempt_paths",
            entry,
            mode,
            calibration,
        ));
    }
    out
}

/// PG007 — fires when a new `CX.exempt_paths` Full-struct entry lacks
/// `reason` / `expires` / `owner`. Always uses strict severity (calibration
/// does NOT downgrade): calibration legitimizes the act of adding the entry
/// (PG003 → Advisory under calibration), but it does NOT excuse missing
/// justification metadata.
fn exempt_path_metadata_diagnostic(
    list_field: &str,
    ep: &CxExemptPath,
    mode: CheckMode,
) -> Option<Diagnostic> {
    let mut missing: Vec<&'static str> = Vec::new();
    if ep.reason.as_deref().is_none_or(str::is_empty) {
        missing.push("reason");
    }
    if ep.expires.as_deref().is_none_or(str::is_empty) {
        missing.push("expires");
    }
    if ep.owner.as_deref().is_none_or(str::is_empty) {
        missing.push("owner");
    }
    if missing.is_empty() {
        return None;
    }
    let missing_label = missing.join(", ");
    Some(Diagnostic {
        rule_id: PG007_EXEMPT_PATH_LACKS_DEBT_METADATA.to_string(),
        severity: pg_strict_severity(mode),
        span: lockfile_span(),
        concept: None,
        message: format!(
            "new exempt path `{}` in `{list_field}` lacks debt metadata \
             ({missing_label})",
            ep.pattern
        ),
        why: vec![
            "an exempt-path silences a rule for matching modules; without \
             `reason` / `expires` / `owner` it becomes invisible debt"
                .into(),
            format!("missing field(s): {missing_label}"),
            "PG007 is unaffected by `--allow-policy-calibration`: calibration \
             accepts the addition itself (PG003), but does not waive the \
             requirement to record why, when to revisit, and who owns it"
                .into(),
        ],
        suggested_fix: Some(
            "populate `reason` (why this exemption exists), `expires` \
             (`YYYY-MM-DD` review date), and `owner` (team/individual). \
             Use the struct form: `{\"pattern\": \"…\", \"reason\": \"…\", \
             \"expires\": \"YYYY-MM-DD\", \"owner\": \"…\"}`."
                .into(),
        ),
    })
}

fn exempt_path_added_diagnostic(
    list_field: &str,
    entry: &str,
    mode: CheckMode,
    calibration: bool,
) -> Diagnostic {
    Diagnostic {
        rule_id: PG003_EXEMPT_PATH_ADDED.to_string(),
        severity: pg_severity(mode, calibration),
        span: lockfile_span(),
        concept: None,
        message: format!("new exempt path `{entry}` in `{list_field}`"),
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
    }
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
        .map(|e| e.prefix())
        .collect();
    current
        .acknowledged_empty
        .iter()
        .filter(|e| !base.contains(e.prefix()))
        .map(|entry| {
            let prefix = entry.prefix();
            Diagnostic {
                rule_id: PG004_ACKNOWLEDGED_EMPTY_ADDED.to_string(),
                severity: pg_severity(mode, calibration),
                span: lockfile_span(),
                concept: Some(prefix.to_string()),
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
            }
        })
        .collect()
}

// ---- PG009 acknowledged_empty new entry lacking metadata ----------

/// PG009 — fires when a new `acknowledged_empty` entry lacks `reason` /
/// `expires` / `owner`. Always uses strict severity (calibration does NOT
/// downgrade): calibration legitimizes the act of adding the entry
/// (PG004 → Advisory under calibration), but it does NOT excuse missing
/// justification metadata.
fn check_acknowledged_empty_metadata(
    current: &Lockfile,
    baseline: &Lockfile,
    mode: CheckMode,
) -> Vec<Diagnostic> {
    // Build the set of grandfathered prefixes from the baseline (both forms).
    let base_prefixes: std::collections::HashSet<&str> = baseline
        .acknowledged_empty
        .iter()
        .map(|e| e.prefix())
        .collect();

    let mut out = Vec::new();
    for entry in &current.acknowledged_empty {
        let prefix = entry.prefix();
        // Only check entries that are genuinely new (not in baseline).
        if base_prefixes.contains(prefix) {
            continue;
        }
        // Synthesize the metadata fields from whichever variant this is.
        let pg009_target: AcknowledgedEmpty = match entry {
            AcknowledgedEmptyEntry::Full(meta) => meta.clone(),
            AcknowledgedEmptyEntry::Legacy(s) => AcknowledgedEmpty {
                prefix: s.clone(),
                ..Default::default()
            },
        };
        if let Some(d) = acknowledged_empty_metadata_diagnostic(prefix, &pg009_target, mode) {
            out.push(d);
        }
    }
    out
}

fn acknowledged_empty_metadata_diagnostic(
    prefix: &str,
    meta: &AcknowledgedEmpty,
    mode: CheckMode,
) -> Option<Diagnostic> {
    let mut missing: Vec<&'static str> = Vec::new();
    if meta.reason.as_deref().is_none_or(str::is_empty) {
        missing.push("reason");
    }
    if meta.expires.as_deref().is_none_or(str::is_empty) {
        missing.push("expires");
    }
    if meta.owner.as_deref().is_none_or(str::is_empty) {
        missing.push("owner");
    }
    if missing.is_empty() {
        return None;
    }
    let missing_label = missing.join(", ");
    Some(Diagnostic {
        rule_id: PG009_ACKNOWLEDGED_EMPTY_LACKS_DEBT_METADATA.to_string(),
        severity: pg_strict_severity(mode),
        span: lockfile_span(),
        concept: Some(prefix.to_string()),
        message: format!(
            "new `acknowledged_empty` entry `{prefix}` lacks debt metadata \
             ({missing_label})"
        ),
        why: vec![
            "acknowledging a paradigm as empty silences its `LOCUS002` \
             vacancy nudge; without `reason` / `expires` / `owner` it \
             becomes invisible debt"
                .into(),
            format!("missing field(s): {missing_label}"),
            "PG009 is unaffected by `--allow-policy-calibration`: calibration \
             accepts the addition itself (PG004), but does not waive the \
             requirement to record why, when to revisit, and who owns it"
                .into(),
        ],
        suggested_fix: Some(
            "use the struct form: `{\"prefix\": \"…\", \"reason\": \"…\", \
             \"expires\": \"YYYY-MM-DD\", \"owner\": \"…\"}` and populate \
             all three fields."
                .into(),
        ),
    })
}

// ---- PG008 new OT.converter_paths entry --------------------------

fn check_new_converter_paths(
    current: &Lockfile,
    baseline: &Lockfile,
    mode: CheckMode,
    calibration: bool,
) -> Vec<Diagnostic> {
    let cur_ot: OtSection = current.paradigm_section("OT").unwrap_or_default();
    let base_ot: OtSection = baseline.paradigm_section("OT").unwrap_or_default();
    let base_set: std::collections::HashSet<&str> =
        base_ot.converter_paths.iter().map(String::as_str).collect();
    let mut out = Vec::new();
    for pattern in &cur_ot.converter_paths {
        if base_set.contains(pattern.as_str()) {
            continue;
        }
        out.push(converter_path_added_diagnostic(pattern, mode, calibration));
    }
    out
}

fn converter_path_added_diagnostic(
    pattern: &str,
    mode: CheckMode,
    calibration: bool,
) -> Diagnostic {
    Diagnostic {
        rule_id: PG008_CONVERTER_PATH_ADDED.to_string(),
        severity: pg_severity(mode, calibration),
        span: lockfile_span(),
        concept: None,
        message: format!(
            "new OT.converter_paths entry `{pattern}` widens architectural-authority surface"
        ),
        why: vec![
            format!("`paradigms.OT.converter_paths` did not contain `{pattern}` in baseline"),
            "converter_paths patterns grant OT004 authority to construct canonicals \
             across crate boundaries; adding a new pattern widens that surface without \
             fixing the underlying architecture"
                .into(),
        ],
        suggested_fix: Some(
            "if this is a deliberate calibration, re-run `locus check` with \
             `--allow-policy-calibration` (PG008 will fire as Advisory). Otherwise \
             remove the entry and address the underlying architectural concern."
                .into(),
        ),
    }
}

#[cfg(test)]
#[path = "policy_guard_tests.rs"]
mod tests;
