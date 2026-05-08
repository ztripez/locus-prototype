//! CF rules.
//!
//! Implemented:
//! - [`cf001`]: environment-variable read outside the config layer. Reads
//!   the workspace-level `AirFact` list — specifically `FactKind::ConfigRead`
//!   facts produced by the std-rt loader (or any other loader that knows
//!   about env-read patterns) — and pairs each with the file the targeted
//!   function lives in.
//! - [`cf002`]: filesystem-walk rule, reserved for a future
//!   filesystem-aware loader. Lockfile fields (`config_file_patterns`,
//!   `accepted_config_files`) ship today so users can pre-populate the
//!   allowlist; the rule body is a no-op stub.

use locus_air::{AirFact, AirItem, AirWorkspace, FactKind, FactTarget};

use super::lockfile_schema::{CfSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// CF001 — environment-variable read outside the config layer.
///
/// For every `FactKind::ConfigRead` fact produced by a loader, look up the
/// targeted function's file and fire when the file's `module_path` does
/// *not* match any pattern in `config_paths`.
///
/// Always Fatal: ownership of decision-data is structural — an env read in
/// a handler is hidden config ownership, the exact failure mode the
/// paradigm exists to prevent. Files that legitimately load configuration
/// declare themselves via `config_paths`.
///
/// Silent until `config_paths` is populated: like DG/UT/BO, CF is a user
/// assertion, not an inference. No `config_paths` means the user hasn't
/// declared a config layer yet, and the rule has nothing to reason about.
pub fn cf001(air: &AirWorkspace, section: &CfSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.config_paths.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for fact in &air.facts {
        if fact.kind != FactKind::ConfigRead {
            continue;
        }
        let FactTarget::Function { symbol } = &fact.target else {
            // Non-Function targets aren't paired to a module path here;
            // CF001 needs a module to evaluate against `config_paths`.
            continue;
        };
        let Some((module_path, fn_span)) = lookup_function(air, symbol) else {
            continue;
        };
        if section
            .config_paths
            .iter()
            .any(|pat| matches_pattern(pat, module_path))
        {
            continue;
        }
        out.push(diagnostic_for(fact, symbol, module_path, fn_span, mode));
    }
    out
}

/// CF002 — unregistered config-like file in the workspace.
///
/// **Reserved / not yet implemented.** Locus rules consume `AirWorkspace`,
/// which carries no filesystem-walk results. CF002 is a filesystem-aware
/// rule: it would scan the repo root for files matching
/// `section.config_file_patterns` (`*.yaml`, `*.toml`, …) outside any
/// pattern in `section.config_paths` (a module-path concept that maps to
/// directories the user already owns) and outside the
/// `section.accepted_config_files` allowlist (`Cargo.toml`,
/// `.github/**/*`, …).
///
/// Until a filesystem-aware loader lands and surfaces those facts to the
/// paradigm tier, this function returns an empty diagnostic vector. The
/// lockfile fields are present today so users can pre-populate them
/// without later schema churn; the eventual implementation will read the
/// same fields without breaking on-disk format.
///
/// Tracked: see workspace-level CHANGELOG / loader plan in
/// `docs/PARADIGMS.md` §"Framework Knowledge and Sub-Paradigm Loaders".
pub fn cf002(_air: &AirWorkspace, _section: &CfSection, _mode: CheckMode) -> Vec<Diagnostic> {
    Vec::new()
}

/// Find the `(module_path, function_span)` for the function with this
/// symbol. Returns `None` when the symbol isn't found, when the file has
/// no resolved module path, or when neither is available. Walks every
/// package/file/item — fine at the scale we operate on; a precomputed
/// index can replace this if it ever shows up hot in profiling.
fn lookup_function<'a>(
    air: &'a AirWorkspace,
    symbol: &str,
) -> Option<(&'a str, locus_air::AirSpan)> {
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
    fn_span: locus_air::AirSpan,
    mode: CheckMode,
) -> Diagnostic {
    // FactTarget::Span carries a precise call-site span; otherwise fall
    // back to the enclosing function's span (still useful for fix
    // targeting).
    let span = match &fact.target {
        FactTarget::Span(s) => s.clone(),
        FactTarget::Function { .. } | FactTarget::File { .. } => fn_span,
    };
    let function_label = symbol;
    let why_reasons = if fact.reasons.is_empty() {
        vec!["loader detected env-read pattern".to_string()]
    } else {
        fact.reasons.clone()
    };
    Diagnostic {
        rule_id: "CF001".to_string(),
        severity: mode.elevate(Severity::Fatal),
        span,
        concept: None,
        message: format!(
            "module `{module_path}` reads an environment variable from \
             `{function_label}` outside the config layer"
        ),
        why: {
            let mut w = vec![format!(
                "module `{module_path}` does not match any \
                 `paradigms.CF.config_paths` pattern"
            )];
            for r in why_reasons {
                w.push(r);
            }
            w.push(format!("enclosing function: `{function_label}`"));
            w
        },
        suggested_fix: Some(
            "move the env read into a config-layer module (one accepted \
             loader) and pass the resolved value through dependency \
             injection; if this file is the legitimate config owner, \
             add its module pattern to `paradigms.CF.config_paths` in \
             `locus.lock`"
                .into(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{
        AIR_SCHEMA_VERSION, AirFile, AirFunction, AirPackage, AirSpan, AirWorkspace, Visibility,
    };

    fn func(symbol: &str, line: u32) -> AirItem {
        AirItem::Function(AirFunction {
            name: symbol.rsplit("::").next().unwrap_or(symbol).into(),
            symbol: symbol.into(),
            visibility: Visibility::Public,
            params: Vec::new(),
            return_type: None,
            span: AirSpan::new("t.rs", line, line + 5),
            line_count: 6,
            doc: None,
        })
    }

    fn env_fact(symbol: &str, reason: &str) -> AirFact {
        AirFact {
            kind: FactKind::ConfigRead,
            target: FactTarget::Function {
                symbol: symbol.into(),
            },
            source: "test".into(),
            confidence: 1.0,
            reasons: vec![reason.into()],
            evidence: Some("std::env::var".into()),
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
                    module_path: module.map(|s| s.to_string()),
                    items,
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                }],
            }],
            facts,
        }
    }

    #[test]
    fn cf001_fires_when_env_read_in_non_config_file() {
        let air = air_with(
            Some("crate::handler::user"),
            vec![func("crate::handler::user::resolve_db", 12)],
            vec![env_fact(
                "crate::handler::user::resolve_db",
                "`std::env::var` reads an env var",
            )],
        );
        let section = CfSection {
            config_paths: vec!["crate::config::*".into()],
            ..Default::default()
        };
        let diags = cf001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "CF001");
        assert_eq!(diags[0].severity, Severity::Fatal);
        assert!(diags[0].message.contains("crate::handler::user"));
        assert!(diags[0].message.contains("resolve_db"));
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("config_paths") && w.contains("crate::handler::user")),
            "expected module-vs-config_paths reason in why; got {:?}",
            diags[0].why
        );
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("env var") || w.contains("env-read")),
            "expected loader reason in why; got {:?}",
            diags[0].why
        );
        assert!(
            diags[0].why.iter().any(|w| w.contains("resolve_db")),
            "expected enclosing function in why; got {:?}",
            diags[0].why
        );
    }

    #[test]
    fn cf001_quiet_when_env_read_in_config_pattern_file() {
        let air = air_with(
            Some("crate::config::loader"),
            vec![func("crate::config::loader::load", 10)],
            vec![env_fact("crate::config::loader::load", "env read")],
        );
        let section = CfSection {
            config_paths: vec!["crate::config::*".into()],
            ..Default::default()
        };
        assert!(cf001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cf001_quiet_on_non_readsenv_facts() {
        let air = air_with(
            Some("crate::handler::user"),
            vec![func("crate::handler::user::create", 20)],
            vec![
                AirFact {
                    kind: FactKind::SpawnedWork,
                    target: FactTarget::Function {
                        symbol: "crate::handler::user::create".into(),
                    },
                    source: "test".into(),
                    confidence: 1.0,
                    reasons: Vec::new(),
                    evidence: None,
                },
                AirFact {
                    kind: FactKind::Logging,
                    target: FactTarget::Function {
                        symbol: "crate::handler::user::create".into(),
                    },
                    source: "test".into(),
                    confidence: 1.0,
                    reasons: Vec::new(),
                    evidence: None,
                },
            ],
        );
        let section = CfSection {
            config_paths: vec!["crate::config::*".into()],
            ..Default::default()
        };
        assert!(cf001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cf001_silent_when_config_paths_empty() {
        let air = air_with(
            Some("crate::handler::user"),
            vec![func("crate::handler::user::resolve_db", 12)],
            vec![env_fact("crate::handler::user::resolve_db", "env read")],
        );
        let section = CfSection::default();
        assert!(cf001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cf001_skips_files_without_module_path() {
        // A file the adapter couldn't resolve to a module path can't be
        // judged against config_paths — skip it rather than firing
        // spuriously. The function lookup walks AIR — if no file with a
        // module path holds the function, the lookup misses and the rule
        // stays silent.
        let air = air_with(
            None,
            vec![func("anonymous::resolve", 12)],
            vec![env_fact("anonymous::resolve", "env read")],
        );
        let section = CfSection {
            config_paths: vec!["crate::config::*".into()],
            ..Default::default()
        };
        assert!(cf001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cf001_agent_strict_keeps_severity_fatal() {
        // CF001 is already Fatal in human mode; --agent-strict elevates but
        // can't go higher than Fatal — verify it stays Fatal, not panicked.
        let air = air_with(
            Some("crate::handler::user"),
            vec![func("crate::handler::user::call", 30)],
            vec![env_fact("crate::handler::user::call", "env read")],
        );
        let section = CfSection {
            config_paths: vec!["crate::config::*".into()],
            ..Default::default()
        };
        let diags = cf001(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn cf001_skips_facts_whose_function_isnt_in_air() {
        // A loader can produce a fact for a function the AIR doesn't carry
        // (e.g. external crate). CF001 has nothing to evaluate — skip
        // rather than panic.
        let air = air_with(
            Some("crate::handler::user"),
            Vec::new(), // no functions
            vec![env_fact("crate::other::resolve_db", "env read")],
        );
        let section = CfSection {
            config_paths: vec!["crate::config::*".into()],
            ..Default::default()
        };
        assert!(cf001(&air, &section, CheckMode::Human).is_empty());
    }

    // ---- CF002: deferred filesystem-walk rule ----

    #[test]
    fn cf002_returns_no_diagnostics_today() {
        // Stub rule — the body is reserved for a future filesystem-aware
        // loader. Until that lands, CF002 is silent regardless of input.
        let air = air_with(Some("crate::handler::user"), Vec::new(), Vec::new());
        let section = CfSection::default();
        assert!(cf002(&air, &section, CheckMode::Human).is_empty());
        assert!(cf002(&air, &section, CheckMode::AgentStrict).is_empty());
    }

    #[test]
    fn cf002_lockfile_fields_round_trip_through_serde() {
        // Users can pre-populate the future rule's allowlist today.
        // The defaults survive a serde round-trip; partial JSON falls back
        // to the seeded patterns.
        let s = CfSection::default();
        assert!(!s.config_file_patterns.is_empty());
        assert!(!s.accepted_config_files.is_empty());

        let j = serde_json::to_value(&s).unwrap();
        let back: CfSection = serde_json::from_value(j).unwrap();
        assert_eq!(s, back);

        let from_empty: CfSection = serde_json::from_str("{}").unwrap();
        assert_eq!(from_empty, CfSection::default());
    }
}
