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
pub fn ob001(air: &AirWorkspace, section: &ObSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.observer_paths.is_empty() {
        return Vec::new();
    }
    let forbidden = section.effective_forbidden_log_targets();
    if forbidden.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for fact in &air.facts {
        if fact.kind != FactKind::Logging {
            continue;
        }
        // OB001 only flags loggers whose evidence (the callee path) matches
        // a forbidden pattern. Loaders with no evidence (aggregate facts)
        // are skipped — there's nothing to match against.
        let Some(evidence) = fact.evidence.as_deref() else {
            continue;
        };
        let Some(forbidden_pattern) = forbidden.iter().find(|pat| matches_pattern(pat, evidence))
        else {
            continue;
        };
        let FactTarget::Function { symbol } = &fact.target else {
            continue;
        };
        let Some((module_path, fn_span)) = lookup_function(air, symbol) else {
            continue;
        };
        if section
            .observer_paths
            .iter()
            .any(|pat| matches_pattern(pat, module_path))
        {
            continue;
        }
        out.push(diagnostic_for(
            fact,
            symbol,
            module_path,
            fn_span,
            evidence,
            forbidden_pattern,
            mode,
        ));
    }
    out
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
    let function_label = symbol;
    let why_reasons = if fact.reasons.is_empty() {
        vec!["loader detected logging primitive".to_string()]
    } else {
        fact.reasons.clone()
    };
    Diagnostic {
        rule_id: "OB001".to_string(),
        severity: mode.elevate(Severity::Warning),
        span,
        concept: None,
        message: format!(
            "logging call `{evidence}` in `{module_path}` (fn `{function_label}`) — \
             matches `paradigms.OB.forbidden_log_targets` pattern `{forbidden_pattern}`"
        ),
        why: {
            let mut w = vec![
                format!(
                    "module `{module_path}` does not match any \
                     `paradigms.OB.observer_paths` pattern"
                ),
                format!("evidence `{evidence}` matches forbidden pattern `{forbidden_pattern}`"),
            ];
            for r in why_reasons {
                w.push(r);
            }
            w.push(format!("enclosing function: `{function_label}`"));
            w
        },
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
pub fn ob004(air: &AirWorkspace, _section: &ObSection, mode: CheckMode) -> Vec<Diagnostic> {
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

    let mut out = Vec::new();
    for symbol in missing {
        let span = match lookup_function(air, symbol) {
            Some((_, fn_span)) => fn_span,
            None => AirSpan::new("<unknown>", 0, 0),
        };
        out.push(Diagnostic {
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
        });
    }
    out
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
mod tests {
    use super::super::lockfile_schema::{
        default_event_macro_patterns, default_forbidden_log_targets, default_metric_macro_patterns,
    };
    use super::*;
    use locus_air::{
        AIR_SCHEMA_VERSION, AirFile, AirFunction, AirPackage, AirSpan, AirWorkspace, Visibility,
    };

    fn func(symbol: &str, file: &str, line: u32) -> AirItem {
        AirItem::Function(AirFunction {
            name: symbol.rsplit("::").next().unwrap_or(symbol).into(),
            symbol: symbol.into(),
            visibility: Visibility::Public,
            params: Vec::new(),
            return_type: None,
            span: AirSpan::new(file, line, line + 5),
            line_count: 6,
            decorators: Vec::new(),
            symbol_segments: Vec::new(),
            doc: None,
        })
    }

    fn log_fact(symbol: &str, evidence: &str, reason: &str) -> AirFact {
        AirFact {
            kind: FactKind::Logging,
            target: FactTarget::Function {
                symbol: symbol.into(),
            },
            source: "test".into(),
            confidence: 1.0,
            reasons: vec![reason.into()],
            evidence: Some(evidence.into()),
        }
    }

    fn air_with(module: Option<&str>, items: Vec<AirItem>, facts: Vec<AirFact>) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "t.rs".into(),
                    module_path: module.map(|m| m.into()),
                    items,
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                }],
            }],
            facts,
        }
    }

    /// Onboarded baseline: a single observer pattern that doesn't match any
    /// of the test fixture's `x::domain::*` modules. OB stays silent until
    /// `observer_paths` is populated (mirrors every other lockfile-driven
    /// rule), so tests need at least one observer pattern declared.
    fn default_section() -> ObSection {
        ObSection {
            observer_paths: vec!["x::cli::*".into()],
            forbidden_log_targets: default_forbidden_log_targets(),
            ..ObSection::default()
        }
    }

    #[test]
    fn ob001_fires_on_raw_println_in_non_observer_file() {
        let air = air_with(
            Some("x::domain::user"),
            vec![func("x::domain::user::greet", "t.rs", 5)],
            vec![log_fact(
                "x::domain::user::greet",
                "println",
                "`println!` is a raw print/dbg macro",
            )],
        );
        let diags = ob001(&air, &default_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "OB001");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(
            diags[0].message.contains("x::domain::user"),
            "expected module_path in message; got {}",
            diags[0].message
        );
        assert!(
            diags[0].message.contains("greet"),
            "expected function in message; got {}",
            diags[0].message
        );
        assert!(
            diags[0].why.iter().any(|w| w.contains("observer_paths")),
            "expected observer_paths reasoning in why; got {:?}",
            diags[0].why
        );
        assert!(
            diags[0].why.iter().any(|w| w.contains("println")),
            "expected loader reason in why; got {:?}",
            diags[0].why
        );
    }

    #[test]
    fn ob001_quiet_on_non_forbidden_log_targets() {
        // `tracing::info` doesn't match any forbidden_log_targets pattern,
        // so OB001 stays silent — the canonical structured facility.
        let air = air_with(
            Some("x::domain::user"),
            vec![func("x::domain::user::greet", "t.rs", 5)],
            vec![log_fact(
                "x::domain::user::greet",
                "tracing::info",
                "`tracing::info!` is a structured log macro",
            )],
        );
        assert!(ob001(&air, &default_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn ob001_quiet_on_raw_log_in_observer_path_matching_file() {
        let air = air_with(
            Some("x::cli::main"),
            vec![func("x::cli::main::run", "t.rs", 5)],
            vec![log_fact("x::cli::main::run", "println", "println")],
        );
        let section = ObSection {
            observer_paths: vec!["x::cli::*".into()],
            forbidden_log_targets: default_forbidden_log_targets(),
            ..ObSection::default()
        };
        assert!(ob001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ob001_skips_facts_without_module_path() {
        // Function exists in AIR but its file has no module path — the
        // function lookup misses the module check, the rule stays silent.
        let air = air_with(
            None,
            vec![func("anon::fn", "t.rs", 5)],
            vec![log_fact("anon::fn", "println", "println")],
        );
        assert!(ob001(&air, &default_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn ob001_agent_strict_elevates_warning_to_fatal() {
        let air = air_with(
            Some("x::domain::user"),
            vec![func("x::domain::user::greet", "t.rs", 5)],
            vec![log_fact("x::domain::user::greet", "println", "println")],
        );
        let diags = ob001(&air, &default_section(), CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn ob001_multiple_raw_log_facts_produce_one_diagnostic_each() {
        let air = air_with(
            Some("x::domain::user"),
            vec![
                func("x::domain::user::greet", "t.rs", 5),
                func("x::domain::user::oops", "t.rs", 12),
                func("x::domain::user::ok", "t.rs", 14),
            ],
            vec![
                log_fact("x::domain::user::greet", "println", "println"),
                log_fact("x::domain::user::greet", "dbg", "dbg"),
                log_fact("x::domain::user::oops", "eprintln", "eprintln"),
                // `tracing::info` is the canonical facility — never flagged
                // because it doesn't match any forbidden_log_targets pattern.
                log_fact("x::domain::user::ok", "tracing::info", "tracing::info"),
            ],
        );
        let diags = ob001(&air, &default_section(), CheckMode::Human);
        assert_eq!(diags.len(), 3);
    }

    #[test]
    fn ob001_silent_when_observer_paths_empty() {
        let air = air_with(
            Some("x::domain::user"),
            vec![func("x::domain::user::greet", "t.rs", 5)],
            vec![log_fact("x::domain::user::greet", "println", "println")],
        );
        let section = ObSection {
            observer_paths: Vec::new(),
            forbidden_log_targets: default_forbidden_log_targets(),
            ..ObSection::default()
        };
        assert!(ob001(&air, &section, CheckMode::Human).is_empty());
    }

    fn macro_call(callee: &str, function: Option<&str>, line: u32) -> AirItem {
        AirItem::CallSite(AirCallSite {
            callee: callee.into(),
            kind: CallKind::Meta,
            function: function.map(|s| s.to_string()),
            span: AirSpan::new("t.rs", line, line),
        })
    }

    fn fn_call(callee: &str, function: Option<&str>, line: u32) -> AirItem {
        AirItem::CallSite(AirCallSite {
            callee: callee.into(),
            kind: CallKind::Function,
            function: function.map(|s| s.to_string()),
            span: AirSpan::new("t.rs", line, line),
        })
    }

    fn air_with_calls(module: &str, items: Vec<AirItem>) -> AirWorkspace {
        air_with(Some(module), items, Vec::new())
    }

    // ─── OB002 ───────────────────────────────────────────────────────────

    #[test]
    fn ob002_fires_on_metrics_macro_outside_owner_path() {
        let air = air_with_calls(
            "x::domain::user",
            vec![macro_call(
                "metrics::counter",
                Some("x::domain::user::tick"),
                7,
            )],
        );
        let section = ObSection {
            metric_owner_paths: vec!["x::observability::*".into()],
            metric_macro_patterns: default_metric_macro_patterns(),
            ..ObSection::default()
        };
        let diags = ob002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "OB002");
        assert!(diags[0].message.contains("metrics::counter"));
        assert!(diags[0].message.contains("x::domain::user"));
    }

    #[test]
    fn ob002_quiet_inside_metric_owner_path() {
        let air = air_with_calls(
            "x::observability::metrics",
            vec![macro_call(
                "metrics::counter",
                Some("x::observability::metrics::bump"),
                3,
            )],
        );
        let section = ObSection {
            metric_owner_paths: vec!["x::observability::*".into()],
            metric_macro_patterns: default_metric_macro_patterns(),
            ..ObSection::default()
        };
        assert!(ob002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ob002_silent_when_metric_owner_paths_empty() {
        let air = air_with_calls(
            "x::domain::user",
            vec![macro_call(
                "metrics::counter",
                Some("x::domain::user::tick"),
                7,
            )],
        );
        let section = ObSection::default();
        assert!(ob002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ob002_skips_function_calls() {
        // Function-shaped calls aren't macro emissions even if their text
        // matches a metric macro pattern.
        let air = air_with_calls(
            "x::domain::user",
            vec![fn_call(
                "metrics::counter",
                Some("x::domain::user::tick"),
                7,
            )],
        );
        let section = ObSection {
            metric_owner_paths: vec!["x::observability::*".into()],
            metric_macro_patterns: default_metric_macro_patterns(),
            ..ObSection::default()
        };
        assert!(ob002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ob002_quiet_when_callee_does_not_match_pattern() {
        let air = air_with_calls(
            "x::domain::user",
            vec![macro_call("println", Some("x::domain::user::tick"), 7)],
        );
        let section = ObSection {
            metric_owner_paths: vec!["x::observability::*".into()],
            metric_macro_patterns: default_metric_macro_patterns(),
            ..ObSection::default()
        };
        assert!(ob002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ob002_agent_strict_elevates_to_fatal() {
        let air = air_with_calls(
            "x::domain::user",
            vec![macro_call(
                "metrics::histogram",
                Some("x::domain::user::tick"),
                7,
            )],
        );
        let section = ObSection {
            metric_owner_paths: vec!["x::observability::*".into()],
            metric_macro_patterns: default_metric_macro_patterns(),
            ..ObSection::default()
        };
        let diags = ob002(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    // ─── OB003 ───────────────────────────────────────────────────────────

    #[test]
    fn ob003_fires_on_event_macro_outside_owner_path() {
        let air = air_with_calls(
            "x::domain::user",
            vec![macro_call("event", Some("x::domain::user::publish"), 7)],
        );
        let section = ObSection {
            event_owner_paths: vec!["x::observability::events::*".into()],
            event_macro_patterns: default_event_macro_patterns(),
            ..ObSection::default()
        };
        let diags = ob003(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "OB003");
        assert!(diags[0].message.contains("event"));
    }

    #[test]
    fn ob003_quiet_inside_event_owner_path() {
        let air = air_with_calls(
            "x::observability::events::user",
            vec![macro_call(
                "publish",
                Some("x::observability::events::user::send"),
                3,
            )],
        );
        let section = ObSection {
            event_owner_paths: vec!["x::observability::events::*".into()],
            event_macro_patterns: default_event_macro_patterns(),
            ..ObSection::default()
        };
        assert!(ob003(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ob003_silent_when_event_owner_paths_empty() {
        let air = air_with_calls(
            "x::domain::user",
            vec![macro_call("event", Some("x::domain::user::publish"), 7)],
        );
        let section = ObSection::default();
        assert!(ob003(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ob003_matches_tracing_event_pattern() {
        let air = air_with_calls(
            "x::domain::user",
            vec![macro_call(
                "tracing::event",
                Some("x::domain::user::span"),
                9,
            )],
        );
        let section = ObSection {
            event_owner_paths: vec!["x::observability::events::*".into()],
            event_macro_patterns: default_event_macro_patterns(),
            ..ObSection::default()
        };
        let diags = ob003(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("tracing::event"));
    }

    #[test]
    fn ob003_agent_strict_elevates_to_fatal() {
        let air = air_with_calls(
            "x::domain::user",
            vec![macro_call("emit", Some("x::domain::user::publish"), 7)],
        );
        let section = ObSection {
            event_owner_paths: vec!["x::observability::events::*".into()],
            event_macro_patterns: default_event_macro_patterns(),
            ..ObSection::default()
        };
        let diags = ob003(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    // ─── OB004 ───────────────────────────────────────────────────────────

    fn boundary_entry_marker_fact(symbol: &str) -> AirFact {
        AirFact {
            kind: FactKind::BoundaryEntry,
            target: FactTarget::Function {
                symbol: symbol.into(),
            },
            source: "markers".into(),
            confidence: 1.0,
            reasons: vec!["// locus: fact boundary_entry".into()],
            evidence: None,
        }
    }

    fn logging_fact(symbol: &str, callee: &str) -> AirFact {
        AirFact {
            kind: FactKind::Logging,
            target: FactTarget::Function {
                symbol: symbol.into(),
            },
            source: "std-rt".into(),
            confidence: 0.9,
            reasons: vec![format!("calls `{callee}!`")],
            evidence: Some(callee.into()),
        }
    }

    #[test]
    fn ob004_fires_when_boundary_entry_has_no_logging() {
        let air = air_with(
            Some("x::api::http"),
            vec![func("x::api::http::handle", "t.rs", 5)],
            vec![boundary_entry_marker_fact("x::api::http::handle")],
        );
        let diags = ob004(&air, &ObSection::default(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "OB004");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(
            diags[0].message.contains("x::api::http::handle"),
            "expected symbol in message; got {}",
            diags[0].message
        );
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("BoundaryEntry") && w.contains("marker")),
            "expected BoundaryEntry marker reason in why; got {:?}",
            diags[0].why
        );
        assert!(
            diags[0].why.iter().any(|w| w.contains("no `Logging` fact")),
            "expected logging-absence reason in why; got {:?}",
            diags[0].why
        );
        assert_eq!(diags[0].span.file, "t.rs");
        assert_eq!(diags[0].span.line_start, 5);
    }

    #[test]
    fn ob004_quiet_when_boundary_entry_has_logging() {
        let air = air_with(
            Some("x::api::http"),
            vec![func("x::api::http::handle", "t.rs", 5)],
            vec![
                boundary_entry_marker_fact("x::api::http::handle"),
                logging_fact("x::api::http::handle", "tracing::info"),
            ],
        );
        assert!(ob004(&air, &ObSection::default(), CheckMode::Human).is_empty());
    }

    #[test]
    fn ob004_quiet_when_only_logging_no_boundary_entry() {
        let air = air_with(
            Some("x::domain::user"),
            vec![func("x::domain::user::greet", "t.rs", 5)],
            vec![logging_fact("x::domain::user::greet", "tracing::info")],
        );
        assert!(ob004(&air, &ObSection::default(), CheckMode::Human).is_empty());
    }

    #[test]
    fn ob004_silent_when_no_boundary_entry_facts_in_workspace() {
        let air = air_with(
            Some("x::domain::user"),
            vec![func("x::domain::user::greet", "t.rs", 5)],
            Vec::new(),
        );
        assert!(ob004(&air, &ObSection::default(), CheckMode::Human).is_empty());
    }

    #[test]
    fn ob004_agent_strict_elevates_warning_to_fatal() {
        let air = air_with(
            Some("x::api::http"),
            vec![func("x::api::http::handle", "t.rs", 5)],
            vec![boundary_entry_marker_fact("x::api::http::handle")],
        );
        let diags = ob004(&air, &ObSection::default(), CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn ob004_multiple_boundary_entries_without_logging_produce_one_each() {
        let air = air_with(
            Some("x::api::http"),
            vec![
                func("x::api::http::create", "t.rs", 5),
                func("x::api::http::update", "t.rs", 12),
                func("x::api::http::delete", "t.rs", 19),
                func("x::api::http::read", "t.rs", 26),
            ],
            vec![
                boundary_entry_marker_fact("x::api::http::create"),
                boundary_entry_marker_fact("x::api::http::update"),
                boundary_entry_marker_fact("x::api::http::delete"),
                // `read` is a boundary entry that DOES log — must be quiet.
                boundary_entry_marker_fact("x::api::http::read"),
                logging_fact("x::api::http::read", "tracing::info"),
            ],
        );
        let diags = ob004(&air, &ObSection::default(), CheckMode::Human);
        assert_eq!(diags.len(), 3);
        let symbols: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        assert!(symbols.iter().any(|m| m.contains("create")));
        assert!(symbols.iter().any(|m| m.contains("update")));
        assert!(symbols.iter().any(|m| m.contains("delete")));
        assert!(
            !symbols.iter().any(|m| m.contains("::read`")),
            "boundary entry with logging should not be flagged; got {:?}",
            symbols
        );
    }
}
