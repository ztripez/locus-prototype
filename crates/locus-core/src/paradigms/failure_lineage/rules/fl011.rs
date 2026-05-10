//! FL011 — default-variant arm as failure sink.
//!
//! Reads [`AirItem::MatchArm`] items. Fires on an arm whose pattern is
//! the bare wildcard `_` (or `_ if guard` — anything that trims to `_`
//! at the head) AND whose body shape is `Empty`, `Literal`, or `Call`.
//! The "I don't know what to do here so I'll just default" anti-pattern
//! on enum scrutinees: an unknown variant should be an explicit error
//! or explicit ignore, not a silent fall-through.
//!
//! Distinct from FL007: FL007 fires on `Err(_)` shapes (any pattern
//! targeting the `Err` variant); FL011 fires only on the bare `_`
//! pattern. They never overlap.
//!
//! Severity: `mode.elevate(Severity::Warning)`.
//!
//! Lockfile-driven silence: stays quiet until `invariant_owner_paths`
//! is populated.

use locus_air::{AirItem, AirMatchArm, AirWorkspace};

use super::super::lockfile_schema::FlSection;
use super::helpers::{body_shape_label, callsite_in_invariant_owner, is_silent_body_shape};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

pub fn fl011(air: &AirWorkspace, section: &FlSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.invariant_owner_paths.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            for item in &file.items {
                let AirItem::MatchArm(arm) = item else {
                    continue;
                };
                if !is_bare_wildcard_pattern(&arm.pattern) {
                    continue;
                }
                if !is_silent_body_shape(arm.body_shape) {
                    continue;
                }
                if callsite_in_invariant_owner(
                    module_path,
                    arm.function.as_deref(),
                    &section.invariant_owner_paths,
                ) {
                    continue;
                }
                out.push(diagnostic_for_fl011(arm, module_path, mode));
            }
        }
    }
    out
}

/// True when the arm pattern is a bare wildcard — `_` or `_ if guard`.
/// We deliberately accept the guard form so `_ if cond => 0` still
/// trips FL011: a guarded silent default is the same anti-pattern.
fn is_bare_wildcard_pattern(pattern: &str) -> bool {
    let p = pattern.trim();
    p == "_" || p.starts_with("_ if ") || p.starts_with("_ @ ")
}

fn diagnostic_for_fl011(arm: &AirMatchArm, module_path: &str, mode: CheckMode) -> Diagnostic {
    let function_label = arm
        .function
        .as_deref()
        .unwrap_or("<unknown enclosing function>");
    let body_label = body_shape_label(arm.body_shape);
    Diagnostic {
        rule_id: "FL011".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: arm.span.clone(),
        concept: None,
        message: format!(
            "bare `_` arm in `{module_path}` (fn `{function_label}`) returns a \
             `{body_label}` default — unknown variants silently routed to a sink"
        ),
        why: vec![
            format!("scrutinee `{}`", arm.scrutinee),
            format!("module `{module_path}`"),
            format!("enclosing function: `{function_label}`"),
            "arm pattern is `_`".into(),
            format!("arm body is a `{body_label}` (silent default)"),
            "an unknown enum variant should be an error or explicit ignore, \
             not a default-value fall-through"
                .into(),
        ],
        suggested_fix: Some(format!(
            "enumerate the missing variants explicitly so the compiler enforces \
             exhaustiveness; or, if the catch-all is intentional, rewrite as \
             `_ => Err(SomeError::Unknown)` so the failure has an owner. If \
             `{module_path}` is a legitimate invariant owner, add it to \
             `paradigms.FL.invariant_owner_paths`. For a one-off accepted \
             default, suppress with `// locus: allow FL011 reason=\"…\" \
             expires=\"YYYY-MM-DD\"`"
        )),
    }
}
