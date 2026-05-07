//! OB rules.
//!
//! Implemented:
//! - [`ob001`]: raw `println!` / `dbg!` (and equivalents) outside test,
//!   example, or other observer-declared modules. The "agent stitched in
//!   ad-hoc logs while patching" anti-pattern: raw print/debug macros bypass
//!   any structured logging facility, leak to stdout/stderr in production,
//!   and signal that observability isn't owned. Structured facilities like
//!   `tracing::info!` / `log::warn!` are intentionally NOT flagged.

use locus_air::{AirFact, AirItem, AirSpan, AirWorkspace, FactKind, FactTarget};

use super::lockfile_schema::{ObSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// OB001 — raw print/dbg in non-test, non-observer code.
///
/// For every `FactKind::LogsRaw` fact produced by a loader, look up the
/// targeted function's file and fire when the file's `module_path` does
/// NOT match any pattern in `observer_paths`. `LogsStructured` is NEVER
/// flagged — structured logging is the canonical path.
///
/// Severity: Warning by default; Fatal under `--agent-strict`. The spec is
/// explicit that observability-ownership violations are heuristic — a stray
/// `println!` in scratch code shouldn't take CI down by default, but
/// agent-introduced raw prints in domain code should be caught aggressively.
///
/// Silent until the user populates `observer_paths`. Same lockfile-driven
/// posture as DG/MO/UT/CR/CX/... — the user declares which modules are
/// observers before OB001 starts firing. The (currently vestigial)
/// `forbidden_log_targets` field stays in the lockfile schema for future
/// per-target customisation but is unused by OB001; the rule fires on
/// every `LogsRaw` fact in non-observer files.
pub fn ob001(air: &AirWorkspace, section: &ObSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.observer_paths.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for fact in &air.facts {
        if fact.kind != FactKind::LogsRaw {
            continue;
        }
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
        out.push(diagnostic_for(fact, symbol, module_path, fn_span, mode));
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
        vec!["loader detected raw log macro".to_string()]
    } else {
        fact.reasons.clone()
    };
    Diagnostic {
        rule_id: "OB001".to_string(),
        severity: mode.elevate(Severity::Warning),
        span,
        concept: None,
        message: format!(
            "raw log call in `{module_path}` (fn `{function_label}`) — \
             bypasses structured logging"
        ),
        why: {
            let mut w = vec![format!(
                "module `{module_path}` does not match any \
                 `paradigms.OB.observer_paths` pattern"
            )];
            for r in why_reasons {
                w.push(r);
            }
            w.push(format!("enclosing function: `{function_label}`"));
            w
        },
        suggested_fix: Some(format!(
            "route this through the project's structured logging facility \
             (e.g. `tracing::info!` / `log::warn!` with the accepted spans \
             and fields), or, if `{module_path}` legitimately owns user-facing \
             or test output, accept it via `paradigms.OB.observer_paths` in \
             `locus.lock`"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::super::lockfile_schema::default_forbidden_log_targets;
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
            doc: None,
        })
    }

    fn raw_log_fact(symbol: &str, reason: &str) -> AirFact {
        AirFact {
            kind: FactKind::LogsRaw,
            target: FactTarget::Function {
                symbol: symbol.into(),
            },
            source: "test".into(),
            confidence: 1.0,
            reasons: vec![reason.into()],
        }
    }

    fn structured_log_fact(symbol: &str, reason: &str) -> AirFact {
        AirFact {
            kind: FactKind::LogsStructured,
            target: FactTarget::Function {
                symbol: symbol.into(),
            },
            source: "test".into(),
            confidence: 1.0,
            reasons: vec![reason.into()],
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
        }
    }

    #[test]
    fn ob001_fires_on_raw_println_in_non_observer_file() {
        let air = air_with(
            Some("x::domain::user"),
            vec![func("x::domain::user::greet", "t.rs", 5)],
            vec![raw_log_fact(
                "x::domain::user::greet",
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
    fn ob001_quiet_on_logs_structured_facts() {
        // `LogsStructured` is the canonical facility — never flagged.
        let air = air_with(
            Some("x::domain::user"),
            vec![func("x::domain::user::greet", "t.rs", 5)],
            vec![structured_log_fact(
                "x::domain::user::greet",
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
            vec![raw_log_fact("x::cli::main::run", "println")],
        );
        let section = ObSection {
            observer_paths: vec!["x::cli::*".into()],
            forbidden_log_targets: default_forbidden_log_targets(),
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
            vec![raw_log_fact("anon::fn", "println")],
        );
        assert!(ob001(&air, &default_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn ob001_agent_strict_elevates_warning_to_fatal() {
        let air = air_with(
            Some("x::domain::user"),
            vec![func("x::domain::user::greet", "t.rs", 5)],
            vec![raw_log_fact("x::domain::user::greet", "println")],
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
                raw_log_fact("x::domain::user::greet", "println"),
                raw_log_fact("x::domain::user::greet", "dbg"),
                raw_log_fact("x::domain::user::oops", "eprintln"),
                // LogsStructured is the canonical facility — never flagged.
                structured_log_fact("x::domain::user::ok", "tracing::info"),
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
            vec![raw_log_fact("x::domain::user::greet", "println")],
        );
        let section = ObSection {
            observer_paths: Vec::new(),
            forbidden_log_targets: default_forbidden_log_targets(),
        };
        assert!(ob001(&air, &section, CheckMode::Human).is_empty());
    }
}
