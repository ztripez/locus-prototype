//! OT006 — unregistered conversion between accepted endpoints.
//!
//! Fires when an `AirConversion`'s endpoints are both lockfile-accepted
//! (canonical or boundary) but the conversion symbol itself isn't recorded
//! under that concept's `converters`. This is the "agent added a new mapper"
//! case after `locus init` has been run: the lockfile encodes which
//! conversions are sanctioned; anything else is a candidate fork.
//!
//! Severity: Warning by default; Fatal under `--agent-strict`.

use std::collections::{BTreeMap, BTreeSet};

use locus_air::{AirItem, AirWorkspace};

use super::super::lockfile_schema::OtSection;
use super::helpers::lookup_concept;
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

pub fn ot006(air: &AirWorkspace, section: &OtSection, mode: CheckMode) -> Vec<Diagnostic> {
    // Build a per-concept (accepted-symbol, accepted-converter-symbol) map
    // upfront so the per-conversion lookup is cheap.
    let mut concept_for_symbol: BTreeMap<String, String> = BTreeMap::new();
    let mut accepted_converter_symbols: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (concept_id, entry) in &section.concepts {
        concept_for_symbol.insert(entry.canonical.symbol.clone(), concept_id.clone());
        for b in &entry.boundaries {
            concept_for_symbol.insert(b.symbol.clone(), concept_id.clone());
        }
        let set: BTreeSet<String> = entry.converters.iter().map(|c| c.symbol.clone()).collect();
        accepted_converter_symbols.insert(concept_id.clone(), set);
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Conversion(c) = item else {
                    continue;
                };
                let Some(from_concept) = lookup_concept(&concept_for_symbol, &c.from) else {
                    continue;
                };
                let Some(to_concept) = lookup_concept(&concept_for_symbol, &c.to) else {
                    continue;
                };
                if from_concept != to_concept {
                    // Adapter-to-adapter or cross-concept — that's OT007 territory,
                    // not OT006. OT006 only flags missing acceptance within one concept.
                    continue;
                }
                let accepted = accepted_converter_symbols
                    .get(from_concept)
                    .is_some_and(|set| set.contains(&c.symbol));
                if accepted {
                    continue;
                }
                out.push(Diagnostic {
                    rule_id: "OT006".to_string(),
                    severity: mode.elevate(Severity::Warning),
                    span: c.span.clone(),
                    concept: Some(from_concept.clone()),
                    message: format!(
                        "`{}` converts between accepted symbols of concept `{}` \
                         but is not recorded as an accepted converter",
                        c.symbol, from_concept
                    ),
                    why: vec![
                        format!("from `{}` (accepted)", c.from),
                        format!("to `{}` (accepted)", c.to),
                        format!("conversion symbol `{}` not in lockfile", c.symbol),
                    ],
                    suggested_fix: Some(
                        "rerun `locus init` to refresh the lockfile, or add the \
                         converter symbol manually under the concept's `converters` list"
                            .to_string(),
                    ),
                });
            }
        }
    }
    out
}
