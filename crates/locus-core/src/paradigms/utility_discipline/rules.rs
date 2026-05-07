//! UT rules.
//!
//! Implemented:
//! - [`ut001`]: utility module defines a public type. A "utility module" by
//!   definition holds domain-free technical helpers; defining a public *type*
//!   in one is a smell because types carry semantics, and semantics belong to
//!   a domain/feature module.

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

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{
        AIR_SCHEMA_VERSION, AirFile, AirPackage, AirSpan, AirType, TypeKind, Visibility,
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
        };
        assert!(ut001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ut001_quiet_on_public_type_in_non_matching_module() {
        let air = air_with_module("x::domain::user", vec![ty("User", Visibility::Public)]);
        let section = UtSection {
            utility_paths: vec!["x::utils::*".into()],
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
        };
        let diags = ut001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
    }
}
