//! BO rules.
//!
//! Implemented:
//! - [`bo001`]: domain/application file imports a transport- or
//!   persistence-style dependency. Conceptually adjacent to DG001 but uses
//!   BO's own lockfile shape (`domain_paths` × `forbidden_in_domain`) and is
//!   dedicated to the boundary-vs-domain split.

use locus_air::{AirItem, AirWorkspace};

use super::lockfile_schema::{BoSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// BO001 — domain/application file imports a forbidden transport/persistence
/// dependency.
///
/// For every `AirFile` whose `module_path` matches any pattern in
/// `domain_paths`, walk its `AirImport` items. Fire when the import path
/// matches any pattern in `forbidden_in_domain`.
///
/// Always Fatal: domain leakage of transport/persistence breaks the layered
/// architecture the user has declared via the lockfile — same justification
/// as DG001's forbidden edges.
pub fn bo001(air: &AirWorkspace, section: &BoSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.domain_paths.is_empty() || section.forbidden_in_domain.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(domain_pattern) = section
                .domain_paths
                .iter()
                .find(|pat| matches_pattern(pat, module_path))
            else {
                continue;
            };
            for item in &file.items {
                let AirItem::Import(imp) = item else {
                    continue;
                };
                let Some(forbidden_pattern) = section
                    .forbidden_in_domain
                    .iter()
                    .find(|pat| matches_pattern(pat, &imp.path))
                else {
                    continue;
                };
                out.push(Diagnostic {
                    rule_id: "BO001".to_string(),
                    severity: mode.elevate(Severity::Fatal),
                    span: imp.span.clone(),
                    concept: None,
                    message: format!(
                        "domain module `{module_path}` imports forbidden \
                         transport/persistence path `{}`",
                        imp.path
                    ),
                    why: vec![
                        format!(
                            "importer `{module_path}` matches domain_paths pattern \
                             `{domain_pattern}`"
                        ),
                        format!(
                            "import `{}` matches forbidden_in_domain pattern \
                             `{forbidden_pattern}`",
                            imp.path
                        ),
                        "domain/application code must not depend directly on transport, \
                         persistence, or serialization frameworks; those concerns belong \
                         at the boundary"
                            .into(),
                    ],
                    suggested_fix: Some(
                        "convert at the boundary (introduce a port/adapter, or move the \
                         conversion into an application-layer service that calls the \
                         framework on the domain's behalf); if the import is a \
                         domain-friendly utility, narrow the `paradigms.BO.forbidden_in_domain` \
                         pattern in `locus.lock` so it no longer matches"
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
    use locus_air::{AIR_SCHEMA_VERSION, AirFile, AirImport, AirPackage, AirSpan, Visibility};

    fn import(path: &str) -> AirItem {
        AirItem::Import(AirImport {
            path: path.into(),
            visibility: Visibility::Private,
            span: AirSpan::new("t.rs", 1, 1),
        })
    }

    fn air_with_module(module: &str, items: Vec<AirItem>) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "t.rs".into(),
                    module_path: Some(module.into()),
                    items,
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                }],
            }],
        }
    }

    #[test]
    fn bo001_fires_when_domain_file_imports_forbidden_path() {
        let air = air_with_module("crate::domain::user", vec![import("sqlx::Pool")]);
        let section = BoSection {
            domain_paths: vec!["crate::domain::*".into()],
            forbidden_in_domain: vec!["sqlx::*".into()],
        };
        let diags = bo001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "BO001");
        assert_eq!(diags[0].severity, Severity::Fatal);
        assert!(diags[0].message.contains("crate::domain::user"));
        assert!(diags[0].message.contains("sqlx::Pool"));
        assert!(
            diags[0].why.iter().any(|w| w.contains("crate::domain::*")),
            "expected domain pattern in why; got {:?}",
            diags[0].why
        );
        assert!(
            diags[0].why.iter().any(|w| w.contains("sqlx::*")),
            "expected forbidden pattern in why; got {:?}",
            diags[0].why
        );
    }

    #[test]
    fn bo001_quiet_when_non_domain_file_imports_forbidden_path() {
        // Adapter/infra layer is allowed to use sqlx — that's the whole point
        // of putting persistence at the boundary.
        let air = air_with_module("crate::infra::user_repo", vec![import("sqlx::Pool")]);
        let section = BoSection {
            domain_paths: vec!["crate::domain::*".into()],
            forbidden_in_domain: vec!["sqlx::*".into()],
        };
        assert!(bo001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn bo001_quiet_when_domain_file_imports_non_forbidden_path() {
        let air = air_with_module(
            "crate::domain::user",
            vec![import("crate::domain::value::Email")],
        );
        let section = BoSection {
            domain_paths: vec!["crate::domain::*".into()],
            forbidden_in_domain: vec!["sqlx::*".into()],
        };
        assert!(bo001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn bo001_silent_when_domain_paths_empty() {
        let air = air_with_module("crate::domain::user", vec![import("sqlx::Pool")]);
        let section = BoSection {
            domain_paths: vec![],
            forbidden_in_domain: vec!["sqlx::*".into()],
        };
        assert!(bo001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn bo001_silent_when_forbidden_in_domain_empty() {
        let air = air_with_module("crate::domain::user", vec![import("sqlx::Pool")]);
        let section = BoSection {
            domain_paths: vec!["crate::domain::*".into()],
            forbidden_in_domain: vec![],
        };
        assert!(bo001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn bo001_silent_with_default_section() {
        let air = air_with_module("crate::domain::user", vec![import("sqlx::Pool")]);
        let section = BoSection::default();
        assert!(bo001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn bo001_agent_strict_keeps_severity_fatal() {
        // BO001 is already Fatal in human mode; --agent-strict elevates but
        // can't go higher than Fatal — verify it stays Fatal, not panicked.
        let air = air_with_module("crate::domain::user", vec![import("reqwest::Client")]);
        let section = BoSection {
            domain_paths: vec!["crate::domain::*".into()],
            forbidden_in_domain: vec!["reqwest::*".into()],
        };
        let diags = bo001(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }
}
