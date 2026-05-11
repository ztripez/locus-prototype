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

fn dg001_diagnostic(
    module_path: &str,
    imp: &locus_air::AirImport,
    edge: &super::lockfile_schema::ForbiddenEdge,
    mode: CheckMode,
) -> Diagnostic {
    let mut why = vec![
        format!("importer `{module_path}` matches `from = {}`", edge.from),
        format!("import `{}` matches `to = {}`", imp.path, edge.to),
    ];
    if let Some(reason) = &edge.reason {
        why.push(format!("reason: {reason}"));
    }
    Diagnostic {
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
    }
}

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
                    out.push(dg001_diagnostic(module_path, imp, edge, mode));
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

fn dg003_why(
    module_path: &str,
    imp: &locus_air::AirImport,
    importer_feature: &FeatureDefinition,
    target_feature: &FeatureDefinition,
) -> Vec<String> {
    vec![
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
    ]
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
                    why: dg003_why(module_path, imp, importer_feature, target_feature),
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

fn dg004_diagnostic(
    module_path: &str,
    imp: &locus_air::AirImport,
    shared_pattern: &str,
    target_feature: &FeatureDefinition,
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
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
    }
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
                out.push(dg004_diagnostic(
                    module_path,
                    imp,
                    shared_pattern,
                    target_feature,
                    mode,
                ));
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
#[path = "rules_tests.rs"]
mod rules_tests;
