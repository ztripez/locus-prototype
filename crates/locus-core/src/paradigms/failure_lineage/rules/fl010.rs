//! FL010 — invalid input silently converted into a valid default state.
//!
//! Reads [`AirItem::FallbackCall`] items (AIR v12). Fires when the
//! callee is `unwrap_or` or `or` AND the default-arg shape is
//! `Literal` or `Call`. Skips:
//!
//! - `unwrap_or_default` — FL002's territory (covered by the default
//!   `forbidden_callees` list).
//! - `default_shape == Empty` — same case as `unwrap_or_default`, no
//!   explicit default.
//! - `default_shape == Block` or `Other` — multi-statement fallback
//!   blocks might be doing real work (logging, propagation,
//!   compensating action). Conservative skip; the deterministic
//!   blocking rule shouldn't speculate about block contents.
//!
//! Severity: `mode.elevate(Severity::Warning)` — Warning in human,
//! Fatal under `--agent-strict`.
//!
//! Lockfile-driven silence: stays quiet until `invariant_owner_paths`
//! is populated, mirroring every other FL silent-discard rule.

use locus_air::{AirFallbackCall, AirItem, AirWorkspace, ArmBodyShape};

use super::super::lockfile_schema::FlSection;
use super::helpers::{body_shape_label, callsite_in_invariant_owner};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

pub fn fl010(air: &AirWorkspace, section: &FlSection, mode: CheckMode) -> Vec<Diagnostic> {
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
                let AirItem::FallbackCall(call) = item else {
                    continue;
                };
                if !is_fl010_callee(&call.callee) {
                    continue;
                }
                if !is_fl010_default_shape(call.default_shape) {
                    continue;
                }
                if callsite_in_invariant_owner(
                    module_path,
                    call.function.as_deref(),
                    &section.invariant_owner_paths,
                ) {
                    continue;
                }
                out.push(diagnostic_for_fl010(call, module_path, mode));
            }
        }
    }
    out
}

/// True for the FL010-relevant fallback callees. `unwrap_or_default`
/// is deliberately excluded — FL002 covers the no-arg form.
fn is_fl010_callee(callee: &str) -> bool {
    matches!(callee, "unwrap_or" | "or")
}

/// True when the default-arg shape is one FL010 fires on. `Empty`
/// means the call had no default arg (i.e. `unwrap_or_default()`),
/// which FL002 already covers. `Block` and `Other` could be doing
/// real recovery work, so the deterministic rule passes on them.
fn is_fl010_default_shape(shape: ArmBodyShape) -> bool {
    matches!(shape, ArmBodyShape::Literal | ArmBodyShape::Call)
}

fn diagnostic_for_fl010(call: &AirFallbackCall, module_path: &str, mode: CheckMode) -> Diagnostic {
    let function_label = call
        .function
        .as_deref()
        .unwrap_or("<unknown enclosing function>");
    let shape_label = body_shape_label(call.default_shape);
    let shape_token = fallback_shape_token(call.default_shape);
    Diagnostic {
        rule_id: "FL010".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: call.span.clone(),
        concept: None,
        message: format!(
            ".{}(...) returns a silent {shape_token} default in `{module_path}` \
             (fn `{function_label}`) — invalid input gets converted to valid state",
            call.callee,
        ),
        why: vec![
            format!("module `{module_path}`"),
            format!("enclosing function: `{function_label}`"),
            format!("callee `{}`", call.callee),
            format!("default-arg shape: {shape_label}"),
            "the failure path is silently replaced with a default value".into(),
            format!(
                "no `paradigms.FL.invariant_owner_paths` entry covers `{module_path}` \
                 — the rule treats this site as a non-owner"
            ),
        ],
        suggested_fix: Some(format!(
            "propagate the failure with `?`; or map the error to a typed \
             domain error (e.g. `.map_err(MyError::from)?`); or, if \
             `{module_path}` is a legitimate fallback owner (supervisor, \
             startup-asserting bin entry), add it to \
             `paradigms.FL.invariant_owner_paths`. For a one-off accepted \
             default, suppress with `// locus: allow FL010 reason=\"…\" \
             expires=\"YYYY-MM-DD\"`"
        )),
    }
}

/// Render `default_shape` as a snake_case token suitable for the
/// FL010 headline message — mirrors how FL011/FL007 use
/// `body_shape_label` but without spaces for the "silent X default"
/// phrasing.
fn fallback_shape_token(shape: ArmBodyShape) -> &'static str {
    match shape {
        ArmBodyShape::Empty => "empty",
        ArmBodyShape::Literal => "literal",
        ArmBodyShape::Call => "call",
        ArmBodyShape::Return => "return",
        ArmBodyShape::ErrorPropagation => "propagate",
        ArmBodyShape::Block => "block",
        ArmBodyShape::Other => "other",
    }
}
