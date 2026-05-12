//! OT004 — direct canonical construction outside owner or accepted converter.
//!
//! Walks every `Construct` truth-action in AIR. Fires when the constructed
//! type is an accepted canonical, the construction is *not* in the owner
//! file, and the enclosing function is *not* an accepted converter.
//!
//! Always Fatal: per the spec, canonical types may only be constructed in
//! their owner module or in named, accepted converters. Anywhere else is
//! authority fragmentation.

use std::collections::{BTreeMap, BTreeSet};

use locus_air::{ActionKind, AirItem, AirWorkspace};

use super::super::lockfile_schema::OtSection;
use super::helpers::{file_of_symbol, matches_symbol_pattern};
use crate::diagnostics::{CheckMode, Severity};
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{FindingIdMinter, ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

pub struct Ot004Rule;

pub static OT004_RULE: Ot004Rule = Ot004Rule;

const OT004_ID: RuleId = RuleId::new("OT004");
const OT_PARADIGM: ParadigmId = ParadigmId::new("OT");

impl RuleDefinition for Ot004Rule {
    fn id(&self) -> RuleId {
        OT004_ID
    }
    fn paradigm(&self) -> ParadigmId {
        OT_PARADIGM
    }
    fn title(&self) -> &'static str {
        "direct canonical construction outside owner or accepted converter"
    }
    fn default_severity(&self) -> Severity {
        Severity::Fatal
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        let section: OtSection = ctx.lockfile.paradigm_section("OT").unwrap_or_default();
        produce_findings(ctx.air, &section, ctx.mode, ctx.finding_ids)
    }
}

/// Build canonical short-name index: `short → (full_symbol, owner_file, concept_id)`.
fn build_ot004_canonicals(
    air: &AirWorkspace,
    section: &OtSection,
) -> BTreeMap<String, (String, String, String)> {
    let mut canonicals: BTreeMap<String, (String, String, String)> = BTreeMap::new();
    for (concept_id, entry) in &section.concepts {
        let symbol = &entry.canonical.symbol;
        let Some(short) = symbol.rsplit("::").next() else {
            continue;
        };
        let Some(file_path) = file_of_symbol(air, symbol) else {
            continue;
        };
        canonicals.insert(
            short.to_string(),
            (symbol.clone(), file_path, concept_id.clone()),
        );
    }
    canonicals
}

/// Check one `Construct` action against the canonical index; push a finding
/// if it violates OT004.
#[allow(clippy::too_many_arguments)]
fn ot004_check_action(
    a: &locus_air::AirTruthAction,
    file_path: &str,
    canonicals: &BTreeMap<String, (String, String, String)>,
    accepted_converters: &BTreeSet<&str>,
    section: &OtSection,
    mode: CheckMode,
    finding_ids: &FindingIdMinter,
    out: &mut Vec<RuleFinding>,
) {
    if a.action != ActionKind::Construct {
        return;
    }
    // `target` is the rendered constructed-path text. Use the last `::` segment
    // so path-prefixed literal forms still match.
    let short = a
        .target
        .rsplit("::")
        .next()
        .unwrap_or(a.target.as_str())
        .trim();
    let Some((canonical_symbol, owner_file, concept_id)) = canonicals.get(short) else {
        return;
    };
    if file_path == owner_file {
        return; // construction in owner module is fine
    }
    if let Some(fn_sym) = &a.function
        && accepted_converters.contains(fn_sym.as_str())
    {
        return; // construction inside an accepted converter is fine
    }
    if section.converter_paths.iter().any(|p| {
        a.function
            .as_deref()
            .is_some_and(|f| matches_symbol_pattern(f, p))
            || matches_symbol_pattern(file_path, p)
    }) {
        return; // accepted by OT.converter_paths authority
    }
    let function_label = a
        .function
        .as_deref()
        .unwrap_or("(no enclosing function recorded)");
    out.push(ot004_finding(
        a,
        canonical_symbol,
        owner_file,
        function_label,
        concept_id,
        mode,
        finding_ids,
    ));
}

pub(crate) fn produce_findings(
    air: &AirWorkspace,
    section: &OtSection,
    mode: CheckMode,
    finding_ids: &FindingIdMinter,
) -> Vec<RuleFinding> {
    let canonicals = build_ot004_canonicals(air, section);
    if canonicals.is_empty() {
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
            for item in &file.items {
                let AirItem::TruthAction(a) = item else {
                    continue;
                };
                ot004_check_action(
                    a,
                    &file.path,
                    &canonicals,
                    &accepted_converters,
                    section,
                    mode,
                    finding_ids,
                    &mut out,
                );
            }
        }
    }
    out
}

fn ot004_finding(
    a: &locus_air::AirTruthAction,
    canonical_symbol: &str,
    owner_file: &str,
    function_label: &str,
    concept_id: &str,
    mode: CheckMode,
    finding_ids: &FindingIdMinter,
) -> RuleFinding {
    RuleFinding {
        id: finding_ids.next(),
        source: FindingSource::RegisteredRule(OT004_ID),
        rule_id: Some(OT004_ID),
        paradigm_id: Some(OT_PARADIGM),
        default_severity: mode.elevate(Severity::Fatal),
        span: Some(a.span.clone()),
        concept: Some(concept_id.to_string()),
        message: format!(
            "direct construction of canonical `{canonical_symbol}` outside its owner module \
             and outside any accepted converter"
        ),
        evidence: vec![],
        why: vec![
            format!("constructed at `{}:{}`", a.span.file, a.span.line_start),
            format!("owner module is `{owner_file}`"),
            format!("enclosing function `{function_label}` is not an accepted converter"),
        ],
        suggested_fix: Some(format!(
            "go through the accepted converter (e.g. `{canonical_symbol}::try_from(value)?`), \
             or accept this function as a converter and rerun `locus init`"
        )),
        diagnostic_code: None,
    }
}
