//! CF rules.
//!
//! Implemented:
//! - [`cf001`]: environment-variable read outside the config layer. Uses CF's
//!   own lockfile shape (`config_paths`) and the AIR `ActionKind::EnvRead`
//!   truth-action emitted by the Rust visitor when a call resolves to
//!   `*::env::var` / `*::env::var_os`.

use locus_air::{ActionKind, AirItem, AirWorkspace};

use super::lockfile_schema::{CfSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// CF001 — environment-variable read outside the config layer.
///
/// For every `AirFile` whose `module_path` does *not* match any pattern in
/// `config_paths`, walk its `AirItem::TruthAction` items. Fire when the
/// action's `kind` is `ActionKind::EnvRead`.
///
/// Always Fatal: ownership of decision-data is structural — an env read in a
/// handler is hidden config ownership, the exact failure mode the paradigm
/// exists to prevent. Files that legitimately load configuration declare
/// themselves via `config_paths`.
///
/// Silent until `config_paths` is populated: like DG/UT/BO, CF is a user
/// assertion, not an inference. No `config_paths` means the user hasn't
/// declared a config layer yet, and the rule has nothing to reason about.
pub fn cf001(air: &AirWorkspace, section: &CfSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.config_paths.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            if section
                .config_paths
                .iter()
                .any(|pat| matches_pattern(pat, module_path))
            {
                continue;
            }
            for item in &file.items {
                let AirItem::TruthAction(action) = item else {
                    continue;
                };
                if action.action != ActionKind::EnvRead {
                    continue;
                }
                let function_label = action.function.as_deref().unwrap_or("<unknown>");
                out.push(Diagnostic {
                    rule_id: "CF001".to_string(),
                    severity: mode.elevate(Severity::Fatal),
                    span: action.span.clone(),
                    concept: None,
                    message: format!(
                        "module `{module_path}` reads environment variable `{}` from \
                         `{function_label}` outside the config layer",
                        action.target
                    ),
                    why: vec![
                        format!(
                            "module `{module_path}` does not match any \
                             `paradigms.CF.config_paths` pattern"
                        ),
                        format!(
                            "call resolves to an environment-variable read \
                             (target `{}`)",
                            action.target
                        ),
                        format!("enclosing function: `{function_label}`"),
                    ],
                    suggested_fix: Some(
                        "move the env read into a config-layer module (one accepted \
                         loader) and pass the resolved value through dependency \
                         injection; if this file is the legitimate config owner, \
                         add its module pattern to `paradigms.CF.config_paths` in \
                         `locus.lock`"
                            .into(),
                    ),
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

    fn truth_action(kind: ActionKind, target: &str, function: Option<&str>, line: u32) -> AirItem {
        AirItem::TruthAction(AirTruthAction {
            action: kind,
            target: target.into(),
            function: function.map(|s| s.to_string()),
            span: AirSpan::new("t.rs", line, line),
            confidence: 0.9,
            reasons: Vec::new(),
        })
    }

    fn air_with_module(module: Option<&str>, items: Vec<AirItem>) -> AirWorkspace {
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
        }
    }

    #[test]
    fn cf001_fires_when_env_read_in_non_config_file() {
        let air = air_with_module(
            Some("crate::handler::user"),
            vec![truth_action(
                ActionKind::EnvRead,
                "DATABASE_URL",
                Some("crate::handler::user::resolve_db"),
                12,
            )],
        );
        let section = CfSection {
            config_paths: vec!["crate::config::*".into()],
        };
        let diags = cf001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "CF001");
        assert_eq!(diags[0].severity, Severity::Fatal);
        assert!(diags[0].message.contains("crate::handler::user"));
        assert!(diags[0].message.contains("DATABASE_URL"));
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
                .any(|w| w.contains("environment-variable read")),
            "expected env-read reason in why; got {:?}",
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
        let air = air_with_module(
            Some("crate::config::loader"),
            vec![truth_action(
                ActionKind::EnvRead,
                "DATABASE_URL",
                Some("crate::config::loader::load"),
                10,
            )],
        );
        let section = CfSection {
            config_paths: vec!["crate::config::*".into()],
        };
        assert!(cf001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cf001_quiet_on_non_envread_truth_actions() {
        let air = air_with_module(
            Some("crate::handler::user"),
            vec![
                truth_action(
                    ActionKind::Construct,
                    "User",
                    Some("crate::handler::user::create"),
                    20,
                ),
                truth_action(
                    ActionKind::Log,
                    "info",
                    Some("crate::handler::user::create"),
                    21,
                ),
                truth_action(
                    ActionKind::Spawn,
                    "tokio::spawn",
                    Some("crate::handler::user::create"),
                    22,
                ),
            ],
        );
        let section = CfSection {
            config_paths: vec!["crate::config::*".into()],
        };
        assert!(cf001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cf001_silent_when_config_paths_empty() {
        let air = air_with_module(
            Some("crate::handler::user"),
            vec![truth_action(
                ActionKind::EnvRead,
                "DATABASE_URL",
                Some("crate::handler::user::resolve_db"),
                12,
            )],
        );
        let section = CfSection::default();
        assert!(cf001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cf001_skips_files_without_module_path() {
        // A file the adapter couldn't resolve to a module path can't be
        // judged against config_paths — skip it rather than firing
        // spuriously.
        let air = air_with_module(
            None,
            vec![truth_action(
                ActionKind::EnvRead,
                "DATABASE_URL",
                Some("anonymous"),
                12,
            )],
        );
        let section = CfSection {
            config_paths: vec!["crate::config::*".into()],
        };
        assert!(cf001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cf001_agent_strict_keeps_severity_fatal() {
        // CF001 is already Fatal in human mode; --agent-strict elevates but
        // can't go higher than Fatal — verify it stays Fatal, not panicked.
        let air = air_with_module(
            Some("crate::handler::user"),
            vec![truth_action(
                ActionKind::EnvRead,
                "API_KEY",
                Some("crate::handler::user::call"),
                30,
            )],
        );
        let section = CfSection {
            config_paths: vec!["crate::config::*".into()],
        };
        let diags = cf001(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn cf001_uses_unknown_label_when_function_missing() {
        // Truth-actions emitted outside any function (top-level statics,
        // module-init expressions) carry `function: None`. The diagnostic
        // should still render — fall back to a placeholder label.
        let air = air_with_module(
            Some("crate::handler::user"),
            vec![truth_action(ActionKind::EnvRead, "API_KEY", None, 5)],
        );
        let section = CfSection {
            config_paths: vec!["crate::config::*".into()],
        };
        let diags = cf001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("<unknown>"));
        assert!(
            diags[0].why.iter().any(|w| w.contains("<unknown>")),
            "expected <unknown> placeholder in why; got {:?}",
            diags[0].why
        );
    }
}
