//! MO rule implementations.
//!
//! Implemented:
//! - [`mo001`]: too many public top-level types in a single file.
//! - [`mo002`]: responsibility entropy in a single file (canonical/boundary/
//!   converter hints, handler-named functions, persistence imports, io call
//!   sites — too many distinct architectural roles co-existing).
//! - [`mo003`]: canonical hint co-located with a boundary hint in the same file.
//! - [`mo004`]: canonical hint co-located with a handler-named function in the
//!   same file.

use locus_air::{
    AirFile, AirHint, AirImport, AirItem, AirSpan, AirWorkspace, HintKind, Visibility,
};

use super::lockfile_schema::{MoSection, matches_name_glob, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

fn mo001_why(
    module_path: &str,
    count: u32,
    budget: u32,
    default_budget: u32,
    matched_override: Option<&super::lockfile_schema::MoOverride>,
    section: &MoSection,
) -> Vec<String> {
    let mut why = vec![
        format!("file `{module_path}` defines {count} public top-level type(s)"),
        if let Some(o) = matched_override {
            format!("budget {budget} from override `module = {}`", o.module)
        } else {
            format!("budget {budget} (workspace default)")
        },
    ];
    if matched_override.is_none() && section.default_max_public_types.is_none() {
        why.push(format!(
            "no `default_max_public_types` configured; using built-in fallback {default_budget}",
        ));
    }
    why
}

fn mo001_diagnostic(
    module_path: &str,
    count: u32,
    budget: u32,
    default_budget: u32,
    span: AirSpan,
    matched_override: Option<&super::lockfile_schema::MoOverride>,
    section: &MoSection,
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
        rule_id: "MO001".to_string(),
        severity: mode.elevate(Severity::Warning),
        span,
        concept: None,
        message: format!(
            "module `{module_path}` has {count} public top-level types (budget {budget})"
        ),
        why: mo001_why(module_path, count, budget, default_budget, matched_override, section),
        suggested_fix: Some(
            "split the module into submodules each owning one architectural role, \
             or — if this density is intended (e.g. an API surface) — raise the \
             budget by adding an override to `paradigms.MO.overrides` in \
             `locus.lock`"
                .into(),
        ),
    }
}

/// MO001 — module file has too many public top-level types.
///
/// For each `AirFile` with a `module_path`, count `AirItem::Type` items
/// whose visibility is `Public`. Compare against the file's effective
/// budget (override wins, then default, then built-in fallback).
/// Fires when `count > budget`. One diagnostic per file.
///
/// Severity: Warning by default; `--agent-strict` elevates to Fatal.
/// Fires by default on un-onboarded code using built-in fallback budgets.
pub fn mo001(air: &AirWorkspace, section: &MoSection, mode: CheckMode) -> Vec<Diagnostic> {
    let default_budget = section.effective_default();
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let count = file
                .items
                .iter()
                .filter(
                    |item| matches!(item, AirItem::Type(t) if t.visibility == Visibility::Public),
                )
                .count() as u32;
            let matched_override = section.matching_override(module_path);
            let budget = matched_override
                .map(|o| o.max_public_types)
                .unwrap_or(default_budget);
            if count <= budget {
                continue;
            }
            // Anchor at the first public type, or line 1 of the file.
            let span = file
                .items
                .iter()
                .find_map(|item| match item {
                    AirItem::Type(t) if t.visibility == Visibility::Public => Some(t.span.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| locus_air::AirSpan::new(file.path.clone(), 1, 1));
            out.push(mo001_diagnostic(
                module_path, count, budget, default_budget, span,
                matched_override, section, mode,
            ));
        }
    }
    out
}

/// In-code default callee patterns flagged as "io" by MO002. Kept as a
/// constant rather than a lockfile field because MO002's spec only enumerates
/// `entropy_threshold`, `handler_name_patterns`, and `persistence_import_patterns`
/// as configurable surface — the io contributor is a built-in heuristic, not
/// user policy. If a project legitimately makes io calls in a non-blob file,
/// the noise is absorbed by the entropy threshold (count must reach 3+).
const IO_CALLEE_PATTERNS: &[&str] = &[
    "*::fs::*",
    "*::net::*",
    "*::TcpStream::*",
    "*::TcpListener::*",
    "*::UdpSocket::*",
];

/// Anchor a per-file diagnostic at a useful span: the first item in the
/// file when present, otherwise line 1 of the file.
fn file_anchor_span(file: &AirFile) -> AirSpan {
    file.items
        .iter()
        .map(|item| match item {
            AirItem::Type(t) => t.span.clone(),
            AirItem::Function(f) => f.span.clone(),
            AirItem::Conversion(c) => c.span.clone(),
            AirItem::Import(i) => i.span.clone(),
            AirItem::Impl(i) => i.span.clone(),
            AirItem::TruthAction(a) => a.span.clone(),
            AirItem::CallSite(c) => c.span.clone(),
            AirItem::Usage(u) => u.span.clone(),
            AirItem::SilentDiscard(d) => d.span.clone(),
            AirItem::PartialResultMatch(p) => p.span.clone(),
            AirItem::MatchArm(a) => a.span.clone(),
            AirItem::ClosureMethodCall(c) => c.span.clone(),
            AirItem::FallbackCall(c) => c.span.clone(),
            AirItem::RetryLoop(l) => l.span.clone(),
            AirItem::ScrutineeLiteral(l) => l.span.clone(),
        })
        .next()
        .unwrap_or_else(|| AirSpan::new(file.path.clone(), 1, 1))
}

fn has_canonical_hint(file: &AirFile) -> bool {
    file.hints
        .iter()
        .any(|h| matches!(h.kind, HintKind::Canonical))
}

fn has_boundary_hint(file: &AirFile) -> bool {
    file.hints
        .iter()
        .any(|h| matches!(h.kind, HintKind::Boundary { .. }))
}

fn has_converter_hint(file: &AirFile) -> bool {
    file.hints
        .iter()
        .any(|h| matches!(h.kind, HintKind::Converter))
}

fn has_handler_named_function(file: &AirFile, patterns: &[&str]) -> bool {
    file.items.iter().any(|item| {
        let AirItem::Function(f) = item else {
            return false;
        };
        patterns.iter().any(|p| matches_name_glob(p, &f.name))
    })
}

fn has_persistence_import(file: &AirFile, patterns: &[&str]) -> bool {
    file.items.iter().any(|item| {
        let AirItem::Import(AirImport { path, .. }) = item else {
            return false;
        };
        patterns.iter().any(|p| matches_pattern(p, path))
    })
}

fn has_io_call_site(file: &AirFile) -> bool {
    file.items.iter().any(|item| {
        let AirItem::CallSite(c) = item else {
            return false;
        };
        IO_CALLEE_PATTERNS
            .iter()
            .any(|p| matches_pattern(p, &c.callee))
    })
}

fn mo002_diagnostic(
    module_path: &str,
    count: u32,
    threshold: u32,
    role_list: &str,
    span: AirSpan,
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
        rule_id: "MO002".to_string(),
        severity: mode.elevate(Severity::Warning),
        span,
        concept: None,
        message: format!(
            "module `{module_path}` carries {count} distinct architectural roles \
             ({role_list}); threshold is {threshold}"
        ),
        why: vec![
            format!("file `{module_path}` exhibits roles: {role_list}"),
            format!(
                "MO002 entropy threshold is {threshold} (configured via \
                 `paradigms.MO.entropy_threshold` in `locus.lock`)"
            ),
            "a single file mixing canonical/boundary/converter/handler/persistence/io \
             roles is a responsibility blob — split each role into its own module"
                .into(),
        ],
        suggested_fix: Some(
            "split this file along role boundaries: canonical types into \
             `domain/`, boundary DTOs into `dto/`, conversions into a \
             `convert.rs`, handlers into a `handlers/` module, and \
             persistence/io into an adapter layer. If the density is \
             intentional, raise `paradigms.MO.entropy_threshold` in \
             `locus.lock`."
                .into(),
        ),
    }
}

/// MO002 — responsibility entropy in a single file.
///
/// Counts how many of the six architectural roles a file carries:
/// canonical hint, boundary hint, converter hint, handler-named function,
/// persistence import, or IO call site. Fires when `count >= threshold`
/// (default 3).
///
/// Severity: Warning by default; `--agent-strict` elevates to Fatal.
/// Fires by default with built-in fallback threshold and pattern lists.
pub fn mo002(air: &AirWorkspace, section: &MoSection, mode: CheckMode) -> Vec<Diagnostic> {
    let threshold = section.effective_entropy_threshold();
    let handler_patterns = section.effective_handler_name_patterns();
    let persistence_patterns = section.effective_persistence_import_patterns();
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let mut roles: Vec<&'static str> = Vec::new();
            if has_canonical_hint(file) {
                roles.push("canonical");
            }
            if has_boundary_hint(file) {
                roles.push("boundary");
            }
            if has_converter_hint(file) {
                roles.push("converter");
            }
            if has_handler_named_function(file, &handler_patterns) {
                roles.push("handler");
            }
            if has_persistence_import(file, &persistence_patterns) {
                roles.push("persistence");
            }
            if has_io_call_site(file) {
                roles.push("io");
            }
            let count = roles.len() as u32;
            if count < threshold {
                continue;
            }
            let span = file_anchor_span(file);
            let role_list = roles.join(", ");
            out.push(mo002_diagnostic(module_path, count, threshold, &role_list, span, mode));
        }
    }
    out
}

fn mo003_diagnostic(module_path: &str, span: AirSpan, mode: CheckMode) -> Diagnostic {
    Diagnostic {
        rule_id: "MO003".to_string(),
        severity: mode.elevate(Severity::Warning),
        span,
        concept: None,
        message: format!("module `{module_path}` mixes canonical and boundary types"),
        why: vec![
            format!(
                "file `{module_path}` has both a `// locus: ot canonical` \
                 and a `// locus: ot boundary` hint"
            ),
            "canonical types are the domain truth; boundary types are the \
             wire/protocol shadow of that truth — keeping them in one file \
             blurs ownership and makes the converter direction ambiguous"
                .into(),
        ],
        suggested_fix: Some(
            "split the file: move canonical types into a `domain/` module \
             and boundary types into a `dto/` module, with explicit \
             `From`/`TryFrom` converters between them"
                .into(),
        ),
    }
}

/// MO003 — canonical type co-located with a boundary type in the same file.
///
/// Fires for any `AirFile` containing both an `AirHint::Canonical` and an
/// `AirHint::Boundary`. The two hints describe opposing roles — canonical
/// types are the domain truth; boundary types are the wire/protocol shadow
/// of that truth — so co-locating them in one file blurs ownership.
///
/// Severity: Warning by default; `--agent-strict` elevates to Fatal. No
/// new lockfile fields — the rule is a pure structural check on hints.
pub fn mo003(air: &AirWorkspace, mode: CheckMode) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            if !(has_canonical_hint(file) && has_boundary_hint(file)) {
                continue;
            }
            let span = file
                .hints
                .iter()
                .find(|h| matches!(h.kind, HintKind::Canonical))
                .map(|h: &AirHint| h.span.clone())
                .unwrap_or_else(|| file_anchor_span(file));
            out.push(mo003_diagnostic(module_path, span, mode));
        }
    }
    out
}

fn mo004_diagnostic(
    module_path: &str,
    handler: &locus_air::AirFunction,
    handler_patterns: &[&str],
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
        rule_id: "MO004".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: handler.span.clone(),
        concept: None,
        message: format!(
            "module `{module_path}` co-locates handler `{}` with a canonical concept",
            handler.name
        ),
        why: vec![
            format!("file `{module_path}` has a `// locus: ot canonical` hint"),
            format!(
                "function `{}` matches handler name pattern (one of {handler_patterns:?})",
                handler.name,
            ),
            "handlers belong to an application/transport layer; canonical \
             types belong to the domain layer — co-locating them couples \
             the two and makes the canonical reusable from non-handler \
             callers harder"
                .into(),
        ],
        suggested_fix: Some(format!(
            "move `{}` into a `handlers/` module that depends on the \
             canonical, instead of defining both in the same file. If the \
             name match is a false positive, narrow \
             `paradigms.MO.handler_name_patterns` in `locus.lock`.",
            handler.name
        )),
    }
}

/// MO004 — handler co-located with a canonical concept in the same file.
///
/// Fires for any `AirFile` containing both an `AirHint::Canonical` *and* a
/// function whose name matches `handler_name_patterns` (reuses the same
/// patterns as MO002, with the same default fallback `*_handler`/`handle_*`).
///
/// Handlers belong to an application/transport layer; canonical types belong
/// to the domain layer. Co-locating them couples the two.
///
/// Severity: Warning by default; `--agent-strict` elevates to Fatal.
pub fn mo004(air: &AirWorkspace, section: &MoSection, mode: CheckMode) -> Vec<Diagnostic> {
    let handler_patterns = section.effective_handler_name_patterns();
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            if !has_canonical_hint(file) {
                continue;
            }
            let handler = file.items.iter().find_map(|item| {
                let AirItem::Function(f) = item else {
                    return None;
                };
                if handler_patterns
                    .iter()
                    .any(|p| matches_name_glob(p, &f.name))
                {
                    Some(f)
                } else {
                    None
                }
            });
            let Some(handler) = handler else {
                continue;
            };
            out.push(mo004_diagnostic(module_path, handler, &handler_patterns, mode));
        }
    }
    out
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod rules_tests;
