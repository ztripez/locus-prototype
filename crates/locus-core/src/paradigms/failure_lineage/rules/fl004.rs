//! FL004 — `let _ = expr;` silent-discard binding.
//!
//! Closes the gap FL003 leaves open: FL003 sees `result.ok()` /
//! `.err()` / `.unwrap_or_else()` (method-call shape), but it can't see
//! the `let _ = result;` shape because it's a binding, not a method call.
//! AIR v9 added [`AirItem::SilentDiscard`] for exactly this case — the
//! visitor records `let _ = <call>` statements with the rendered callee.
//!
//! Detection rules:
//! - Only `DiscardKind::Method` / `Function` / `Macro` are considered.
//!   `Other` discards (`let _ = some_field;`) are skipped — the
//!   false-positive surface for arbitrary expression discards is too
//!   large to be useful.
//! - The discarded callee must NOT match any pattern in
//!   `silent_discard_allowed_callees` (default covers the canonical
//!   fire-and-forget shapes: `lock`, `send`, `drop`, `set_logger`,
//!   `subscribe`, `try_init`).
//! - The enclosing file's `module_path` must NOT match any
//!   `invariant_owner_paths` pattern.
//!
//! Severity: `mode.elevate(Severity::Warning)` — Warning in human, Fatal
//! under `--agent-strict`. Same posture as FL002 / FL003.
//!
//! Lockfile-driven silence: stays quiet until `invariant_owner_paths`
//! is populated, regardless of how the allowlist is configured.

use locus_air::{AirItem, AirSilentDiscard, AirWorkspace, DiscardKind};

use super::super::lockfile_schema::{FlSection, matches_pattern};
use super::helpers::callsite_in_invariant_owner;
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

pub fn fl004(air: &AirWorkspace, section: &FlSection, mode: CheckMode) -> Vec<Diagnostic> {
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
                let AirItem::SilentDiscard(d) = item else {
                    continue;
                };
                if matches!(d.kind, DiscardKind::Other) {
                    continue;
                }
                let Some(callee) = d.callee.as_deref() else {
                    continue;
                };
                if section
                    .silent_discard_allowed_callees
                    .iter()
                    .any(|pat| matches_pattern(pat, callee))
                {
                    continue;
                }
                if callsite_in_invariant_owner(
                    module_path,
                    d.function.as_deref(),
                    &section.invariant_owner_paths,
                ) {
                    continue;
                }
                out.push(diagnostic_for_fl004(d, module_path, callee, mode));
            }
        }
    }
    out
}

fn diagnostic_for_fl004(
    d: &AirSilentDiscard,
    module_path: &str,
    callee: &str,
    mode: CheckMode,
) -> Diagnostic {
    let function_label = d
        .function
        .as_deref()
        .unwrap_or("<unknown enclosing function>");
    let kind_label = match d.kind {
        DiscardKind::Method => "method",
        DiscardKind::Function => "function",
        DiscardKind::Meta => "macro",
        DiscardKind::Other => "expression",
    };
    Diagnostic {
        rule_id: "FL004".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: d.span.clone(),
        concept: None,
        message: format!(
            "discarded binding `let _ = {callee}(...)` ({kind_label}) in `{module_path}` \
             (fn `{function_label}`) — failure (if any) is silently dropped"
        ),
        why: vec![
            format!("`let _ = ...` discards the binding without inspecting the value"),
            format!("discarded callee: `{callee}` ({kind_label})"),
            format!("enclosing function: `{function_label}`"),
            format!(
                "module `{module_path}` does not match any \
                 `paradigms.FL.invariant_owner_paths` pattern"
            ),
            format!(
                "callee does not match any \
                 `paradigms.FL.silent_discard_allowed_callees` pattern"
            ),
        ],
        suggested_fix: Some(format!(
            "if `{callee}` returns a `Result`, propagate the error with \
             `?` instead of dropping it; if the discard is intentional \
             and the callee is a known fire-and-forget pattern (e.g. \
             `lock`, `send`, `drop`), add it to \
             `paradigms.FL.silent_discard_allowed_callees`. For a one-off \
             accepted discard, suppress with `// locus: allow FL004 reason=\"…\" \
             expires=\"YYYY-MM-DD\"`. If `{module_path}` is a legitimate \
             invariant owner, add it to `paradigms.FL.invariant_owner_paths`"
        )),
    }
}
