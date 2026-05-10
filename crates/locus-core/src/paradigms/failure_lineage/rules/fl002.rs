//! FL002 — unwrap-family failure swallowing.
//!
//! For every `AirItem::CallSite` whose `kind` is `Method` or `Macro` and
//! whose `callee` (last `::` segment for path-qualified macros) matches any
//! pattern in `forbidden_callees`, fire a diagnostic when the call site's
//! enclosing-file `module_path` does NOT match any pattern in
//! `invariant_owner_paths`. Function-shaped calls are intentionally
//! excluded — `panic!` is a `Macro`, `unwrap`/`expect` are `Method`s, and
//! `Function` calls would only false-positive on user code that happens to
//! name a function `unwrap`.
//!
//! Severity: Warning by default; Fatal under `--agent-strict`. The fact is
//! deterministic — `mode.elevate(Severity::Warning)` — but the policy is a
//! lockfile decision, so the human-mode posture is "warn, don't break CI".
//!
//! Silent until `invariant_owner_paths` is populated, mirroring every other
//! lockfile-driven rule. The default `forbidden_callees` list is non-empty
//! but the rule still doesn't fire until the user has declared *where the
//! legitimate panic-callsites live*.

use locus_air::{AirCallSite, AirItem, AirWorkspace, CallKind};

use super::super::lockfile_schema::{FlSection, matches_pattern};
use super::helpers::callsite_in_invariant_owner;
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

pub fn fl002(air: &AirWorkspace, section: &FlSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.invariant_owner_paths.is_empty() || section.forbidden_callees.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            for item in &file.items {
                let AirItem::CallSite(cs) = item else {
                    continue;
                };
                // Function-shaped calls don't carry the unwrap/panic
                // semantics we care about (and would false-positive on user
                // free functions named `unwrap`). Method and Macro only.
                if !matches!(cs.kind, CallKind::Method | CallKind::Meta) {
                    continue;
                }
                if callsite_in_invariant_owner(
                    module_path,
                    cs.function.as_deref(),
                    &section.invariant_owner_paths,
                ) {
                    continue;
                }
                let last = cs.callee.rsplit("::").next().unwrap_or(&cs.callee);
                let Some(forbidden_pattern) = section
                    .forbidden_callees
                    .iter()
                    .find(|pat| matches_pattern(pat, last))
                else {
                    continue;
                };
                out.push(diagnostic_for_fl002(
                    cs,
                    module_path,
                    forbidden_pattern,
                    mode,
                ));
            }
        }
    }
    out
}

fn diagnostic_for_fl002(
    cs: &AirCallSite,
    module_path: &str,
    forbidden_pattern: &str,
    mode: CheckMode,
) -> Diagnostic {
    let function_label = cs
        .function
        .as_deref()
        .unwrap_or("<unknown enclosing function>");
    Diagnostic {
        rule_id: "FL002".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: cs.span.clone(),
        concept: None,
        message: format!(
            "panic-shaped call `{}` in `{module_path}` (fn `{function_label}`) — \
             matches `paradigms.FL.forbidden_callees` pattern `{forbidden_pattern}`",
            cs.callee,
        ),
        why: vec![
            format!("callee `{}`", cs.callee),
            format!("enclosing function: `{function_label}`"),
            format!(
                "module `{module_path}` does not match any \
                 `paradigms.FL.invariant_owner_paths` pattern"
            ),
            format!(
                "callee matches forbidden pattern `{forbidden_pattern}` in \
                 `paradigms.FL.forbidden_callees`"
            ),
        ],
        suggested_fix: Some(format!(
            "replace this `{}` with structured error propagation — return \
             a `Result` and let the caller handle the failure path — or, if \
             `{module_path}` is a legitimate invariant owner (supervisor, \
             startup-asserting entry point, test-support module), accept it \
             by adding the module to `paradigms.FL.invariant_owner_paths` \
             in `locus.lock`",
            cs.callee,
        )),
    }
}
