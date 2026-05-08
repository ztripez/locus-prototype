//! DG rules.
//!
//! Implemented:
//! - [`dg001`]: forbidden import (importer matches a forbidden edge's `from`
//!   pattern AND the imported path matches the `to` pattern)
//! - [`dg002`]: dependency cycle of any size (Tarjan SCC over crate-level
//!   import edges)
//! - [`dg003`]: cross-feature internals reach (importer in feature A, import
//!   target in feature B's internals — i.e. not in B's public API)
//! - [`dg004`]: shared module reaching feature-specific code (a `shared_paths`
//!   module imports a path that belongs to any feature)

use std::collections::{BTreeMap, BTreeSet};

use locus_air::{AirItem, AirSpan, AirWorkspace};

use super::lockfile_schema::{DgSection, FeatureDefinition, matches_pattern};
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

/// DG002 — dependency cycle across crates.
///
/// Builds a crate-level edge set from every `AirImport`, runs Tarjan's
/// strongly-connected-components algorithm over the directed graph, and
/// emits one Fatal diagnostic per edge that participates in any SCC of
/// size ≥ 2. Catches 2-cycles (`A ↔ B`), 3-cycles (`A → B → C → A`), and
/// arbitrarily large cycles uniformly — the SCC partition handles all of
/// them in a single pass.
///
/// One diagnostic per cycle-participating edge mirrors DG001's per-import
/// granularity, so the user sees a span in each violating import.
///
/// Always Fatal: a cycle is structural and breaks layered ownership.
pub fn dg002(air: &AirWorkspace, mode: CheckMode) -> Vec<Diagnostic> {
    let edges = collect_crate_edges(air);
    if edges.is_empty() {
        return Vec::new();
    }

    // Index nodes (crate names) and build adjacency lists for Tarjan.
    let mut nodes: Vec<String> = edges
        .keys()
        .flat_map(|(a, b)| [a.clone(), b.clone()])
        .collect();
    nodes.sort();
    nodes.dedup();
    let node_idx: BTreeMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i))
        .collect();

    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (a, b) in edges.keys() {
        let ai = node_idx[a.as_str()];
        let bi = node_idx[b.as_str()];
        adj[ai].push(bi);
    }

    let sccs = tarjan_sccs(&adj);

    let mut out = Vec::new();
    for scc in sccs {
        if scc.len() < 2 {
            continue; // single nodes — no cycle (we filter self-loops in collect_crate_edges)
        }
        let scc_set: BTreeSet<usize> = scc.iter().copied().collect();
        let mut members: Vec<&str> = scc.iter().map(|&i| nodes[i].as_str()).collect();
        members.sort();

        for ((a, b), evidence) in &edges {
            let ai = node_idx[a.as_str()];
            let bi = node_idx[b.as_str()];
            if !scc_set.contains(&ai) || !scc_set.contains(&bi) {
                continue;
            }
            out.push(cycle_diagnostic(a, b, evidence, &members, mode));
        }
    }
    out
}

/// Tarjan's strongly-connected-components algorithm. Returns each SCC as a
/// list of node indices. SCCs are returned in reverse topological order
/// (children before parents), but we don't rely on that — callers filter
/// by size and iterate.
fn tarjan_sccs(adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let n = adj.len();
    let mut state = TarjanState {
        index: 0,
        indices: vec![None; n],
        lowlinks: vec![0; n],
        on_stack: vec![false; n],
        stack: Vec::new(),
        sccs: Vec::new(),
    };
    for v in 0..n {
        if state.indices[v].is_none() {
            strongconnect(v, adj, &mut state);
        }
    }
    state.sccs
}

struct TarjanState {
    index: usize,
    indices: Vec<Option<usize>>,
    lowlinks: Vec<usize>,
    on_stack: Vec<bool>,
    stack: Vec<usize>,
    sccs: Vec<Vec<usize>>,
}

fn strongconnect(v: usize, adj: &[Vec<usize>], st: &mut TarjanState) {
    st.indices[v] = Some(st.index);
    st.lowlinks[v] = st.index;
    st.index += 1;
    st.stack.push(v);
    st.on_stack[v] = true;

    // Clone the adjacency snapshot so we don't keep a borrow across recursion.
    let succs = adj[v].clone();
    for w in succs {
        if st.indices[w].is_none() {
            strongconnect(w, adj, st);
            st.lowlinks[v] = st.lowlinks[v].min(st.lowlinks[w]);
        } else if st.on_stack[w] {
            st.lowlinks[v] = st.lowlinks[v].min(st.indices[w].expect("on_stack implies indexed"));
        }
    }

    if Some(st.lowlinks[v]) == st.indices[v] {
        let mut scc = Vec::new();
        loop {
            let w = st.stack.pop().expect("stack non-empty during SCC pop");
            st.on_stack[w] = false;
            scc.push(w);
            if w == v {
                break;
            }
        }
        st.sccs.push(scc);
    }
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

/// DG003 — cross-feature internals reach.
///
/// For every `AirImport`, fire when:
/// - the importer's `module_path` matches some feature A's `module` pattern;
/// - the import path matches some feature B's `module` pattern;
/// - A and B are different features (intra-feature imports are always fine);
/// - the import path does NOT match any of B's `public_api` patterns.
///
/// "Owning feature" of an import target is found by first-match against the
/// `features` list. Overlapping `module` patterns are user error; documented
/// but not actively rejected.
///
/// Always Fatal: feature isolation is the user's declared invariant.
pub fn dg003(air: &AirWorkspace, section: &DgSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.features.len() < 2 {
        // DG003 needs at least two features to identify a cross-feature edge.
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(importer_feature) = owning_feature(&section.features, module_path) else {
                continue;
            };
            for item in &file.items {
                let AirItem::Import(imp) = item else {
                    continue;
                };
                let Some(target_feature) = owning_feature(&section.features, &imp.path) else {
                    continue;
                };
                if std::ptr::eq(importer_feature, target_feature) {
                    continue; // intra-feature import is fine
                }
                if path_in_public_api(target_feature, &imp.path) {
                    continue; // public-API surface is the legal boundary
                }
                out.push(Diagnostic {
                    rule_id: "DG003".to_string(),
                    severity: mode.elevate(Severity::Fatal),
                    span: imp.span.clone(),
                    concept: None,
                    message: format!(
                        "feature `{importer}` reaches into `{target}` internals via `{}`",
                        imp.path,
                        importer = importer_feature.name,
                        target = target_feature.name,
                    ),
                    why: vec![
                        format!(
                            "importer `{module_path}` belongs to feature `{}`",
                            importer_feature.name
                        ),
                        format!(
                            "import `{}` belongs to feature `{}` but is not in its public API",
                            imp.path, target_feature.name
                        ),
                        if target_feature.public_api.is_empty() {
                            format!(
                                "feature `{}` has no public_api defined",
                                target_feature.name
                            )
                        } else {
                            format!(
                                "public_api patterns: {}",
                                target_feature
                                    .public_api
                                    .iter()
                                    .map(|p| format!("`{p}`"))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )
                        },
                    ],
                    suggested_fix: Some(format!(
                        "import through `{}`'s public API, or expand its public_api list \
                         to include `{}` if this access is intentional",
                        target_feature.name, imp.path
                    )),
                });
            }
        }
    }
    out
}

/// DG004 — shared module reaching feature-specific code.
///
/// A module is "shared" if its `module_path` matches any of `shared_paths`.
/// Shared modules must not depend on any feature: dependency direction must
/// stay feature → shared, never shared → feature. Fires when a shared
/// module's import path matches some feature's `module` pattern.
///
/// Always Fatal.
pub fn dg004(air: &AirWorkspace, section: &DgSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.shared_paths.is_empty() || section.features.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(shared_pattern) = section
                .shared_paths
                .iter()
                .find(|pat| matches_pattern(pat, module_path))
            else {
                continue;
            };
            for item in &file.items {
                let AirItem::Import(imp) = item else {
                    continue;
                };
                let Some(target_feature) = owning_feature(&section.features, &imp.path) else {
                    continue;
                };
                out.push(Diagnostic {
                    rule_id: "DG004".to_string(),
                    severity: mode.elevate(Severity::Fatal),
                    span: imp.span.clone(),
                    concept: None,
                    message: format!(
                        "shared module `{module_path}` imports feature `{}` via `{}`",
                        target_feature.name, imp.path
                    ),
                    why: vec![
                        format!("`{module_path}` matches shared_paths pattern `{shared_pattern}`"),
                        format!(
                            "`{}` belongs to feature `{}` (pattern `{}`)",
                            imp.path, target_feature.name, target_feature.module
                        ),
                        "shared infrastructure must not depend on any feature".into(),
                    ],
                    suggested_fix: Some(
                        "invert the dependency: the feature should depend on the shared module \
                         (move the call into the feature, or extract the shared module's \
                         responsibility into a port the feature provides)"
                            .into(),
                    ),
                });
            }
        }
    }
    out
}

/// Find the first feature whose `module` pattern matches `path`. Returns
/// `None` when the path doesn't belong to any declared feature.
fn owning_feature<'a>(
    features: &'a [FeatureDefinition],
    path: &str,
) -> Option<&'a FeatureDefinition> {
    features.iter().find(|f| matches_pattern(&f.module, path))
}

fn path_in_public_api(feature: &FeatureDefinition, path: &str) -> bool {
    feature
        .public_api
        .iter()
        .any(|pat| matches_pattern(pat, path))
}

fn cycle_diagnostic(
    importer: &str,
    imported: &str,
    evidence: &EdgeEvidence,
    cycle_members: &[&str],
    mode: CheckMode,
) -> Diagnostic {
    let members_label = if cycle_members.len() == 2 {
        format!("`{}` ↔ `{}`", cycle_members[0], cycle_members[1])
    } else {
        let joined = cycle_members
            .iter()
            .map(|m| format!("`{m}`"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("[{joined}]")
    };
    Diagnostic {
        rule_id: "DG002".to_string(),
        severity: mode.elevate(Severity::Fatal),
        span: evidence.span.clone(),
        concept: None,
        message: format!(
            "dependency cycle: `{importer}` -> `{}` participates in cycle {members_label}",
            evidence.import_path
        ),
        why: vec![
            format!(
                "`{importer}` -> `{imported}` (via `{}`)",
                evidence.import_path
            ),
            format!("cycle participants: {members_label}"),
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
            path_segments: Vec::new(),
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
            facts: Vec::new(),
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
            ..DgSection::default()
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
            ..DgSection::default()
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
            ..DgSection::default()
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
            ..DgSection::default()
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
                            line_count: 1,
                        })
                        .collect(),
                })
                .collect(),
            facts: Vec::new(),
        }
    }

    #[test]
    fn dg002_fires_on_two_crate_cycle() {
        let air = air_with_pkgs(vec![
            ("a", vec![("a/src/lib.rs", "a", vec!["b::Type1"])]),
            ("b", vec![("b/src/lib.rs", "b", vec!["a::Type2"])]),
        ]);
        let diags = dg002(&air, CheckMode::Human);
        assert_eq!(diags.len(), 2, "one diag per edge in SCC; got {diags:?}");
        for d in &diags {
            assert_eq!(d.rule_id, "DG002");
            assert_eq!(d.severity, Severity::Fatal);
            // 2-cycle uses ↔ shorthand in the cycle label.
            assert!(
                d.message.contains("`a` ↔ `b`") || d.message.contains("`b` ↔ `a`"),
                "expected ↔ label for 2-cycle; got `{}`",
                d.message
            );
        }
        let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        assert!(messages.iter().any(|m| m.contains("`a` -> `b::Type1`")));
        assert!(messages.iter().any(|m| m.contains("`b` -> `a::Type2`")));
    }

    #[test]
    fn dg002_fires_on_three_cycle() {
        // a -> b -> c -> a, no shortcut edges.
        let air = air_with_pkgs(vec![
            ("a", vec![("a/src/lib.rs", "a", vec!["b::T"])]),
            ("b", vec![("b/src/lib.rs", "b", vec!["c::T"])]),
            ("c", vec![("c/src/lib.rs", "c", vec!["a::T"])]),
        ]);
        let diags = dg002(&air, CheckMode::Human);
        assert_eq!(
            diags.len(),
            3,
            "3-cycle has 3 edges, 3 diagnostics; got {diags:?}"
        );
        for d in &diags {
            assert!(d.message.contains("`a`"));
            assert!(d.message.contains("`b`"));
            assert!(d.message.contains("`c`"));
        }
    }

    // ---- DG003 ----

    fn feature(name: &str, module: &str, public_api: &[&str]) -> FeatureDefinition {
        FeatureDefinition {
            name: name.into(),
            module: module.into(),
            public_api: public_api.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    #[test]
    fn dg003_fires_on_cross_feature_internals_reach() {
        let air = air_with_pkgs(vec![(
            "ethics",
            vec![(
                "ethics/src/eval.rs",
                "ethics::eval",
                vec!["anatom::morals::MoralAct"],
            )],
        )]);
        let section = DgSection {
            features: vec![
                feature("anatom", "anatom::*", &["anatom::api::*"]),
                feature("ethics", "ethics::*", &[]),
            ],
            ..DgSection::default()
        };
        let diags = dg003(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "DG003");
        assert_eq!(diags[0].severity, Severity::Fatal);
        assert!(diags[0].message.contains("`ethics`"));
        assert!(diags[0].message.contains("`anatom`"));
        assert!(diags[0].message.contains("MoralAct"));
    }

    #[test]
    fn dg003_quiet_when_target_is_in_public_api() {
        let air = air_with_pkgs(vec![(
            "ethics",
            vec![(
                "ethics/src/eval.rs",
                "ethics::eval",
                vec!["anatom::api::evaluate"],
            )],
        )]);
        let section = DgSection {
            features: vec![
                feature("anatom", "anatom::*", &["anatom::api::*"]),
                feature("ethics", "ethics::*", &[]),
            ],
            ..DgSection::default()
        };
        assert!(dg003(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn dg003_quiet_for_intra_feature_imports() {
        let air = air_with_pkgs(vec![(
            "anatom",
            vec![(
                "anatom/src/internal.rs",
                "anatom::internal",
                vec!["anatom::morals::MoralAct"],
            )],
        )]);
        let section = DgSection {
            features: vec![
                feature("anatom", "anatom::*", &["anatom::api::*"]),
                feature("ethics", "ethics::*", &[]),
            ],
            ..DgSection::default()
        };
        assert!(dg003(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn dg003_silent_when_under_two_features_defined() {
        let air = air_with_pkgs(vec![("x", vec![("x/src/lib.rs", "x", vec!["y::Foo"])])]);
        let section = DgSection {
            features: vec![feature("x", "x::*", &[])],
            ..DgSection::default()
        };
        assert!(dg003(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn dg003_quiet_when_importer_is_not_a_feature() {
        let air = air_with_pkgs(vec![(
            "scripts",
            vec![(
                "scripts/src/main.rs",
                "scripts::main",
                vec!["anatom::morals::MoralAct"],
            )],
        )]);
        let section = DgSection {
            features: vec![
                feature("anatom", "anatom::*", &[]),
                feature("ethics", "ethics::*", &[]),
            ],
            ..DgSection::default()
        };
        assert!(dg003(&air, &section, CheckMode::Human).is_empty());
    }

    // ---- DG004 ----

    #[test]
    fn dg004_fires_on_shared_to_feature_import() {
        let air = air_with_pkgs(vec![(
            "core",
            vec![(
                "core/src/util.rs",
                "core::util",
                vec!["anatom::types::Anatom"],
            )],
        )]);
        let section = DgSection {
            features: vec![feature("anatom", "anatom::*", &["anatom::api::*"])],
            shared_paths: vec!["core::*".into()],
            ..DgSection::default()
        };
        let diags = dg004(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "DG004");
        assert_eq!(diags[0].severity, Severity::Fatal);
        assert!(diags[0].message.contains("core::util"));
        assert!(diags[0].message.contains("anatom"));
    }

    #[test]
    fn dg004_quiet_when_shared_imports_non_feature() {
        let air = air_with_pkgs(vec![(
            "core",
            vec![("core/src/util.rs", "core::util", vec!["std::fmt::Debug"])],
        )]);
        let section = DgSection {
            features: vec![feature("anatom", "anatom::*", &[])],
            shared_paths: vec!["core::*".into()],
            ..DgSection::default()
        };
        assert!(dg004(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn dg004_quiet_when_importer_not_shared() {
        let air = air_with_pkgs(vec![(
            "anatom",
            vec![("anatom/src/lib.rs", "anatom", vec!["other_feature::Thing"])],
        )]);
        let section = DgSection {
            features: vec![
                feature("other_feature", "other_feature::*", &[]),
                feature("anatom", "anatom::*", &[]),
            ],
            shared_paths: vec!["core::*".into()],
            ..DgSection::default()
        };
        assert!(dg004(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn dg004_silent_without_shared_paths() {
        let air = air_with_pkgs(vec![(
            "anywhere",
            vec![(
                "anywhere/src/lib.rs",
                "anywhere",
                vec!["anatom::types::Anatom"],
            )],
        )]);
        let section = DgSection {
            features: vec![feature("anatom", "anatom::*", &[])],
            shared_paths: vec![],
            ..DgSection::default()
        };
        assert!(dg004(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn dg002_treats_disjoint_sccs_independently() {
        let air = air_with_pkgs(vec![
            ("a", vec![("a/src/lib.rs", "a", vec!["b::T"])]),
            ("b", vec![("b/src/lib.rs", "b", vec!["a::T"])]),
            ("c", vec![("c/src/lib.rs", "c", vec!["d::T"])]),
            ("d", vec![("d/src/lib.rs", "d", vec!["c::T"])]),
        ]);
        let diags = dg002(&air, CheckMode::Human);
        assert_eq!(
            diags.len(),
            4,
            "two disjoint 2-cycles → 4 diagnostics; got {diags:?}"
        );
        let ab = diags
            .iter()
            .filter(|d| d.message.contains("`a` ↔ `b`") || d.message.contains("`b` ↔ `a`"))
            .count();
        let cd = diags
            .iter()
            .filter(|d| d.message.contains("`c` ↔ `d`") || d.message.contains("`d` ↔ `c`"))
            .count();
        assert_eq!(ab, 2);
        assert_eq!(cd, 2);
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
            ..DgSection::default()
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
