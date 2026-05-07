//! RW rule implementations.
//!
//! Implemented:
//! - [`rw001`]: spawn-shaped action outside any declared runtime owner module.
//!
//! All RW rules are lockfile-driven: they stay silent until the user has
//! populated `runtime_owner_paths` (otherwise we have no idea which modules
//! are legitimately spawning runtime work).

use locus_air::{ActionKind, AirItem, AirWorkspace};

use super::lockfile_schema::{RwSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// RW001 — spawn outside the runtime-ownership boundary.
///
/// For every `AirItem::TruthAction` with `action == Spawn`, fires when the
/// containing file's `module_path` does NOT match any pattern in
/// `runtime_owner_paths`.
///
/// Always Fatal: per the spec, runtime-ownership violations are structural —
/// `tokio::spawn` (or any equivalent) dropped into a handler hides
/// concurrency, error-propagation, and lifecycle concerns from the layer
/// that owns them.
///
/// Silent when `runtime_owner_paths` is empty: we wait for the user to
/// declare where their runtime owners live before flagging anything. Files
/// without a `module_path` are skipped — we can't decide anything about them.
pub fn rw001(air: &AirWorkspace, section: &RwSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.runtime_owner_paths.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                // Without a module path we can't classify the file as
                // runtime-owner-or-not; skip rather than guess.
                continue;
            };
            if section
                .runtime_owner_paths
                .iter()
                .any(|pat| matches_pattern(pat, module_path))
            {
                continue; // file is itself a runtime owner
            }
            for item in &file.items {
                let AirItem::TruthAction(a) = item else {
                    continue;
                };
                if a.action != ActionKind::Spawn {
                    continue;
                }
                let function_label = a
                    .function
                    .as_deref()
                    .unwrap_or("(no enclosing function recorded)");
                out.push(Diagnostic {
                    rule_id: "RW001".to_string(),
                    severity: mode.elevate(Severity::Fatal),
                    span: a.span.clone(),
                    concept: None,
                    message: format!(
                        "spawn-shaped call `{}` in module `{module_path}` \
                         (function `{function_label}`) outside any declared \
                         runtime owner",
                        a.target
                    ),
                    why: vec![
                        format!(
                            "module `{module_path}` matches none of the \
                             `runtime_owner_paths` patterns"
                        ),
                        format!("call target `{}` is a spawn-shaped path", a.target),
                        format!("enclosing function: `{function_label}`"),
                    ],
                    suggested_fix: Some(format!(
                        "move the spawn of `{}` into a runtime-owner module \
                         (job queue, orchestrator, supervisor, or runtime entry \
                         point) and have this code submit work to it through a \
                         port; or, if `{module_path}` really is a legitimate \
                         runtime owner, expand `paradigms.RW.runtime_owner_paths` \
                         in `locus.lock` to include it",
                        a.target
                    )),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{
        AIR_SCHEMA_VERSION, AirFile, AirPackage, AirSpan, AirTruthAction, AirWorkspace,
    };

    fn truth_action(
        action: ActionKind,
        target: &str,
        function: Option<&str>,
        file_path: &str,
        line: u32,
    ) -> AirItem {
        AirItem::TruthAction(AirTruthAction {
            action,
            target: target.into(),
            function: function.map(|s| s.into()),
            span: AirSpan::new(file_path, line, line),
            confidence: 0.95,
            reasons: vec!["spawn-shaped path".into()],
        })
    }

    fn spawn(target: &str, function: &str, file_path: &str, line: u32) -> AirItem {
        truth_action(ActionKind::Spawn, target, Some(function), file_path, line)
    }

    fn air_with_file(
        module_path: Option<&str>,
        file_path: &str,
        items: Vec<AirItem>,
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
        }
    }

    #[test]
    fn rw001_fires_on_spawn_in_non_runtime_owner_file() {
        let air = air_with_file(
            Some("crate::handler"),
            "src/handler.rs",
            vec![spawn(
                "tokio::spawn",
                "crate::handler::create_user",
                "src/handler.rs",
                17,
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
        assert!(diags[0].message.contains("tokio::spawn"));
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
            vec![spawn(
                "tokio::spawn",
                "crate::runtime::pool::run",
                "src/runtime/pool.rs",
                4,
            )],
        );
        let section = RwSection {
            runtime_owner_paths: vec!["crate::runtime::*".into()],
        };
        assert!(rw001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn rw001_quiet_on_non_spawn_truth_actions() {
        let air = air_with_file(
            Some("crate::handler"),
            "src/handler.rs",
            vec![
                truth_action(
                    ActionKind::Construct,
                    "User",
                    Some("crate::handler::make"),
                    "src/handler.rs",
                    3,
                ),
                truth_action(
                    ActionKind::EnvRead,
                    "std::env::var",
                    Some("crate::handler::cfg"),
                    "src/handler.rs",
                    7,
                ),
                truth_action(
                    ActionKind::Log,
                    "tracing::info",
                    Some("crate::handler::cfg"),
                    "src/handler.rs",
                    9,
                ),
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
            vec![spawn(
                "tokio::spawn",
                "crate::handler::create_user",
                "src/handler.rs",
                17,
            )],
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
            vec![spawn(
                "std::thread::spawn",
                "build::main",
                "src/build.rs",
                2,
            )],
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
            vec![spawn(
                "rayon::spawn",
                "crate::handler::process",
                "src/handler.rs",
                12,
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
