//! TA rule implementations.
//!
//! Implemented:
//! - [`ta001`]: test module defines a public domain-shaped type. Test
//!   fixtures duplicating domain concepts as public types is the "we made our
//!   own `User` struct in tests" pattern the spec calls out — domain truth
//!   should live on the canonical path, not in a test-local public clone.
//!
//! Mirrors UT001 in shape (lockfile-driven module pattern match, fires on
//! public types) but with a different fix narrative: demote to non-`pub`,
//! lift to a real production module, or accept as a shared fixture surface
//! (future TA mechanism).

use locus_air::{AirItem, AirWorkspace, Visibility};

use super::lockfile_schema::{TaSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// TA001 — test module defines a public domain-shaped type.
///
/// For every `AirFile` whose `module_path` matches any pattern in
/// `test_paths`, fire one diagnostic per public `AirItem::Type`.
///
/// Severity: Warning by default; Fatal under `--agent-strict`. Test modules
/// can legitimately hold private fixture types, so the structural fail-fast
/// tier isn't a fit — a public type is the heuristic signal that domain
/// concepts are being shadowed in test code.
pub fn ta001(air: &AirWorkspace, section: &TaSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.test_paths.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(pattern) = section
                .test_paths
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
                    rule_id: "TA001".to_string(),
                    severity: mode.elevate(Severity::Warning),
                    span: ty.span.clone(),
                    concept: None,
                    message: format!(
                        "test module `{module_path}` defines public type `{}` \
                         (matched test pattern `{pattern}`)",
                        ty.name
                    ),
                    why: vec![
                        format!("module `{module_path}` matches test pattern `{pattern}`"),
                        format!(
                            "public type `{}` (`{}`, visibility `{:?}`)",
                            ty.name, ty.symbol, ty.visibility
                        ),
                        "test modules must not create new domain truth; a public type \
                         in test code is typically a shadow of a domain concept that \
                         should live on the canonical production path"
                            .into(),
                    ],
                    suggested_fix: Some(format!(
                        "demote `{}` to non-`pub` if it's only used inside this test \
                         module; or move it out of the test module if it's actually \
                         shared production code; or accept this test module as a \
                         legitimate public-fixture surface (future TA mechanism)",
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
            symbol: format!("x::tests::{name}"),
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
    fn ta001_fires_on_public_type_in_test_module() {
        let air = air_with_module("x::tests", vec![ty("User", Visibility::Public)]);
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
        };
        let diags = ta001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "TA001");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("User"));
        assert!(diags[0].message.contains("x::tests"));
        assert!(diags[0].message.contains("x::tests::*"));
    }

    #[test]
    fn ta001_quiet_on_private_type_in_test_module() {
        let air = air_with_module("x::tests", vec![ty("Fixture", Visibility::Private)]);
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
        };
        assert!(ta001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ta001_quiet_on_public_type_in_non_matching_module() {
        let air = air_with_module("x::domain::user", vec![ty("User", Visibility::Public)]);
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
        };
        assert!(ta001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ta001_silent_when_test_paths_empty() {
        let air = air_with_module("x::tests", vec![ty("User", Visibility::Public)]);
        let section = TaSection::default();
        assert!(ta001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ta001_multiple_public_types_produce_multiple_diagnostics() {
        let air = air_with_module(
            "x::tests",
            vec![
                ty("User", Visibility::Public),
                ty("Order", Visibility::Public),
                ty("Internal", Visibility::Private), // not flagged
                ty("Account", Visibility::Public),
            ],
        );
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
        };
        let diags = ta001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 3);
        let names: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        assert!(names.iter().any(|m| m.contains("User")));
        assert!(names.iter().any(|m| m.contains("Order")));
        assert!(names.iter().any(|m| m.contains("Account")));
        assert!(!names.iter().any(|m| m.contains("Internal")));
    }

    #[test]
    fn ta001_agent_strict_elevates_to_fatal() {
        let air = air_with_module("x::tests", vec![ty("User", Visibility::Public)]);
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
        };
        let diags = ta001(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }
}
