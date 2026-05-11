//! OB rules.
//!
//! Implemented:
//! - [`ob001`]: a `Logging` fact whose evidence (the callee path) matches
//!   `forbidden_log_targets`, fired in a non-observer file. Default
//!   forbidden set is the bare print/dbg macro family — `println!`,
//!   `eprintln!`, `print!`, `eprint!`, `dbg!` — but the lockfile decides.
//!   The raw-vs-structured distinction is a *user policy* (encoded as
//!   patterns), not a fact taxonomy.
//! - [`ob002`]: a metric-emission macro (`metrics::counter!`,
//!   `metrics::histogram!`, `metrics::gauge!` by default) called from a
//!   file outside `metric_owner_paths`. The signal is "metrics emission
//!   landing outside the accepted owner module."
//! - [`ob003`]: same shape for event-emission macros — `event!`, `emit!`,
//!   `publish!`, `tracing::event!` by default — gated by
//!   `event_owner_paths`.
//! - [`ob004`]: a function symbol carries a `FactKind::BoundaryEntry`
//!   marker but no `FactKind::Logging` fact targets the same symbol —
//!   silent boundary entries make outage triage and request tracing
//!   impossible. Opt-in lives in the `// locus: fact boundary_entry`
//!   source hint; no lockfile field is needed.

use std::collections::HashSet;

use locus_air::{
    AirCallSite, AirFact, AirItem, AirSpan, AirWorkspace, CallKind, FactKind, FactTarget,
};

use super::lockfile_schema::{ObSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// OB001 — forbidden logging primitive in a non-observer file.
///
/// For every `FactKind::Logging` fact, check whether the fact's
/// [`AirFact::evidence`] matches any pattern in `forbidden_log_targets`.
/// If yes, look up the targeted function's file and fire when the file's
/// `module_path` does NOT match any pattern in `observer_paths`.
///
/// Severity: Warning by default; Fatal under `--agent-strict`. The spec
/// frames observability-ownership as heuristic — `println!` in scratch
/// code shouldn't break CI by default, but agent-introduced raw prints
/// in domain code should be caught aggressively.
///
/// Silent until the user populates `observer_paths`. Same lockfile-driven
/// posture as DG/MO/UT/CR/CX/... — the user declares observer modules
/// before OB001 starts firing. The default `forbidden_log_targets` (the
/// print/dbg family) is non-empty, so once `observer_paths` gets a single
/// entry the rule starts working without further configuration.
/// Try to resolve one OB001-candidate fact into a diagnostic.
/// Returns `Some(Diagnostic)` when the fact fires, `None` otherwise.
fn ob001_check_fact(
    fact: &locus_air::AirFact,
    air: &AirWorkspace,
    forbidden: &[String],
    section: &ObSection,
    mode: CheckMode,
) -> Option<Diagnostic> {
    if fact.kind != FactKind::Logging {
        return None;
    }
    let evidence = fact.evidence.as_deref()?;
    let forbidden_pattern = forbidden
        .iter()
        .find(|pat| matches_pattern(pat, evidence))?;
    let FactTarget::Function { symbol } = &fact.target else {
        return None;
    };
    let (module_path, fn_span) = lookup_function(air, symbol)?;
    if section
        .observer_paths
        .iter()
        .any(|pat| matches_pattern(pat, module_path))
    {
        return None;
    }
    Some(diagnostic_for(
        fact,
        symbol,
        module_path,
        fn_span,
        evidence,
        forbidden_pattern,
        mode,
    ))
}

pub fn ob001(air: &AirWorkspace, section: &ObSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.observer_paths.is_empty() {
        return Vec::new();
    }
    let forbidden = section.effective_forbidden_log_targets();
    if forbidden.is_empty() {
        return Vec::new();
    }
    air.facts
        .iter()
        .filter_map(|fact| ob001_check_fact(fact, air, forbidden, section, mode))
        .collect()
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

fn ob001_why(
    module_path: &str,
    evidence: &str,
    forbidden_pattern: &str,
    function_label: &str,
    fact: &AirFact,
) -> Vec<String> {
    let mut w = vec![
        format!("module `{module_path}` does not match any `paradigms.OB.observer_paths` pattern"),
        format!("evidence `{evidence}` matches forbidden pattern `{forbidden_pattern}`"),
    ];
    if fact.reasons.is_empty() {
        w.push("loader detected logging primitive".to_string());
    } else {
        w.extend(fact.reasons.iter().cloned());
    }
    w.push(format!("enclosing function: `{function_label}`"));
    w
}

#[allow(clippy::too_many_arguments)]
fn diagnostic_for(
    fact: &AirFact,
    symbol: &str,
    module_path: &str,
    fn_span: AirSpan,
    evidence: &str,
    forbidden_pattern: &str,
    mode: CheckMode,
) -> Diagnostic {
    let span = match &fact.target {
        FactTarget::Span(s) => s.clone(),
        FactTarget::Function { .. } | FactTarget::File { .. } => fn_span,
    };
    Diagnostic {
        rule_id: "OB001".to_string(),
        severity: mode.elevate(Severity::Warning),
        span,
        concept: None,
        message: format!(
            "logging call `{evidence}` in `{module_path}` (fn `{symbol}`) — \
             matches `paradigms.OB.forbidden_log_targets` pattern `{forbidden_pattern}`"
        ),
        why: ob001_why(module_path, evidence, forbidden_pattern, symbol, fact),
        suggested_fix: Some(format!(
            "route this through the project's structured logging facility \
             (e.g. `tracing::info!` / `log::warn!` with accepted spans and \
             fields), or, if `{module_path}` legitimately owns user-facing \
             or test output, accept it via `paradigms.OB.observer_paths`. \
             To allow this specific target everywhere, remove `{forbidden_pattern}` \
             from `paradigms.OB.forbidden_log_targets`."
        )),
    }
}

/// OB002 — metric emission outside the accepted owner module.
///
/// For every `AirItem::CallSite` of `CallKind::Meta` whose `callee` matches
/// any pattern in `metric_macro_patterns`, fire when the enclosing file's
/// `module_path` does NOT match any pattern in `metric_owner_paths`. The
/// shape mirrors OB001 but skips the fact-tier — there's no normalized
/// `MetricEmission` fact yet, so we read call-sites directly.
///
/// Severity: Warning by default; Fatal under `--agent-strict`.
///
/// Silent until `metric_owner_paths` is populated — same lockfile-driven
/// posture as OB001 / FL002 / DG001 / etc.
pub fn ob002(air: &AirWorkspace, section: &ObSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.metric_owner_paths.is_empty() || section.metric_macro_patterns.is_empty() {
        return Vec::new();
    }
    macro_emission_diagnostics(
        air,
        &section.metric_macro_patterns,
        &section.metric_owner_paths,
        "OB002",
        "metric emission",
        "paradigms.OB.metric_macro_patterns",
        "paradigms.OB.metric_owner_paths",
        mode,
    )
}

/// OB003 — event emission outside the accepted owner module.
///
/// Same shape as [`ob002`] but for event-emission macros. Defaults cover
/// the bare `event!` / `emit!` / `publish!` macros plus `tracing::event!`.
///
/// Silent until `event_owner_paths` is populated.
pub fn ob003(air: &AirWorkspace, section: &ObSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.event_owner_paths.is_empty() || section.event_macro_patterns.is_empty() {
        return Vec::new();
    }
    macro_emission_diagnostics(
        air,
        &section.event_macro_patterns,
        &section.event_owner_paths,
        "OB003",
        "event emission",
        "paradigms.OB.event_macro_patterns",
        "paradigms.OB.event_owner_paths",
        mode,
    )
}

/// OB004 — boundary entry without observability.
///
/// Cross-references `FactKind::BoundaryEntry` markers (emitted by the
/// `markers` loader from `// locus: fact boundary_entry` source hints)
/// with `FactKind::Logging` facts on the same function symbol. Every
/// boundary entry — the public surface where data crosses into the
/// system — should emit at least one log line, span, metric, or event
/// so failure / success / latency is traceable.
///
/// Opt-in is the user's act of placing the `boundary_entry` marker;
/// the rule needs no lockfile field. If no boundary-entry markers
/// exist anywhere in the workspace, OB004 is silent.
///
/// Severity: Warning by default; Fatal under `--agent-strict`.
/// Collect boundary-entry symbols and logged symbols from workspace facts.
fn ob004_collect_symbols(air: &AirWorkspace) -> (HashSet<&str>, HashSet<&str>) {
    let mut boundary_entries: HashSet<&str> = HashSet::new();
    let mut logged: HashSet<&str> = HashSet::new();
    for fact in &air.facts {
        let FactTarget::Function { symbol } = &fact.target else {
            continue;
        };
        match fact.kind {
            FactKind::BoundaryEntry => {
                boundary_entries.insert(symbol.as_str());
            }
            FactKind::Logging => {
                logged.insert(symbol.as_str());
            }
            _ => {}
        }
    }
    (boundary_entries, logged)
}

pub fn ob004(air: &AirWorkspace, _section: &ObSection, mode: CheckMode) -> Vec<Diagnostic> {
    let (boundary_entries, logged) = ob004_collect_symbols(air);
    if boundary_entries.is_empty() {
        return Vec::new();
    }
    let mut missing: Vec<&str> = boundary_entries
        .iter()
        .filter(|sym| !logged.contains(*sym))
        .copied()
        .collect();
    // Stable ordering so multiple-diagnostic tests (and human output)
    // don't depend on HashSet iteration order.
    missing.sort_unstable();
    missing
        .into_iter()
        .map(|symbol| {
            let span = lookup_function(air, symbol)
                .map(|(_, s)| s)
                .unwrap_or_else(|| AirSpan::new("<unknown>", 0, 0));
            ob004_diagnostic(symbol, span, mode)
        })
        .collect()
}

fn ob004_diagnostic(symbol: &str, span: AirSpan, mode: CheckMode) -> Diagnostic {
    Diagnostic {
        rule_id: "OB004".to_string(),
        severity: mode.elevate(Severity::Warning),
        span,
        concept: None,
        message: format!(
            "boundary entry function `{symbol}` has no observability — \
             every entry should emit at least one logging / metric / \
             event call"
        ),
        why: vec![
            format!("function `{symbol}` carries `BoundaryEntry` marker"),
            format!("no `Logging` fact targets `{symbol}`"),
            "boundary entries are the audit / debug surface — silent \
             entries make outage triage and request tracing impossible"
                .to_string(),
        ],
        suggested_fix: Some(format!(
            "emit at least one structured log line at the entry of \
             `{symbol}` (e.g. `tracing::info!(\"entering boundary\", \
             request_id = %id)`), or a metric counter increment, or a \
             span. The `paradigms.OB` lockfile section enumerates what \
             counts as logging in this codebase."
        )),
    }
}

/// Check a single file's call-sites for macro-emission rule violations.
#[allow(clippy::too_many_arguments)]
fn macro_emission_check_file(
    file: &locus_air::AirFile,
    module_path: &str,
    macro_patterns: &[String],
    rule_id: &str,
    kind_label: &str,
    macro_lockfile_path: &str,
    owner_lockfile_path: &str,
    mode: CheckMode,
    out: &mut Vec<Diagnostic>,
) {
    for item in &file.items {
        let AirItem::CallSite(cs) = item else {
            continue;
        };
        if cs.kind != CallKind::Meta {
            continue;
        }
        let Some(matched_pattern) = macro_patterns
            .iter()
            .find(|pat| matches_pattern(pat, &cs.callee))
        else {
            continue;
        };
        out.push(diagnostic_for_macro_emission(
            cs,
            module_path,
            matched_pattern,
            rule_id,
            kind_label,
            macro_lockfile_path,
            owner_lockfile_path,
            mode,
        ));
    }
}

#[allow(clippy::too_many_arguments)]
fn macro_emission_diagnostics(
    air: &AirWorkspace,
    macro_patterns: &[String],
    owner_paths: &[String],
    rule_id: &str,
    kind_label: &str,
    macro_lockfile_path: &str,
    owner_lockfile_path: &str,
    mode: CheckMode,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            if owner_paths
                .iter()
                .any(|pat| matches_pattern(pat, module_path))
            {
                continue;
            }
            macro_emission_check_file(
                file,
                module_path,
                macro_patterns,
                rule_id,
                kind_label,
                macro_lockfile_path,
                owner_lockfile_path,
                mode,
                &mut out,
            );
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn diagnostic_for_macro_emission(
    cs: &AirCallSite,
    module_path: &str,
    matched_pattern: &str,
    rule_id: &str,
    kind_label: &str,
    macro_lockfile_path: &str,
    owner_lockfile_path: &str,
    mode: CheckMode,
) -> Diagnostic {
    let function_label = cs
        .function
        .as_deref()
        .unwrap_or("<unknown enclosing function>");
    Diagnostic {
        rule_id: rule_id.to_string(),
        severity: mode.elevate(Severity::Warning),
        span: cs.span.clone(),
        concept: None,
        message: format!(
            "{kind_label} `{}!` in `{module_path}` (fn `{function_label}`) — \
             matches `{macro_lockfile_path}` pattern `{matched_pattern}` \
             but module isn't in `{owner_lockfile_path}`",
            cs.callee,
        ),
        why: vec![
            format!("callee `{}!` (CallKind::Meta)", cs.callee),
            format!("matches `{macro_lockfile_path}` pattern `{matched_pattern}`"),
            format!("module `{module_path}` does not match any `{owner_lockfile_path}` pattern"),
            format!("enclosing function: `{function_label}`"),
        ],
        suggested_fix: Some(format!(
            "route this {kind_label} through the accepted owner module \
             (e.g. an observability facade in `{owner_lockfile_path}`), or, \
             if `{module_path}` is the legitimate owner, add it to \
             `{owner_lockfile_path}` in `locus.lock`. To stop treating \
             `{matched_pattern}` as a {kind_label} site, remove it from \
             `{macro_lockfile_path}`."
        )),
    }
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod rules_tests;
