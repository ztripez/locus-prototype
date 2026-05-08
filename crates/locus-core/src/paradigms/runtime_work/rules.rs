//! RW rule implementations.
//!
//! Implemented:
//! - [`rw001`]: spawn-shaped fact outside any declared runtime owner module.
//!
//! All RW rules are lockfile-driven: they stay silent until the user has
//! populated `runtime_owner_paths` (otherwise we have no idea which modules
//! are legitimately spawning runtime work).

use locus_air::{AirFact, AirItem, AirSpan, AirWorkspace, FactKind, FactTarget};

use super::lockfile_schema::{RwSection, matches_pattern};
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

#[cfg(test)]
mod tests {
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

    fn spawn_fact(symbol: &str, reason: &str) -> AirFact {
        AirFact {
            kind: FactKind::SpawnedWork,
            target: FactTarget::Function {
                symbol: symbol.into(),
            },
            source: "test".into(),
            confidence: 1.0,
            reasons: vec![reason.into()],
            evidence: Some("tokio::spawn".into()),
        }
    }

    fn air_with_file(
        module_path: Option<&str>,
        file_path: &str,
        items: Vec<AirItem>,
        facts: Vec<AirFact>,
    ) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: file_path.into(),
                    module_path: module_path.map(|s| s.into()),
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
    fn rw001_fires_on_spawn_in_non_runtime_owner_file() {
        let air = air_with_file(
            Some("crate::handler"),
            "src/handler.rs",
            vec![func("crate::handler::create_user", "src/handler.rs", 17)],
            vec![spawn_fact(
                "crate::handler::create_user",
                "`tokio::spawn` is a spawn-shaped call",
            )],
        );
        let section = RwSection {
            runtime_owner_paths: vec!["crate::runtime::*".into(), "bin::*".into()],
        };
        let diags = rw001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "RW001");
        assert_eq!(diags[0].severity, Severity::Fatal);
        assert_eq!(diags[0].span.line_start, 17);
        assert!(diags[0].message.contains("crate::handler"));
        assert!(diags[0].message.contains("crate::handler::create_user"));
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("runtime_owner_paths")),
            "expected lockfile pattern reason; got {:?}",
            diags[0].why
        );
        assert!(
            diags[0].why.iter().any(|w| w.contains("spawn-shaped")),
            "expected spawn-shape reason; got {:?}",
            diags[0].why
        );
        assert!(
            diags[0].why.iter().any(|w| w.contains("create_user")),
            "expected enclosing-function reason; got {:?}",
            diags[0].why
        );
    }

    #[test]
    fn rw001_quiet_on_spawn_in_runtime_owner_pattern_file() {
        let air = air_with_file(
            Some("crate::runtime::pool"),
            "src/runtime/pool.rs",
            vec![func("crate::runtime::pool::run", "src/runtime/pool.rs", 4)],
            vec![spawn_fact("crate::runtime::pool::run", "spawn detected")],
        );
        let section = RwSection {
            runtime_owner_paths: vec!["crate::runtime::*".into()],
        };
        assert!(rw001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn rw001_quiet_on_non_spawnswork_facts() {
        let air = air_with_file(
            Some("crate::handler"),
            "src/handler.rs",
            vec![func("crate::handler::cfg", "src/handler.rs", 5)],
            vec![
                AirFact {
                    kind: FactKind::ConfigRead,
                    target: FactTarget::Function {
                        symbol: "crate::handler::cfg".into(),
                    },
                    source: "test".into(),
                    confidence: 1.0,
                    reasons: Vec::new(),
                    evidence: None,
                },
                AirFact {
                    kind: FactKind::Logging,
                    target: FactTarget::Function {
                        symbol: "crate::handler::cfg".into(),
                    },
                    source: "test".into(),
                    confidence: 1.0,
                    reasons: Vec::new(),
                    evidence: None,
                },
            ],
        );
        let section = RwSection {
            runtime_owner_paths: vec!["crate::runtime::*".into()],
        };
        assert!(rw001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn rw001_silent_when_runtime_owner_paths_empty() {
        let air = air_with_file(
            Some("crate::handler"),
            "src/handler.rs",
            vec![func("crate::handler::create_user", "src/handler.rs", 17)],
            vec![spawn_fact("crate::handler::create_user", "spawn detected")],
        );
        let section = RwSection::default();
        assert!(
            rw001(&air, &section, CheckMode::Human).is_empty(),
            "rule should wait for explicit runtime_owner_paths declaration"
        );
    }

    #[test]
    fn rw001_skips_files_without_module_path() {
        let air = air_with_file(
            None,
            "src/build.rs",
            vec![func("build::main", "src/build.rs", 2)],
            vec![spawn_fact("build::main", "spawn detected")],
        );
        let section = RwSection {
            runtime_owner_paths: vec!["crate::runtime::*".into()],
        };
        assert!(rw001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn rw001_agent_strict_keeps_fatal() {
        let air = air_with_file(
            Some("crate::handler"),
            "src/handler.rs",
            vec![func("crate::handler::process", "src/handler.rs", 12)],
            vec![spawn_fact(
                "crate::handler::process",
                "`rayon::spawn` is a spawn-shaped call",
            )],
        );
        let section = RwSection {
            runtime_owner_paths: vec!["crate::runtime::*".into()],
        };
        let diags = rw001(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }
}
