//! OT005 — missing converter for an accepted boundary.
//!
//! Fires when a concept has accepted boundaries but no accepted converter
//! mentions a given boundary (in either direction). The spec eventually wants
//! this to track inbound vs outbound directions; for Phase 2 we only require
//! at least one converter touching the boundary.
//!
//! Always Fatal: a boundary with no converter is a dead end — boundary data
//! either can't reach the canonical or can't leave it.

use locus_air::{AirSpan, AirWorkspace};

use super::super::lockfile_schema::OtSection;
use super::helpers::{short_name, span_of_symbol};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

pub fn ot005(air: &AirWorkspace, section: &OtSection, mode: CheckMode) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (concept_id, entry) in &section.concepts {
        for boundary in &entry.boundaries {
            let short = short_name(&boundary.symbol);
            let has_converter = entry
                .converters
                .iter()
                .any(|c| short_name(&c.from) == short || short_name(&c.to) == short);
            if has_converter {
                continue;
            }
            let span = span_of_symbol(air, &boundary.symbol)
                .unwrap_or_else(|| AirSpan::new(boundary.symbol.clone(), 1, 1));
            out.push(Diagnostic {
                rule_id: "OT005".to_string(),
                severity: mode.elevate(Severity::Fatal),
                span,
                concept: Some(concept_id.clone()),
                message: format!(
                    "boundary `{}` (concept `{concept_id}`) has no accepted converter \
                     to/from the canonical",
                    boundary.symbol
                ),
                why: vec![
                    format!("canonical: `{}`", entry.canonical.symbol),
                    format!(
                        "no entry under `paradigms.OT.concepts.{concept_id}.converters` \
                         mentions `{short}` on either side"
                    ),
                ],
                suggested_fix: Some(format!(
                    "add an `impl TryFrom<{short}> for {}` (or its inverse) and rerun \
                     `locus init`; alternatively remove the boundary acceptance if it's \
                     no longer needed",
                    short_name(&entry.canonical.symbol),
                )),
            });
        }
    }
    out
}
