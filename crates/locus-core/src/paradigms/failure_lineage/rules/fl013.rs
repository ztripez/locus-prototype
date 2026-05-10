//! FL013 — lossy error stringification in error returns.
//!
//! Catches the pattern: a function returns `Result<T, String>` (or
//! `Result<T, &str>`) and somewhere inside it a call site stringifies a
//! value — `.to_string()`, `format!(...)`, `format(...)`, `.display()`.
//! The function isn't carrying a typed error out; it's collapsing
//! whatever failed into a string at the source, so the call site that
//! should have produced a typed `Err(SomeErrorVariant)` lossy-converted
//! instead. Failure lineage is gone the moment the function returns.
//!
//! Detection:
//! - Walk every `AirItem::Function` whose `return_type` parses as
//!   `Result<_, String>` or `Result<_, &str>` (the same extractor FL001
//!   uses, with normalisation that strips one leading `&`). Custom
//!   `Result<T>` aliases without a top-level comma are skipped.
//! - For each matching function, walk every `AirItem::CallSite` whose
//!   `function == Some(func.symbol)`. Fire when **any** call site's
//!   callee (last `::` segment) matches one of `["to_string",
//!   "format!", "format", "display"]`. Bare-name match — receiver type
//!   resolution is out of scope, same posture as FL003.
//! - Skip files (and call-site enclosing-modules) that match
//!   `invariant_owner_paths`. We reuse the existing FL field rather
//!   than introduce a new lockfile knob — the semantics line up
//!   (modules where the rule's anti-pattern is legitimate, e.g.
//!   CLI top-level / test fixtures).
//!
//! Severity: `mode.elevate(Severity::Warning)` — Warning in human, Fatal
//! under `--agent-strict`. Same posture as FL002 / FL003 / FL004 / FL005.
//!
//! Silent until `invariant_owner_paths` is populated, mirroring every
//! other lockfile-driven FL rule.

use locus_air::{AirCallSite, AirFunction, AirItem, AirWorkspace};

use super::super::lockfile_schema::{FlSection, matches_pattern};
use super::helpers::{callsite_in_invariant_owner, extract_result_error_type};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

pub fn fl013(air: &AirWorkspace, section: &FlSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.invariant_owner_paths.is_empty() {
        return Vec::new();
    }

    const STRINGIFY_CALLEES: &[&str] = &["to_string", "format!", "format", "display"];

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            // Pre-compute file-level invariant-owner status. If the file
            // is on the allowlist, every call site inside it is too.
            let file_is_owner = section
                .invariant_owner_paths
                .iter()
                .any(|p| matches_pattern(p, module_path));
            if file_is_owner {
                continue;
            }

            // Pass 1: collect Result<_, String|&str> functions in this file.
            let stringy_fns: Vec<&AirFunction> = file
                .items
                .iter()
                .filter_map(|item| match item {
                    AirItem::Function(func) => {
                        let ret = func.return_type.as_deref()?;
                        let err_ty = extract_result_error_type(ret)?;
                        let normalised = err_ty.trim().strip_prefix('&').unwrap_or(err_ty).trim();
                        if normalised == "String" || normalised == "str" {
                            Some(func)
                        } else {
                            None
                        }
                    }
                    _ => None,
                })
                .collect();
            if stringy_fns.is_empty() {
                continue;
            }

            // Pass 2: scan call sites; for each matching enclosing-fn, fire
            // on the first stringifying callee we see (one diag per fn —
            // multiple stringifications inside one function reduce to a
            // single signal; the fix is the same regardless of count).
            for func in stringy_fns {
                if callsite_in_invariant_owner(
                    module_path,
                    Some(&func.symbol),
                    &section.invariant_owner_paths,
                ) {
                    continue;
                }
                let hit = file.items.iter().find_map(|item| {
                    let AirItem::CallSite(cs) = item else {
                        return None;
                    };
                    if cs.function.as_deref() != Some(func.symbol.as_str()) {
                        return None;
                    }
                    let last = cs.callee.rsplit("::").next().unwrap_or(&cs.callee);
                    if STRINGIFY_CALLEES.contains(&last) {
                        Some(cs)
                    } else {
                        None
                    }
                });
                if let Some(cs) = hit {
                    out.push(diagnostic_for_fl013(func, cs, module_path, mode));
                }
            }
        }
    }
    out
}

fn diagnostic_for_fl013(
    func: &AirFunction,
    cs: &AirCallSite,
    module_path: &str,
    mode: CheckMode,
) -> Diagnostic {
    let ret = func.return_type.as_deref().unwrap_or("?");
    Diagnostic {
        rule_id: "FL013".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: cs.span.clone(),
        concept: None,
        message: format!(
            "lossy error stringification in `{}` (`{ret}`) — call site `{}` collapses \
             a typed value into a string before it leaves the function",
            func.name, cs.callee,
        ),
        why: vec![
            format!("function `{}` (`{}`)", func.name, func.symbol),
            format!("return type `{ret}` — `String` / `&str` errors carry no failure lineage"),
            format!("call site `{}` (line {})", cs.callee, cs.span.line_start),
            format!(
                "module `{module_path}` does not match any \
                 `paradigms.FL.invariant_owner_paths` pattern"
            ),
            "stringifying an error at the source erases the variant the caller \
             would otherwise pattern-match against; the failure mode is gone by \
             the time the function returns"
                .into(),
        ],
        suggested_fix: Some(format!(
            "define a typed error (`enum {}Error {{ ... }}` or similar) and use \
             `?` propagation so each failure mode keeps its own variant; if \
             `{module_path}` is a legitimate string-error surface (top-level \
             CLI, test fixture, supervisor), add it to \
             `paradigms.FL.invariant_owner_paths`. For a one-off accepted \
             stringification, suppress with `// locus: allow FL013 reason=\"…\" \
             expires=\"YYYY-MM-DD\"`",
            capitalize_first_fl013(&func.name),
        )),
    }
}

/// Capitalize-first helper, local to FL013. Duplicated rather than imported
/// from a sibling paradigm (CLAUDE.md: paradigms must not depend on each
/// other).
fn capitalize_first_fl013(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {
            let mut out = String::with_capacity(s.len());
            out.push(c.to_ascii_uppercase());
            out.extend(chars);
            out
        }
        _ => s.to_string(),
    }
}
