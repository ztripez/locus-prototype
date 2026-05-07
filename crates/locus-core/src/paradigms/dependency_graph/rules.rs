//! DG rules.
//!
//! Implemented:
//! - [`dg001`]: forbidden import (importer matches a forbidden edge's `from`
//!   pattern AND the imported path matches the `to` pattern)
//!
//! Future: DG002 (cyclic dependency between modules), DG003 (cross-feature
//! internals reach), DG004 (shared module reaching feature-specific symbol).

use locus_air::{AirItem, AirWorkspace};

use super::lockfile_schema::{DgSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// DG001 — forbidden import.
///
/// For every `AirImport` in every file, walk the lockfile's `forbidden_edges`.
/// Fire when the file's `module_path` matches the edge's `from` pattern AND
/// the import path matches the edge's `to` pattern.
///
/// Always Fatal: a forbidden edge is, by the user's own declaration, a
/// directional violation.
pub fn dg001(air: &AirWorkspace, section: &DgSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.forbidden_edges.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            for item in &file.items {
                let AirItem::Import(imp) = item else {
                    continue;
                };
                for edge in &section.forbidden_edges {
                    if !matches_pattern(&edge.from, module_path) {
                        continue;
                    }
                    if !matches_pattern(&edge.to, &imp.path) {
                        continue;
                    }
                    let mut why = vec![
                        format!("importer `{module_path}` matches `from = {}`", edge.from),
                        format!("import `{}` matches `to = {}`", imp.path, edge.to),
                    ];
                    if let Some(reason) = &edge.reason {
                        why.push(format!("reason: {reason}"));
                    }
                    out.push(Diagnostic {
                        rule_id: "DG001".to_string(),
                        severity: mode.elevate(Severity::Fatal),
                        span: imp.span.clone(),
                        concept: None,
                        message: format!(
                            "forbidden import: `{module_path}` must not reach `{}`",
                            imp.path
                        ),
                        why,
                        suggested_fix: Some(
                            "remove the import, or route the call through an accepted \
                             intermediary (port, application service, or shared crate); \
                             if the edge is wrong, edit `paradigms.DG.forbidden_edges` in \
                             `locus.lock`"
                                .into(),
                        ),
                    });
                    break; // one diagnostic per (file, import); don't re-fire on overlapping edges
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::lockfile_schema::ForbiddenEdge;
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
                }],
            }],
        }
    }

    fn forbid(from: &str, to: &str) -> ForbiddenEdge {
        ForbiddenEdge {
            from: from.into(),
            to: to.into(),
            reason: None,
        }
    }

    #[test]
    fn dg001_fires_when_module_imports_forbidden_path() {
        let air = air_with_module("lore::domain::user", vec![import("lore::api::v1::UserDto")]);
        let section = DgSection {
            forbidden_edges: vec![forbid("lore::domain::*", "lore::api::*")],
        };
        let diags = dg001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "DG001");
        assert_eq!(diags[0].severity, Severity::Fatal);
        assert!(diags[0].message.contains("lore::api::v1::UserDto"));
        assert!(diags[0].message.contains("lore::domain::user"));
    }

    #[test]
    fn dg001_quiet_when_no_edges_match() {
        let air = air_with_module("lore::domain::user", vec![import("lore::core::Config")]);
        let section = DgSection {
            forbidden_edges: vec![forbid("lore::domain::*", "lore::api::*")],
        };
        assert!(dg001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn dg001_silent_with_empty_lockfile() {
        let air = air_with_module("lore::domain::user", vec![import("lore::api::v1::UserDto")]);
        let section = DgSection::default();
        assert!(dg001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn dg001_skips_non_matching_module_even_if_import_matches() {
        // `from` constrains the importer; api importing api is fine here.
        let air = air_with_module("lore::api::handler", vec![import("lore::api::v1::UserDto")]);
        let section = DgSection {
            forbidden_edges: vec![forbid("lore::domain::*", "lore::api::*")],
        };
        assert!(dg001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn dg001_one_diagnostic_per_file_per_import_when_multiple_edges_match() {
        let air = air_with_module("lore::domain::user", vec![import("lore::api::v1::UserDto")]);
        let section = DgSection {
            forbidden_edges: vec![
                forbid("lore::domain::*", "lore::api::*"),
                forbid("*", "lore::api::v1::UserDto"), // separately covers the same import
            ],
        };
        let diags = dg001(&air, &section, CheckMode::Human);
        assert_eq!(
            diags.len(),
            1,
            "overlapping forbidden edges should not double-report; got {diags:?}"
        );
    }

    #[test]
    fn dg001_carries_reason_into_why() {
        let air = air_with_module("lore::domain::user", vec![import("lore::api::v1::UserDto")]);
        let section = DgSection {
            forbidden_edges: vec![ForbiddenEdge {
                from: "lore::domain::*".into(),
                to: "lore::api::*".into(),
                reason: Some("domain must not depend on transport".into()),
            }],
        };
        let diags = dg001(&air, &section, CheckMode::Human);
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("domain must not depend on transport")),
            "expected reason in `why`; got {:?}",
            diags[0].why
        );
    }
}
