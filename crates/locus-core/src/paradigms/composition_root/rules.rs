//! CR rule implementations.
//!
//! Implemented:
//! - [`cr001`]: service-shaped construction outside any declared composition
//!   root.
//!
//! All CR rules are lockfile-driven: they stay silent until the user has
//! populated `composition_root_paths` (otherwise we have no idea which
//! modules are legitimately wiring concrete services).

use locus_air::{ActionKind, AirItem, AirWorkspace};

use super::lockfile_schema::{CrSection, effective_service_suffixes};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// CR001 — service-shaped construction outside composition root.
///
/// For every `AirItem::TruthAction` with `action == Construct`, fires when:
/// - the file's `module_path` does NOT match any `composition_root_paths`
///   pattern, and
/// - the construction target's last `::` segment ends with one of the
///   accepted service suffixes (heuristic: `Service`, `Client`, `Repository`,
///   `Adapter`, `Connection`, `Pool`, `Manager` by default).
///
/// Always Fatal: composition-root violations are a layered-architecture
/// issue — concrete services must not be wired in handlers, services, or
/// feature modules.
///
/// Silent when `composition_root_paths` is empty: we wait for the user to
/// declare where their roots live before flagging anything.
pub fn cr001(air: &AirWorkspace, section: &CrSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.composition_root_paths.is_empty() {
        return Vec::new();
    }
    let suffixes = effective_service_suffixes(section);
    if suffixes.is_empty() {
        // Defensive: an explicitly user-cleared override could in principle
        // produce this, but `effective_service_suffixes` falls back to the
        // canonical seven on empty input. Either way: nothing to match.
        return Vec::new();
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let module_path = file.module_path.as_deref().unwrap_or("");
            if section
                .composition_root_paths
                .iter()
                .any(|pat| matches_pattern(pat, module_path))
            {
                continue; // file is itself a composition root
            }
            for item in &file.items {
                let AirItem::TruthAction(a) = item else {
                    continue;
                };
                if a.action != ActionKind::Construct {
                    continue;
                }
                let short = a
                    .target
                    .rsplit("::")
                    .next()
                    .unwrap_or(a.target.as_str())
                    .trim();
                let Some(matched_suffix) = suffixes.iter().find(|s| short.ends_with(s.as_str()))
                else {
                    continue;
                };
                let function_label = a
                    .function
                    .as_deref()
                    .unwrap_or("(no enclosing function recorded)");
                let module_label = if module_path.is_empty() {
                    "(unknown module)"
                } else {
                    module_path
                };
                out.push(Diagnostic {
                    rule_id: "CR001".to_string(),
                    severity: mode.elevate(Severity::Fatal),
                    span: a.span.clone(),
                    concept: None,
                    message: format!(
                        "service-shaped construction of `{}` in module `{module_label}` \
                         (matched suffix `{matched_suffix}`) outside any declared \
                         composition root",
                        a.target
                    ),
                    why: vec![
                        format!(
                            "module `{module_label}` matches none of the \
                             `composition_root_paths` patterns"
                        ),
                        format!("target `{}` ends with `{matched_suffix}`", a.target),
                        format!("enclosing function: `{function_label}`"),
                    ],
                    suggested_fix: Some(format!(
                        "move the construction of `{}` into a composition root \
                         (e.g. `main`, a `wire` module, or a declared composition \
                         module), or accept this file by adding its module to \
                         `paradigms.CR.composition_root_paths`",
                        a.target
                    )),
                });
            }
        }
    }
    out
}

/// Pattern matching duplicated locally to mirror DG/UT (suffix wildcards).
/// Kept private to this module so any future tweak to CR's matcher doesn't
/// silently affect DG/UT.
fn matches_pattern(pattern: &str, path: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix("::*") {
        return path == prefix || path.starts_with(&format!("{prefix}::"));
    }
    pattern == path
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{
        AIR_SCHEMA_VERSION, AirFile, AirPackage, AirSpan, AirTruthAction, AirWorkspace,
    };

    fn construct(target: &str, function: &str, file_path: &str, line: u32) -> AirItem {
        AirItem::TruthAction(AirTruthAction {
            action: ActionKind::Construct,
            target: target.into(),
            function: Some(function.into()),
            span: AirSpan::new(file_path, line, line),
            confidence: 0.95,
            reasons: vec!["struct literal".into()],
        })
    }

    fn air_with_file(module_path: &str, file_path: &str, items: Vec<AirItem>) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: file_path.into(),
                    module_path: Some(module_path.into()),
                    items,
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                }],
            }],
        }
    }

    #[test]
    fn cr001_fires_on_service_shaped_construct_outside_root() {
        let air = air_with_file(
            "crate::handler",
            "src/handler.rs",
            vec![construct(
                "UserRepository",
                "crate::handler::create_user",
                "src/handler.rs",
                12,
            )],
        );
        let section = CrSection {
            composition_root_paths: vec!["crate::wire".into(), "bin::*".into()],
            service_suffixes: Vec::new(),
        };
        let diags = cr001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "CR001");
        assert_eq!(diags[0].severity, Severity::Fatal);
        assert!(diags[0].message.contains("UserRepository"));
        assert!(diags[0].message.contains("crate::handler"));
        assert!(diags[0].message.contains("Repository"));
    }

    #[test]
    fn cr001_quiet_inside_composition_root() {
        let air = air_with_file(
            "crate::wire",
            "src/wire.rs",
            vec![construct(
                "UserRepository",
                "crate::wire::build_app",
                "src/wire.rs",
                3,
            )],
        );
        let section = CrSection {
            composition_root_paths: vec!["crate::wire".into()],
            service_suffixes: Vec::new(),
        };
        assert!(cr001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cr001_quiet_on_non_service_shaped_target() {
        let air = air_with_file(
            "crate::handler",
            "src/handler.rs",
            vec![construct(
                "User",
                "crate::handler::create_user",
                "src/handler.rs",
                7,
            )],
        );
        let section = CrSection {
            composition_root_paths: vec!["crate::wire".into()],
            service_suffixes: Vec::new(),
        };
        assert!(cr001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cr001_silent_when_composition_root_paths_empty() {
        let air = air_with_file(
            "crate::handler",
            "src/handler.rs",
            vec![construct(
                "UserRepository",
                "crate::handler::create_user",
                "src/handler.rs",
                4,
            )],
        );
        let section = CrSection::default();
        assert!(
            cr001(&air, &section, CheckMode::Human).is_empty(),
            "rule should wait for explicit composition_root_paths declaration"
        );
    }

    #[test]
    fn cr001_agent_strict_keeps_fatal() {
        let air = air_with_file(
            "crate::handler",
            "src/handler.rs",
            vec![construct(
                "PaymentClient",
                "crate::handler::charge",
                "src/handler.rs",
                9,
            )],
        );
        let section = CrSection {
            composition_root_paths: vec!["crate::wire".into()],
            service_suffixes: Vec::new(),
        };
        let diags = cr001(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn cr001_custom_service_suffixes_override_defaults() {
        // Default suffixes would NOT catch `Gateway`; a `Repository` would.
        // With a user override that drops `Repository` and adds `Gateway`,
        // the behaviour flips.
        let air = air_with_file(
            "crate::handler",
            "src/handler.rs",
            vec![
                construct(
                    "PaymentGateway",
                    "crate::handler::charge",
                    "src/handler.rs",
                    11,
                ),
                construct(
                    "UserRepository",
                    "crate::handler::create_user",
                    "src/handler.rs",
                    22,
                ),
            ],
        );
        let section = CrSection {
            composition_root_paths: vec!["crate::wire".into()],
            service_suffixes: vec!["Gateway".into()],
        };
        let diags = cr001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1, "only `Gateway` should match; got {diags:?}");
        assert!(diags[0].message.contains("PaymentGateway"));
        assert!(!diags[0].message.contains("UserRepository"));
    }

    #[test]
    fn cr001_matches_path_qualified_target() {
        // Constructions like `crate::infra::DbConnection { ... }` appear in
        // AIR with the full path; the suffix check uses the last `::` segment.
        let air = air_with_file(
            "crate::handler",
            "src/handler.rs",
            vec![construct(
                "crate::infra::DbConnection",
                "crate::handler::open",
                "src/handler.rs",
                5,
            )],
        );
        let section = CrSection {
            composition_root_paths: vec!["crate::wire".into()],
            service_suffixes: Vec::new(),
        };
        let diags = cr001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("crate::infra::DbConnection"));
        assert!(diags[0].message.contains("Connection"));
    }
}
