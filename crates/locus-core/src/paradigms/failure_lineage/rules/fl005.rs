//! FL005 — partial `if let Ok/Err = ...` without `else`.
//!
//! Catches the pattern `if let Ok(x) = result { ... }` (or its `Err`
//! inverse) with no `else` branch — the unmatched arm is silent, and
//! any failure (or success) on that path is dropped without
//! acknowledgement. Reads [`AirItem::PartialResultMatch`] items the visitor
//! emits since AIR v9.
//!
//! Severity: `mode.elevate(Severity::Warning)` — Warning in human, Fatal
//! under `--agent-strict`. Symmetric with FL003 / FL004.
//!
//! Lockfile-driven silence: stays quiet until `invariant_owner_paths`
//! is populated.

use locus_air::{AirItem, AirPartialResultMatch, AirWorkspace, ResultMatchVariant};

use super::super::lockfile_schema::FlSection;
use super::helpers::callsite_in_invariant_owner;
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

pub fn fl005(air: &AirWorkspace, section: &FlSection, mode: CheckMode) -> Vec<Diagnostic> {
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
                let AirItem::PartialResultMatch(p) = item else {
                    continue;
                };
                if callsite_in_invariant_owner(
                    module_path,
                    p.function.as_deref(),
                    &section.invariant_owner_paths,
                ) {
                    continue;
                }
                out.push(diagnostic_for_fl005(p, module_path, mode));
            }
        }
    }
    out
}

fn diagnostic_for_fl005(
    p: &AirPartialResultMatch,
    module_path: &str,
    mode: CheckMode,
) -> Diagnostic {
    let function_label = p
        .function
        .as_deref()
        .unwrap_or("<unknown enclosing function>");
    // AIR v13 turned `variant` from a String into `ResultMatchVariant`.
    // Render the architectural-side label (Success/Failure) and the
    // surface-side label (Ok/Err for Rust, ok/err for TS, etc. — Rust
    // adapter today uses Ok/Err) so the diagnostic stays readable in
    // both vocabularies.
    let (matched_label, unmatched_label) = match p.variant {
        ResultMatchVariant::Success => ("Ok", "Err"),
        ResultMatchVariant::Failure => ("Err", "Ok"),
    };
    Diagnostic {
        rule_id: "FL005".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: p.span.clone(),
        concept: None,
        message: format!(
            "partial `if let {matched_label}(...) = ...` (no `else` branch) in `{module_path}` \
             (fn `{function_label}`) — the `{unmatched_label}` arm is implicitly silent"
        ),
        why: vec![
            format!(
                "`if let {matched_label}(...) = ...` matches only the `{matched_label}` variant; the `{unmatched_label}` \
                 arm has no body and falls through silently"
            ),
            format!("enclosing function: `{function_label}`"),
            format!(
                "module `{module_path}` does not match any \
                 `paradigms.FL.invariant_owner_paths` pattern"
            ),
        ],
        suggested_fix: Some(format!(
            "rewrite as a `match` with both arms, or add an `else` branch \
             that handles the `{unmatched_label}` case (log, propagate, or \
             explicitly accept). If `{module_path}` is a legitimate \
             invariant owner (supervisor, test-support module), add it to \
             `paradigms.FL.invariant_owner_paths`. For a one-off accepted \
             partial match, suppress with `// locus: allow FL005 reason=\"…\" \
             expires=\"YYYY-MM-DD\"`"
        )),
    }
}
