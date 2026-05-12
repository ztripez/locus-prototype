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
use crate::diagnostics::{CheckMode, Severity};
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{FindingIdMinter, ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

pub struct Ot003Rule;

pub static OT003_RULE: Ot003Rule = Ot003Rule;

const OT003_ID: RuleId = RuleId::new("OT003");
const OT_PARADIGM: ParadigmId = ParadigmId::new("OT");

impl RuleDefinition for Ot003Rule {
    fn id(&self) -> RuleId {
        OT003_ID
    }
    fn paradigm(&self) -> ParadigmId {
        OT_PARADIGM
    }
    fn title(&self) -> &'static str {
        "boundary adapter leak"
    }
    fn default_severity(&self) -> Severity {
        Severity::Fatal
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        let section: OtSection = ctx.lockfile.paradigm_section("OT").unwrap_or_default();
        produce_findings(ctx.air, &section, ctx.mode, ctx.finding_ids)
    }
}

/// Collect the boundary context for OT003:
/// - boundary_files: set of file paths that define accepted boundary symbols
/// - boundary_short_names: list of (short_name, concept_id) pairs
fn collect_ot003_boundaries(
    air: &AirWorkspace,
    section: &OtSection,
) -> (BTreeSet<String>, Vec<(String, String)>) {
    let mut boundary_files: BTreeSet<String> = BTreeSet::new();
    let mut boundary_short_names: Vec<(String, String)> = Vec::new();
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
    (boundary_files, boundary_short_names)
}

/// Scan one function for OT003 hits; push findings into `out`.
fn ot003_scan_function(
    f: &locus_air::AirFunction,
    boundary_short_names: &[(String, String)],
    accepted_converters: &BTreeSet<&str>,
    mode: CheckMode,
    finding_ids: &FindingIdMinter,
    out: &mut Vec<RuleFinding>,
) {
    if accepted_converters.contains(f.symbol.as_str()) {
        return;
    }
    // Aggregate every boundary referenced in any signature slot; one
    // finding per (function, boundary) is enough.
    let mut hits: BTreeMap<String, String> = BTreeMap::new();
    for (_, ty_text) in &f.params {
        for (short, concept) in boundary_short_names {
            if type_text_references(ty_text, short) {
                hits.entry(short.clone()).or_insert_with(|| concept.clone());
            }
        }
    }
    if let Some(ret) = &f.return_type {
        for (short, concept) in boundary_short_names {
            if type_text_references(ret, short) {
                hits.entry(short.clone()).or_insert_with(|| concept.clone());
            }
        }
    }
    for (short, concept) in hits {
        out.push(ot003_finding(f, &short, &concept, mode, finding_ids));
    }
}

pub(crate) fn produce_findings(
    air: &AirWorkspace,
    section: &OtSection,
    mode: CheckMode,
    finding_ids: &FindingIdMinter,
) -> Vec<RuleFinding> {
    let (boundary_files, boundary_short_names) = collect_ot003_boundaries(air, section);
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
                ot003_scan_function(
                    f,
                    &boundary_short_names,
                    &accepted_converters,
                    mode,
                    finding_ids,
                    &mut out,
                );
            }
        }
    }
    out
}

fn ot003_finding(
    f: &locus_air::AirFunction,
    short: &str,
    concept: &str,
    mode: CheckMode,
    finding_ids: &FindingIdMinter,
) -> RuleFinding {
    RuleFinding {
        id: finding_ids.next(),
        source: FindingSource::RegisteredRule(OT003_ID),
        rule_id: Some(OT003_ID),
        paradigm_id: Some(OT_PARADIGM),
        default_severity: mode.elevate(Severity::Fatal),
        span: Some(f.span.clone()),
        concept: Some(concept.to_string()),
        message: format!(
            "function `{}` exposes boundary type `{}` in its signature; \
             boundary types must be converted before crossing into \
             domain/application code",
            f.symbol, short
        ),
        evidence: vec![],
        why: vec![
            format!(
                "file `{}` is not a boundary file (no accepted boundary lives here)",
                f.span.file
            ),
            format!("`{short}` is the accepted boundary for concept `{concept}`"),
            format!("`{}` is not an accepted converter", f.symbol),
        ],
        suggested_fix: Some(format!(
            "convert `{short}` at the edge: \
             `let domain = canonical_for_{concept}::try_from(value)?;`, \
             then take the canonical type in this signature instead"
        )),
        diagnostic_code: None,
    }
}
