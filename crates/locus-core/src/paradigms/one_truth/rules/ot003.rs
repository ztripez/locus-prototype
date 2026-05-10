//! OT003 — boundary adapter leak.
//!
//! Fires when a function lives in a non-boundary file, isn't an accepted
//! converter, and has a parameter or return type that references an
//! accepted boundary type (by short name).
//!
//! "Boundary file" = any file in the workspace that defines an accepted
//! boundary symbol. Boundary code is allowed to use boundary types freely;
//! only domain/application code must convert at the edge.
//!
//! Always Fatal: boundary leaks are the headline OT violation per the spec.

use std::collections::{BTreeMap, BTreeSet};

use locus_air::{AirItem, AirWorkspace};

use super::super::lockfile_schema::OtSection;
use super::helpers::{file_of_symbol, type_text_references};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

pub fn ot003(air: &AirWorkspace, section: &OtSection, mode: CheckMode) -> Vec<Diagnostic> {
    let mut boundary_files: BTreeSet<String> = BTreeSet::new();
    let mut boundary_short_names: Vec<(String, String)> = Vec::new(); // (short, concept)
    for (concept_id, entry) in &section.concepts {
        for b in &entry.boundaries {
            if let Some(file_path) = file_of_symbol(air, &b.symbol) {
                boundary_files.insert(file_path);
            }
            if let Some(short) = b.symbol.rsplit("::").next() {
                boundary_short_names.push((short.to_string(), concept_id.clone()));
            }
        }
    }
    if boundary_short_names.is_empty() {
        return Vec::new();
    }
    let accepted_converters: BTreeSet<&str> = section
        .concepts
        .values()
        .flat_map(|e| e.converters.iter().map(|c| c.symbol.as_str()))
        .collect();

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            if boundary_files.contains(&file.path) {
                continue;
            }
            for item in &file.items {
                let AirItem::Function(f) = item else {
                    continue;
                };
                if accepted_converters.contains(f.symbol.as_str()) {
                    continue;
                }
                // Aggregate every boundary referenced in any signature slot,
                // emit one diagnostic per (function, boundary). Multiple
                // diagnostics for the same boundary in different params would
                // be noise; one is enough.
                let mut hits: BTreeMap<String, String> = BTreeMap::new(); // short → concept
                for (_, ty_text) in &f.params {
                    for (short, concept) in &boundary_short_names {
                        if type_text_references(ty_text, short) {
                            hits.entry(short.clone()).or_insert_with(|| concept.clone());
                        }
                    }
                }
                if let Some(ret) = &f.return_type {
                    for (short, concept) in &boundary_short_names {
                        if type_text_references(ret, short) {
                            hits.entry(short.clone()).or_insert_with(|| concept.clone());
                        }
                    }
                }
                for (short, concept) in hits {
                    out.push(Diagnostic {
                        rule_id: "OT003".to_string(),
                        severity: mode.elevate(Severity::Fatal),
                        span: f.span.clone(),
                        concept: Some(concept.clone()),
                        message: format!(
                            "function `{}` exposes boundary type `{}` in its signature; \
                             boundary types must be converted before crossing into \
                             domain/application code",
                            f.symbol, short
                        ),
                        why: vec![
                            format!("file `{}` is not a boundary file (no accepted boundary lives here)", f.span.file),
                            format!("`{short}` is the accepted boundary for concept `{concept}`"),
                            format!("`{}` is not an accepted converter", f.symbol),
                        ],
                        suggested_fix: Some(format!(
                            "convert `{short}` at the edge: \
                             `let domain = canonical_for_{concept}::try_from(value)?;`, \
                             then take the canonical type in this signature instead"
                        )),
                    });
                }
            }
        }
    }
    out
}
