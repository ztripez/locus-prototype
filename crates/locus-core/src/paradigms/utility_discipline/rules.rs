//! UT rules.
//!
//! Implemented:
//! - [`ut001`]: utility module defines a public type. A "utility module" by
//!   definition holds domain-free technical helpers; defining a public *type*
//!   in one is a smell because types carry semantics, and semantics belong to
//!   a domain/feature module.
//! - [`ut002`]: utility module imports a forbidden feature/domain path. UT001
//!   catches public types defined in utility modules; UT002 catches helpers
//!   that *know about* domain concepts via imports.

use locus_air::{AirItem, AirWorkspace, Visibility};

use super::lockfile_schema::{UtSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// UT001 — utility module defines a public type.
///
/// For every `AirFile` whose `module_path` matches any pattern in
/// `utility_paths`, fire one diagnostic per public `AirItem::Type`.
///
/// Severity: Warning by default; Fatal under `--agent-strict`. The spec lists
/// this as a heuristic warning — utility modules can legitimately hold private
/// helper types, so the structural fail-fast tier isn't a fit.
pub fn ut001(air: &AirWorkspace, section: &UtSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.utility_paths.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(pattern) = section
                .utility_paths
                .iter()
                .find(|pat| matches_pattern(pat, module_path))
            else {
                continue;
            };
            for item in &file.items {
                let AirItem::Type(ty) = item else {
                    continue;
                };
                if ty.visibility != Visibility::Public {
                    continue;
                }
                out.push(Diagnostic {
                    rule_id: "UT001".to_string(),
                    severity: mode.elevate(Severity::Warning),
                    span: ty.span.clone(),
                    concept: None,
                    message: format!(
                        "utility module `{module_path}` defines public type `{}` \
                         (matched utility pattern `{pattern}`)",
                        ty.name
                    ),
                    why: vec![
                        format!("module `{module_path}` matches utility pattern `{pattern}`"),
                        format!("public type `{}` (`{}`)", ty.name, ty.symbol),
                        "utility modules must hold only domain-free technical helpers; \
                         public types carry semantics that belong to a domain/feature module"
                            .into(),
                    ],
                    suggested_fix: Some(format!(
                        "move `{}` to a domain/feature module that owns the concept it \
                         represents; if it really is a domain-free helper type, demote it \
                         to private (utility modules can hold private types) or rename the \
                         module so it's no longer marked as utility in \
                         `paradigms.UT.utility_paths`",
                        ty.name
                    )),
                });
            }
        }
    }
    out
}

/// UT002 — utility module imports a forbidden feature/domain path.
///
/// For every `AirFile` whose `module_path` matches any pattern in
/// `utility_paths`, walk its `AirItem::Import` items. Fire when the import
/// path matches any pattern in `forbidden_imports`.
///
/// Severity: Fatal in both modes — a forbidden import declared by the user is
/// a structural violation, mirroring DG001 / BO001.
pub fn ut002(air: &AirWorkspace, section: &UtSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.utility_paths.is_empty() || section.forbidden_imports.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(utility_pattern) = section
                .utility_paths
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
                    .forbidden_imports
                    .iter()
                    .find(|pat| matches_pattern(pat, &imp.path))
                else {
                    continue;
                };
                out.push(Diagnostic {
                    rule_id: "UT002".to_string(),
                    severity: mode.elevate(Severity::Fatal),
                    span: imp.span.clone(),
                    concept: None,
                    message: format!(
                        "utility module `{module_path}` imports forbidden \
                         feature/domain path `{}`",
                        imp.path
                    ),
                    why: vec![
                        format!(
                            "importer `{module_path}` matches utility_paths pattern \
                             `{utility_pattern}`"
                        ),
                        format!(
                            "import `{}` matches forbidden_imports pattern \
                             `{forbidden_pattern}`",
                            imp.path
                        ),
                        "utility modules must hold only domain-free technical helpers; \
                         importing a feature/domain concept means the helper knows about \
                         semantics that belong to a domain/feature module"
                            .into(),
                    ],
                    suggested_fix: Some(format!(
                        "move the helper that needs `{}` out of the utility module and \
                         into the domain/feature module that owns the concept; if the \
                         dependency is legitimate, remove `{module_path}` from \
                         `paradigms.UT.utility_paths` (or narrow \
                         `paradigms.UT.forbidden_imports`) in `locus.lock`",
                        imp.path
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
        AIR_SCHEMA_VERSION, AirFile, AirImport, AirPackage, AirSpan, AirType, TypeKind, Visibility,
    };

    fn ty(name: &str, vis: Visibility) -> AirItem {
        AirItem::Type(AirType {
            kind: TypeKind::Struct,
            name: name.into(),
            symbol: format!("x::utils::{name}"),
            visibility: vis,
            fields: Vec::new(),
            variants: Vec::new(),
            derives: Vec::new(),
            attrs: Vec::new(),
            span: AirSpan::new("t.rs", 1, 1),
            doc: None,
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
            facts: Vec::new(),
        }
    }

    #[test]
    fn ut001_fires_on_public_type_in_utility_module() {
        let air = air_with_module("x::utils", vec![ty("Helper", Visibility::Public)]);
        let section = UtSection {
            utility_paths: vec!["x::utils::*".into()],
            ..Default::default()
        };
        let diags = ut001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "UT001");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("Helper"));
        assert!(diags[0].message.contains("x::utils"));
        assert!(diags[0].message.contains("x::utils::*"));
    }

    #[test]
    fn ut001_quiet_on_private_type_in_utility_module() {
        let air = air_with_module("x::utils", vec![ty("Helper", Visibility::Private)]);
        let section = UtSection {
            utility_paths: vec!["x::utils::*".into()],
            ..Default::default()
        };
        assert!(ut001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ut001_quiet_on_crate_visible_type_in_utility_module() {
        // `pub(crate)` is not full Public — utility modules are allowed to
        // hold crate-visible helpers; only the truly Public surface trips UT001.
        let air = air_with_module("x::utils", vec![ty("Helper", Visibility::Crate)]);
        let section = UtSection {
            utility_paths: vec!["x::utils::*".into()],
            ..Default::default()
        };
        assert!(ut001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ut001_quiet_on_public_type_in_non_matching_module() {
        let air = air_with_module("x::domain::user", vec![ty("User", Visibility::Public)]);
        let section = UtSection {
            utility_paths: vec!["x::utils::*".into()],
            ..Default::default()
        };
        assert!(ut001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ut001_silent_when_utility_paths_empty() {
        let air = air_with_module("x::utils", vec![ty("Helper", Visibility::Public)]);
        let section = UtSection::default();
        assert!(ut001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ut001_multiple_public_types_produce_multiple_diagnostics() {
        let air = air_with_module(
            "x::utils",
            vec![
                ty("Helper", Visibility::Public),
                ty("Adapter", Visibility::Public),
                ty("Internal", Visibility::Private), // not flagged
                ty("Bag", Visibility::Public),
            ],
        );
        let section = UtSection {
            utility_paths: vec!["x::utils::*".into()],
            ..Default::default()
        };
        let diags = ut001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 3);
        let names: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        assert!(names.iter().any(|m| m.contains("Helper")));
        assert!(names.iter().any(|m| m.contains("Adapter")));
        assert!(names.iter().any(|m| m.contains("Bag")));
        assert!(!names.iter().any(|m| m.contains("Internal")));
    }

    #[test]
    fn ut001_agent_strict_elevates_to_fatal() {
        let air = air_with_module("x::utils", vec![ty("Helper", Visibility::Public)]);
        let section = UtSection {
            utility_paths: vec!["x::utils::*".into()],
            ..Default::default()
        };
        let diags = ut001(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn ut001_matches_exact_module_path_too() {
        // Pattern `x::utils` (no `::*`) should match the exact module.
        let air = air_with_module("x::utils", vec![ty("Helper", Visibility::Public)]);
        let section = UtSection {
            utility_paths: vec!["x::utils".into()],
            ..Default::default()
        };
        let diags = ut001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
    }

    fn import(path: &str) -> AirItem {
        AirItem::Import(AirImport {
            path: path.into(),
            visibility: Visibility::Private,
            span: AirSpan::new("t.rs", 1, 1),
        })
    }

    #[test]
    fn ut002_fires_when_utility_file_imports_forbidden_path() {
        let air = air_with_module("x::utils", vec![import("crate::domain::user::User")]);
        let section = UtSection {
            utility_paths: vec!["x::utils::*".into()],
            forbidden_imports: vec!["crate::domain::*".into()],
        };
        let diags = ut002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "UT002");
        assert_eq!(diags[0].severity, Severity::Fatal);
        assert!(diags[0].concept.is_none());
        assert!(diags[0].message.contains("x::utils"));
        assert!(diags[0].message.contains("crate::domain::user::User"));
        assert!(
            diags[0].why.iter().any(|w| w.contains("x::utils::*")),
            "expected utility pattern in why; got {:?}",
            diags[0].why
        );
        assert!(
            diags[0].why.iter().any(|w| w.contains("crate::domain::*")),
            "expected forbidden pattern in why; got {:?}",
            diags[0].why
        );
        assert!(
            diags[0].why.iter().any(|w| w.contains("x::utils")),
            "expected importer module in why; got {:?}",
            diags[0].why
        );
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("crate::domain::user::User")),
            "expected import path in why; got {:?}",
            diags[0].why
        );
    }

    #[test]
    fn ut002_quiet_when_non_utility_file_imports_forbidden_path() {
        // Domain modules are allowed to import other domain things — only
        // *utility* modules should be domain-free.
        let air = air_with_module(
            "x::domain::orders",
            vec![import("crate::domain::user::User")],
        );
        let section = UtSection {
            utility_paths: vec!["x::utils::*".into()],
            forbidden_imports: vec!["crate::domain::*".into()],
        };
        assert!(ut002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ut002_quiet_when_utility_file_imports_non_forbidden_path() {
        let air = air_with_module("x::utils", vec![import("std::collections::HashMap")]);
        let section = UtSection {
            utility_paths: vec!["x::utils::*".into()],
            forbidden_imports: vec!["crate::domain::*".into()],
        };
        assert!(ut002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ut002_silent_when_forbidden_imports_empty() {
        let air = air_with_module("x::utils", vec![import("crate::domain::user::User")]);
        let section = UtSection {
            utility_paths: vec!["x::utils::*".into()],
            forbidden_imports: vec![],
        };
        assert!(ut002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ut002_silent_when_utility_paths_empty() {
        let air = air_with_module("x::utils", vec![import("crate::domain::user::User")]);
        let section = UtSection {
            utility_paths: vec![],
            forbidden_imports: vec!["crate::domain::*".into()],
        };
        assert!(ut002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ut002_silent_with_default_section() {
        let air = air_with_module("x::utils", vec![import("crate::domain::user::User")]);
        let section = UtSection::default();
        assert!(ut002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ut002_agent_strict_keeps_severity_fatal() {
        // UT002 is already Fatal in human mode; --agent-strict elevates but
        // can't go higher than Fatal — verify it stays Fatal.
        let air = air_with_module("x::utils", vec![import("crate::roles::Admin")]);
        let section = UtSection {
            utility_paths: vec!["x::utils::*".into()],
            forbidden_imports: vec!["crate::roles::*".into()],
        };
        let diags = ut002(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }
}
