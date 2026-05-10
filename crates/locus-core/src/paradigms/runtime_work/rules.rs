//! RW rule implementations.
//!
//! Implemented:
//! - [`rw001`]: spawn-shaped fact outside any declared runtime owner module.
//! - [`rw002`]: blocking-call fact outside any declared runtime owner module.
//! - [`rw003`]: `Mutex` / `RwLock` field outside the runtime-ownership
//!   boundary.
//! - [`rw004`]: `static` / `OnceCell` / `Lazy`-shaped global outside the
//!   runtime-ownership boundary.
//! - [`rw005`]: blocking call inside a function carrying a `HotPath`
//!   marker fact (`// locus: fact hot_path`).
//! - [`rw006`]: spawn inside a function carrying a `HotPath` marker fact.
//!
//! RW001–RW004 are lockfile-driven (they wait for `runtime_owner_paths`).
//! RW005 / RW006 are marker-driven instead: the user's `// locus: fact
//! hot_path` hint *is* the opt-in, so they fire as soon as a marked
//! function picks up a blocking-call or spawn fact.

use std::collections::HashSet;

use locus_air::{AirFact, AirItem, AirSpan, AirType, AirWorkspace, FactKind, FactTarget, TypeKind};

use super::lockfile_schema::{RwSection, matches_pattern, type_text_matches};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// RW001 — spawn outside the runtime-ownership boundary.
///
/// For every `FactKind::SpawnedWork` fact produced by a loader, look up the
/// targeted function's file and fire when the file's `module_path` does
/// NOT match any pattern in `runtime_owner_paths`.
///
/// Always Fatal: per the spec, runtime-ownership violations are structural —
/// `tokio::spawn` (or any equivalent) dropped into a handler hides
/// concurrency, error-propagation, and lifecycle concerns from the layer
/// that owns them.
///
/// Silent when `runtime_owner_paths` is empty: we wait for the user to
/// declare where their runtime owners live before flagging anything.
/// Functions whose file has no `module_path` are skipped — we can't decide
/// anything about them.
pub fn rw001(air: &AirWorkspace, section: &RwSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.runtime_owner_paths.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for fact in &air.facts {
        if fact.kind != FactKind::SpawnedWork {
            continue;
        }
        let FactTarget::Function { symbol } = &fact.target else {
            continue;
        };
        let Some((module_path, fn_span)) = lookup_function(air, symbol) else {
            continue;
        };
        // Match either the file's `module_path` or the function symbol
        // itself — inline `mod tests {}` blocks live at a deeper symbol
        // path than the file, so a `*::tests::*` pattern would silently
        // miss them if we only checked the file. Same fix FL002–FL005
        // got via `containing_module_of`.
        if section
            .runtime_owner_paths
            .iter()
            .any(|pat| matches_pattern(pat, module_path) || matches_pattern(pat, symbol))
        {
            continue; // file or function is itself a runtime owner
        }
        out.push(diagnostic_for(fact, symbol, module_path, fn_span, mode));
    }
    out
}

/// RW002 — blocking call outside the runtime-ownership boundary.
///
/// For every `FactKind::BlockingCall` fact produced by a loader (e.g. the
/// std-rt loader recognising `std::fs::read`, `std::thread::sleep`,
/// `Command::output`, `TcpStream::connect`, …), look up the targeted
/// function and fire when neither the file's `module_path` nor the
/// function symbol matches any pattern in `runtime_owner_paths`.
///
/// Severity: Warning (Fatal under `--agent-strict`). Softer than RW001's
/// always-Fatal posture: a stray blocking call in a non-runtime-owner
/// module is common-and-bad but not as structurally damaging as untracked
/// spawning. The full hot/request/async-context detection (Paradigm 14
/// proper) requires framework loaders that don't exist yet — RW002 is
/// the simpler, already-actionable shape.
///
/// Silent when `runtime_owner_paths` is empty (same opt-in posture as
/// the rest of RW). Functions whose file has no `module_path` are
/// skipped — we can't decide anything about them.
pub fn rw002(air: &AirWorkspace, section: &RwSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.runtime_owner_paths.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for fact in &air.facts {
        if fact.kind != FactKind::BlockingCall {
            continue;
        }
        let FactTarget::Function { symbol } = &fact.target else {
            continue;
        };
        let Some((module_path, fn_span)) = lookup_function(air, symbol) else {
            continue;
        };
        // Same module-path-or-function-symbol matching as RW001: inline
        // `mod tests {}` blocks live at a deeper symbol path than the
        // file, so a `*::tests::*` pattern would silently miss them if
        // we only checked the file.
        if section
            .runtime_owner_paths
            .iter()
            .any(|pat| matches_pattern(pat, module_path) || matches_pattern(pat, symbol))
        {
            continue;
        }
        out.push(rw002_diagnostic(fact, symbol, module_path, fn_span, mode));
    }
    out
}

fn rw002_diagnostic(
    fact: &AirFact,
    symbol: &str,
    module_path: &str,
    fn_span: AirSpan,
    mode: CheckMode,
) -> Diagnostic {
    let span = match &fact.target {
        FactTarget::Span(s) => s.clone(),
        FactTarget::Function { .. } | FactTarget::File { .. } => fn_span,
    };
    let evidence = fact.evidence.as_deref().unwrap_or("blocking call");
    let why_reasons = if fact.reasons.is_empty() {
        vec!["loader detected blocking-shaped call".to_string()]
    } else {
        fact.reasons.clone()
    };
    Diagnostic {
        rule_id: "RW002".to_string(),
        severity: mode.elevate(Severity::Warning),
        span,
        concept: None,
        message: format!(
            "blocking call `{evidence}` in module `{module_path}` \
             (function `{symbol}`) outside any declared runtime owner"
        ),
        why: {
            let mut w = vec![format!(
                "module `{module_path}` matches none of the \
                 `runtime_owner_paths` patterns"
            )];
            for r in why_reasons {
                w.push(r);
            }
            if let Some(ev) = fact.evidence.as_deref() {
                w.push(format!("evidence: `{ev}`"));
            }
            w.push(
                "blocking calls should be confined to runtime-owner \
                 modules so the runtime can budget them appropriately"
                    .to_string(),
            );
            w
        },
        suggested_fix: Some(format!(
            "move the blocking call to a runtime-owner module (a thread \
             pool, a worker, a blocking-allowed task) and call it through \
             a port; or, if `{module_path}` really is a legitimate \
             blocking owner, expand `paradigms.RW.runtime_owner_paths` in \
             `locus.lock` to include it"
        )),
    }
}

fn lookup_function<'a>(air: &'a AirWorkspace, symbol: &str) -> Option<(&'a str, AirSpan)> {
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                if let AirItem::Function(f) = item
                    && f.symbol == symbol
                {
                    let module = file.module_path.as_deref()?;
                    return Some((module, f.span.clone()));
                }
            }
        }
    }
    None
}

fn diagnostic_for(
    fact: &AirFact,
    symbol: &str,
    module_path: &str,
    fn_span: AirSpan,
    mode: CheckMode,
) -> Diagnostic {
    let span = match &fact.target {
        FactTarget::Span(s) => s.clone(),
        FactTarget::Function { .. } | FactTarget::File { .. } => fn_span,
    };
    let function_label = symbol;
    let why_reasons = if fact.reasons.is_empty() {
        vec!["loader detected spawn-shaped call".to_string()]
    } else {
        fact.reasons.clone()
    };
    Diagnostic {
        rule_id: "RW001".to_string(),
        severity: mode.elevate(Severity::Fatal),
        span,
        concept: None,
        message: format!(
            "spawn-shaped call in module `{module_path}` \
             (function `{function_label}`) outside any declared \
             runtime owner"
        ),
        why: {
            let mut w = vec![format!(
                "module `{module_path}` matches none of the \
                 `runtime_owner_paths` patterns"
            )];
            for r in why_reasons {
                w.push(r);
            }
            w.push(format!("enclosing function: `{function_label}`"));
            w
        },
        suggested_fix: Some(format!(
            "move the spawn into a runtime-owner module (job queue, \
             orchestrator, supervisor, or runtime entry point) and have \
             this code submit work to it through a port; or, if \
             `{module_path}` really is a legitimate runtime owner, expand \
             `paradigms.RW.runtime_owner_paths` in `locus.lock` to \
             include it"
        )),
    }
}

/// True when `module_path` matches any pattern in `runtime_owner_paths`.
/// Mirrors the matching used by RW001 but without the function-symbol
/// fallback — RW003/RW004 only have the file's `module_path` to work with.
fn module_is_runtime_owner(section: &RwSection, module_path: &str) -> bool {
    section
        .runtime_owner_paths
        .iter()
        .any(|pat| matches_pattern(pat, module_path))
}

/// True when *any* of the type-text fragment patterns matches `text`.
fn any_type_text_matches(patterns: &[String], text: &str) -> bool {
    patterns.iter().any(|p| type_text_matches(p, text))
}

/// RW003 — `Mutex` / `RwLock` (or similar runtime-state container) field
/// outside the runtime-ownership boundary.
///
/// For each `AirItem::Type` whose enclosing file's `module_path` is **not**
/// covered by `runtime_owner_paths`, fire when any field's `type_text`
/// matches `runtime_state_type_patterns`. The pattern syntax for these
/// fragments is intentionally minimal — see [`type_text_matches`].
///
/// Severity: Warning by default; Fatal under `--agent-strict`.
///
/// Silent when `runtime_owner_paths` is empty (same opt-in posture as the
/// rest of RW): without a declared owner there's no way to flag "outside
/// the owner" without hand-waving.
pub fn rw003(air: &AirWorkspace, section: &RwSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.runtime_owner_paths.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            if module_is_runtime_owner(section, module_path) {
                continue;
            }
            for item in &file.items {
                let AirItem::Type(t) = item else { continue };
                let Some((field_name, field_text, matched_pattern)) =
                    first_runtime_state_field(t, &section.runtime_state_type_patterns)
                else {
                    continue;
                };
                out.push(rw003_diagnostic(
                    t,
                    module_path,
                    field_name,
                    field_text,
                    matched_pattern,
                    mode,
                ));
            }
        }
    }
    out
}

/// Find the first field on `t` whose rendered type text matches one of the
/// runtime-state-type patterns. Returns `(field_name, field_type_text,
/// matched_pattern)` so the diagnostic can quote the actual offender.
fn first_runtime_state_field<'a>(
    t: &'a AirType,
    patterns: &'a [String],
) -> Option<(&'a str, &'a str, &'a str)> {
    for f in &t.fields {
        for pat in patterns {
            if type_text_matches(pat, &f.type_text) {
                return Some((f.name.as_str(), f.type_text.as_str(), pat.as_str()));
            }
        }
    }
    None
}

fn rw003_diagnostic(
    t: &AirType,
    module_path: &str,
    field_name: &str,
    field_text: &str,
    matched_pattern: &str,
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
        rule_id: "RW003".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: t.span.clone(),
        concept: None,
        message: format!(
            "type `{}` in module `{module_path}` holds a runtime-state field \
             `{field_name}: {field_text}` outside any declared runtime owner",
            t.symbol
        ),
        why: vec![
            format!("module `{module_path}` matches none of `runtime_owner_paths`"),
            format!(
                "field `{field_name}` has type `{field_text}` which matches \
                 runtime-state pattern `{matched_pattern}`"
            ),
        ],
        suggested_fix: Some(format!(
            "move `{}` (or the `{field_name}` field) into a runtime-owner \
             module — supervisors, runtime cores, worker pools — and have \
             this code talk to it through a port. If `{module_path}` is in \
             fact a legitimate runtime owner, expand \
             `paradigms.RW.runtime_owner_paths` in `locus.lock`. To loosen \
             type detection, edit \
             `paradigms.RW.runtime_state_type_patterns`.",
            t.symbol
        )),
    }
}

/// RW004 — `static`/`OnceCell`/`Lazy`/named-singleton type outside the
/// runtime-ownership boundary.
///
/// Narrower than RW003: fires only when the type **itself** looks like a
/// singleton wrapper. A type qualifies if either:
///
/// 1. its `name` matches one of `singleton_name_patterns` (e.g. `*Singleton`,
///    `*Globals`); or
/// 2. it is a single-field `Struct` whose sole field's `type_text` matches
///    `runtime_state_type_patterns` (in practice: `OnceCell<...>` /
///    `OnceLock<...>` / `Lazy<...>` patterns).
///
/// Reusing `runtime_state_type_patterns` keeps the inner-type vocabulary in
/// one place. Same severity / opt-in posture as RW003.
pub fn rw004(air: &AirWorkspace, section: &RwSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.runtime_owner_paths.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            if module_is_runtime_owner(section, module_path) {
                continue;
            }
            for item in &file.items {
                let AirItem::Type(t) = item else { continue };
                let by_name = any_type_text_matches(&section.singleton_name_patterns, &t.name);
                let by_shape = is_single_field_runtime_state_struct(t, section);
                if !(by_name || by_shape) {
                    continue;
                }
                out.push(rw004_diagnostic(t, module_path, by_name, by_shape, mode));
            }
        }
    }
    out
}

/// True when `t` is a single-field struct whose sole field's type text
/// matches one of the runtime-state-type fragment patterns.
fn is_single_field_runtime_state_struct(t: &AirType, section: &RwSection) -> bool {
    if t.kind != TypeKind::Struct || t.fields.len() != 1 {
        return false;
    }
    any_type_text_matches(&section.runtime_state_type_patterns, &t.fields[0].type_text)
}

fn rw004_diagnostic(
    t: &AirType,
    module_path: &str,
    by_name: bool,
    by_shape: bool,
    mode: CheckMode,
) -> Diagnostic {
    let mut why = vec![format!(
        "module `{module_path}` matches none of `runtime_owner_paths`"
    )];
    if by_name {
        why.push(format!(
            "type name `{}` matches one of `singleton_name_patterns`",
            t.name
        ));
    }
    if by_shape && let Some(f) = t.fields.first() {
        why.push(format!(
            "single-field struct whose field `{}: {}` matches \
             `runtime_state_type_patterns`",
            f.name, f.type_text
        ));
    }
    Diagnostic {
        rule_id: "RW004".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: t.span.clone(),
        concept: None,
        message: format!(
            "global-singleton-shaped type `{}` lives in `{module_path}`, \
             outside any declared runtime owner",
            t.symbol
        ),
        why,
        suggested_fix: Some(format!(
            "globals are runtime state; move `{}` into a runtime-owner \
             module and inject it where needed. If this *is* a legitimate \
             runtime-owner location, expand \
             `paradigms.RW.runtime_owner_paths` in `locus.lock`; to widen \
             or narrow detection, edit \
             `paradigms.RW.singleton_name_patterns` or \
             `paradigms.RW.runtime_state_type_patterns`.",
            t.symbol
        )),
    }
}

/// Collect the symbols of every function that has a `FactKind::HotPath`
/// fact targeting it. The markers loader emits these for any function the
/// user annotated with `// locus: fact hot_path`.
fn collect_hot_path_symbols(air: &AirWorkspace) -> HashSet<String> {
    let mut set = HashSet::new();
    for fact in &air.facts {
        if fact.kind != FactKind::HotPath {
            continue;
        }
        if let FactTarget::Function { symbol } = &fact.target {
            set.insert(symbol.clone());
        }
    }
    set
}

/// RW005 — blocking call inside a function the user marked as `hot_path`.
///
/// The user's `// locus: fact hot_path` annotation is what opts a function
/// into this rule: as soon as a function has BOTH a `FactKind::HotPath`
/// marker and a `FactKind::BlockingCall` fact, we fire. This is the
/// already-actionable subset of Paradigm 14's "blocking ops in
/// async/request/hot context" rule — the broader async/request detection
/// requires framework loaders that don't exist yet, but the hot-path
/// half is purely user-declarative and lights up today.
///
/// Severity: Fatal — blocking inside a hot loop / frame budget is
/// structural; it starves the runtime regardless of severity mode.
///
/// Not lockfile-gated: marker presence *is* the opt-in.
pub fn rw005(air: &AirWorkspace, mode: CheckMode) -> Vec<Diagnostic> {
    let hot = collect_hot_path_symbols(air);
    if hot.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for fact in &air.facts {
        if fact.kind != FactKind::BlockingCall {
            continue;
        }
        let FactTarget::Function { symbol } = &fact.target else {
            continue;
        };
        if !hot.contains(symbol) {
            continue;
        }
        let Some((module_path, fn_span)) = lookup_function(air, symbol) else {
            continue;
        };
        out.push(rw005_diagnostic(fact, symbol, module_path, fn_span, mode));
    }
    out
}

fn rw005_diagnostic(
    fact: &AirFact,
    symbol: &str,
    module_path: &str,
    fn_span: AirSpan,
    mode: CheckMode,
) -> Diagnostic {
    let span = match &fact.target {
        FactTarget::Span(s) => s.clone(),
        FactTarget::Function { .. } | FactTarget::File { .. } => fn_span,
    };
    let evidence = fact.evidence.as_deref().unwrap_or("blocking call");
    let why_reasons = if fact.reasons.is_empty() {
        vec!["loader detected blocking-shaped call".to_string()]
    } else {
        fact.reasons.clone()
    };
    Diagnostic {
        rule_id: "RW005".to_string(),
        severity: mode.elevate(Severity::Fatal),
        span,
        concept: None,
        message: format!(
            "hot-path function `{symbol}` performs blocking call \
             `{evidence}` — blocks the hot loop / frame budget"
        ),
        why: {
            let mut w = vec![format!(
                "function `{symbol}` carries `HotPath` marker (in module \
                 `{module_path}`)"
            )];
            for r in why_reasons {
                w.push(r);
            }
            if let Some(ev) = fact.evidence.as_deref() {
                w.push(format!("evidence: `{ev}`"));
            }
            w.push(
                "blocking calls in hot paths starve the runtime — they \
                 must be moved off-thread or replaced with non-blocking \
                 equivalents"
                    .to_string(),
            );
            w
        },
        suggested_fix: Some(format!(
            "move the blocking call out of `{symbol}`: spawn a one-off \
             worker (`std::thread::spawn`) or submit the work to a job \
             queue / thread pool from a runtime-owner module; or, if \
             you're in async, use the non-blocking equivalent (e.g. \
             `tokio::fs::read` instead of `std::fs::read`)"
        )),
    }
}

/// RW006 — spawn inside a function the user marked as `hot_path`.
///
/// Same shape as RW005 but for `FactKind::SpawnedWork` instead of
/// `BlockingCall`. Spawning per-iteration inside a hot loop creates
/// unbounded task pressure: per-frame `tokio::spawn` / `thread::spawn`
/// allocates, schedules, and tears down workers at the loop's rate.
///
/// Severity: Fatal — same structural posture as RW005.
///
/// Not lockfile-gated: the user's `// locus: fact hot_path` annotation is
/// what opts a function into this rule.
pub fn rw006(air: &AirWorkspace, mode: CheckMode) -> Vec<Diagnostic> {
    let hot = collect_hot_path_symbols(air);
    if hot.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for fact in &air.facts {
        if fact.kind != FactKind::SpawnedWork {
            continue;
        }
        let FactTarget::Function { symbol } = &fact.target else {
            continue;
        };
        if !hot.contains(symbol) {
            continue;
        }
        let Some((module_path, fn_span)) = lookup_function(air, symbol) else {
            continue;
        };
        out.push(rw006_diagnostic(fact, symbol, module_path, fn_span, mode));
    }
    out
}

fn rw006_diagnostic(
    fact: &AirFact,
    symbol: &str,
    module_path: &str,
    fn_span: AirSpan,
    mode: CheckMode,
) -> Diagnostic {
    let span = match &fact.target {
        FactTarget::Span(s) => s.clone(),
        FactTarget::Function { .. } | FactTarget::File { .. } => fn_span,
    };
    let evidence = fact.evidence.as_deref().unwrap_or("spawn");
    let why_reasons = if fact.reasons.is_empty() {
        vec!["loader detected spawn-shaped call".to_string()]
    } else {
        fact.reasons.clone()
    };
    Diagnostic {
        rule_id: "RW006".to_string(),
        severity: mode.elevate(Severity::Fatal),
        span,
        concept: None,
        message: format!(
            "hot-path function `{symbol}` spawns work `{evidence}` \
             — uncontrolled per-iteration spawning"
        ),
        why: {
            let mut w = vec![format!(
                "function `{symbol}` carries `HotPath` marker (in module \
                 `{module_path}`)"
            )];
            for r in why_reasons {
                w.push(r);
            }
            if let Some(ev) = fact.evidence.as_deref() {
                w.push(format!("evidence: `{ev}`"));
            }
            w.push(
                "spawning inside a hot loop creates unbounded task \
                 pressure — work should be pre-spawned and submitted via \
                 a port, or reused via a thread pool"
                    .to_string(),
            );
            w
        },
        suggested_fix: Some(format!(
            "pre-spawn the worker in a runtime-owner module and submit \
             work from `{symbol}` via a channel (or other port) instead \
             of spawning per iteration"
        )),
    }
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
