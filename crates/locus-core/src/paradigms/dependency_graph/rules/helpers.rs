//! Shared helpers for DG rule implementations.
//!
//! `collect_crate_edges` builds the crate-level import edge set consumed by
//! DG002. `tarjan_sccs` / `strongconnect` / `TarjanState` implement Tarjan's
//! strongly-connected-components algorithm for cycle detection. `owning_feature`
//! / `path_in_public_api` are feature-boundary helpers shared by DG003/DG004.

use std::collections::BTreeMap;

use locus_air::{AirItem, AirSpan, AirWorkspace};

use super::super::lockfile_schema::{FeatureDefinition, matches_pattern};

#[derive(Debug, Clone)]
pub(super) struct EdgeEvidence {
    pub(super) file_path: String,
    pub(super) span: AirSpan,
    pub(super) import_path: String,
}

pub(super) fn collect_crate_edges(air: &AirWorkspace) -> BTreeMap<(String, String), EdgeEvidence> {
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

pub(super) fn tarjan_sccs(adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
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

pub(super) struct TarjanState {
    pub(super) index: usize,
    pub(super) indices: Vec<Option<usize>>,
    pub(super) lowlinks: Vec<usize>,
    pub(super) on_stack: Vec<bool>,
    pub(super) stack: Vec<usize>,
    pub(super) sccs: Vec<Vec<usize>>,
}

pub(super) fn strongconnect(v: usize, adj: &[Vec<usize>], st: &mut TarjanState) {
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

pub(super) fn owning_feature<'a>(
    features: &'a [FeatureDefinition],
    path: &str,
) -> Option<&'a FeatureDefinition> {
    features.iter().find(|f| matches_pattern(&f.module, path))
}

pub(super) fn path_in_public_api(feature: &FeatureDefinition, path: &str) -> bool {
    feature
        .public_api
        .iter()
        .any(|pat| matches_pattern(pat, path))
}
