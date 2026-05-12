//! DG rules.
//!
//! DG001–DG004 all migrated to `RuleDefinition` in P2/P4 (#71).

pub mod dg001;
pub mod dg002;
pub mod dg003;
pub mod dg004;

use std::collections::BTreeMap;

use locus_air::{AirItem, AirSpan, AirWorkspace};

use super::lockfile_schema::{FeatureDefinition, matches_pattern};

#[derive(Debug, Clone)]
pub(super) struct EdgeEvidence {
    pub(crate) file_path: String,
    pub(crate) span: AirSpan,
    pub(crate) import_path: String,
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

#[cfg(test)]
#[path = "../rules_tests.rs"]
mod rules_tests;
