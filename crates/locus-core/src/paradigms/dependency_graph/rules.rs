//! DG rules.
//!
//! Implemented:
//! - [`dg001`]: forbidden import (importer matches a forbidden edge's `from`
//!   pattern AND the imported path matches the `to` pattern)
//! - [`dg002`]: cross-crate 2-cycle (A imports B and B imports A)
//!
//! Future: DG002 generalised to N-cycles via Tarjan SCCs, DG003 (cross-feature
//! internals reach), DG004 (shared module reaching feature-specific symbol).

use std::collections::{BTreeMap, BTreeSet};

use locus_air::{AirItem, AirSpan, AirWorkspace};

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

/// DG002 — dependency cycle across crates (2-cycle).
///
/// Builds a crate-level edge set from every `AirImport`: each edge is
/// `(importer_crate, imported_crate)` plus a representative span. If both
/// `(A, B)` and `(B, A)` exist, the pair forms a cycle and DG002 fires.
///
/// Phase-2 scope: 2-cycles only. Longer cycles (A→B→C→A) require running
/// Tarjan's SCC on the edge set; that's a polish item once the simpler form
/// proves useful. The diagnostic is emitted twice per cycle — one for each
/// direction — so the user sees the violating import in each crate.
///
/// Always Fatal: a cycle is structural and breaks layered ownership.
pub fn dg002(air: &AirWorkspace, mode: CheckMode) -> Vec<Diagnostic> {
    let edges = collect_crate_edges(air);
    if edges.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    for ((a, b), evidence) in &edges {
        if seen.contains(&(a.clone(), b.clone())) {
            continue;
        }
        let Some(reverse) = edges.get(&(b.clone(), a.clone())) else {
            continue;
        };
        // Mark both directions reported so we don't process the (b, a) entry again.
        seen.insert((a.clone(), b.clone()));
        seen.insert((b.clone(), a.clone()));
        out.push(cycle_diagnostic(a, b, evidence, mode));
        out.push(cycle_diagnostic(b, a, reverse, mode));
    }
    out
}

#[derive(Debug, Clone)]
struct EdgeEvidence {
    file_path: String,
    span: AirSpan,
    import_path: String,
}

fn collect_crate_edges(air: &AirWorkspace) -> BTreeMap<(String, String), EdgeEvidence> {
    let mut edges: BTreeMap<(String, String), EdgeEvidence> = BTreeMap::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let importer = first_segment(module_path);
            if importer.is_empty() {
                continue;
            }
            for item in &file.items {
                let AirItem::Import(imp) = item else {
                    continue;
                };
                let imported = first_segment(&imp.path);
                if imported.is_empty() || imported == importer {
                    continue;
                }
                edges
                    .entry((importer.to_string(), imported.to_string()))
                    .or_insert_with(|| EdgeEvidence {
                        file_path: file.path.clone(),
                        span: imp.span.clone(),
                        import_path: imp.path.clone(),
                    });
            }
        }
    }
    edges
}

fn first_segment(path: &str) -> &str {
    path.split("::").next().unwrap_or("").trim()
}

fn cycle_diagnostic(
    importer: &str,
    imported: &str,
    evidence: &EdgeEvidence,
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
        rule_id: "DG002".to_string(),
        severity: mode.elevate(Severity::Fatal),
        span: evidence.span.clone(),
        concept: None,
        message: format!(
            "dependency cycle: `{importer}` imports `{}` while `{imported}` imports back",
            evidence.import_path
        ),
        why: vec![
            format!(
                "`{importer}` -> `{imported}` (via `{}`)",
                evidence.import_path
            ),
            format!("`{imported}` -> `{importer}` is also present"),
            format!("evidence import in `{}`", evidence.file_path),
        ],
        suggested_fix: Some(
            "break the cycle by extracting a shared trait/port crate, or restructure ownership \
             so one direction is implementation-side only and goes through a port"
                .into(),
        ),
    }
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

    // ---- DG002 ----

    type FileSpec<'a> = (&'a str, &'a str, Vec<&'a str>);
    type PkgSpec<'a> = (&'a str, Vec<FileSpec<'a>>);

    fn air_with_pkgs(pkgs: Vec<PkgSpec<'_>>) -> AirWorkspace {
        // Each pkg is (name, [(file_path, module_path, imports)]).
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: pkgs
                .into_iter()
                .map(|(name, files)| AirPackage {
                    name: name.into(),
                    version: "0".into(),
                    root_dir: "/".into(),
                    files: files
                        .into_iter()
                        .map(|(path, module, imports)| AirFile {
                            path: path.into(),
                            module_path: Some(module.into()),
                            items: imports.into_iter().map(import).collect(),
                            hints: Vec::new(),
                            parse_error: None,
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    #[test]
    fn dg002_fires_on_two_crate_cycle() {
        // a's file imports something in b; b's file imports something in a.
        let air = air_with_pkgs(vec![
            ("a", vec![("a/src/lib.rs", "a", vec!["b::Type1"])]),
            ("b", vec![("b/src/lib.rs", "b", vec!["a::Type2"])]),
        ]);
        let diags = dg002(&air, CheckMode::Human);
        assert_eq!(
            diags.len(),
            2,
            "one diagnostic per cycle direction; got {diags:?}"
        );
        for d in &diags {
            assert_eq!(d.rule_id, "DG002");
            assert_eq!(d.severity, Severity::Fatal);
        }
        let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        assert!(
            messages
                .iter()
                .any(|m| m.contains("`a` imports `b::Type1`"))
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("`b` imports `a::Type2`"))
        );
    }

    #[test]
    fn dg002_silent_when_only_one_direction() {
        let air = air_with_pkgs(vec![
            ("a", vec![("a/src/lib.rs", "a", vec!["b::Type"])]),
            ("b", vec![("b/src/lib.rs", "b", vec![])]),
        ]);
        assert!(dg002(&air, CheckMode::Human).is_empty());
    }

    #[test]
    fn dg002_ignores_intra_crate_self_loops() {
        // a's file imports a::other — same crate, not a cycle.
        let air = air_with_pkgs(vec![(
            "a",
            vec![("a/src/lib.rs", "a", vec!["a::other::Thing"])],
        )]);
        assert!(dg002(&air, CheckMode::Human).is_empty());
    }

    #[test]
    fn dg002_finds_multiple_cycles_independently() {
        let air = air_with_pkgs(vec![
            ("a", vec![("a/src/lib.rs", "a", vec!["b::T", "c::T"])]),
            ("b", vec![("b/src/lib.rs", "b", vec!["a::T"])]),
            ("c", vec![("c/src/lib.rs", "c", vec!["a::T"])]),
        ]);
        let diags = dg002(&air, CheckMode::Human);
        // Two separate 2-cycles (a<->b, a<->c) → 4 diagnostics total.
        assert_eq!(diags.len(), 4, "got {diags:?}");
    }

    #[test]
    fn dg002_does_not_double_report_same_cycle() {
        // Multiple imports in each direction shouldn't multiply diagnostics.
        let air = air_with_pkgs(vec![
            (
                "a",
                vec![("a/src/lib.rs", "a", vec!["b::T1", "b::T2", "b::T3"])],
            ),
            ("b", vec![("b/src/lib.rs", "b", vec!["a::U1", "a::U2"])]),
        ]);
        let diags = dg002(&air, CheckMode::Human);
        assert_eq!(diags.len(), 2, "one diag per direction; got {diags:?}");
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
