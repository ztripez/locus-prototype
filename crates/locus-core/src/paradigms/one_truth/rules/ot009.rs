//! OT009 — scattered validation/normalization.
//!
//! Fires when a function lives outside the canonical's owner file *and*
//! outside any accepted converter, but its *name* and *signature* both look
//! like validation/normalization of a known canonical (e.g. `validate_email`
//! returning a `Result<EmailAddress, _>`, or `normalize_user_id(s: &str)
//! -> UserId`). Both signals are required so generic helpers
//! (`fn validate_input(...)`) don't trip the rule.
//!
//! Confidence 0.75. The spec lists this as "warning by default; fatal under
//! `--agent-strict` for high-confidence cases" — `from_confidence(0.75,
//! AgentStrict)` returns `Fatal`, `(0.75, Human)` returns `Warning`.

use std::collections::{BTreeMap, BTreeSet};

use locus_air::{AirItem, AirWorkspace};

use super::super::lockfile_schema::OtSection;
use super::helpers::{file_of_symbol, type_text_references};
use crate::diagnostics::{CheckMode, Severity};
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{FindingIdMinter, ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

pub struct Ot009Rule;

pub static OT009_RULE: Ot009Rule = Ot009Rule;

const OT009_ID: RuleId = RuleId::new("OT009");
const OT_PARADIGM: ParadigmId = ParadigmId::new("OT");

impl RuleDefinition for Ot009Rule {
    fn id(&self) -> RuleId {
        OT009_ID
    }
    fn paradigm(&self) -> ParadigmId {
        OT_PARADIGM
    }
    fn title(&self) -> &'static str {
        "scattered validation/normalization"
    }
    fn default_severity(&self) -> Severity {
        Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        let section: OtSection = ctx.lockfile.paradigm_section("OT").unwrap_or_default();
        produce_findings(ctx.air, &section, ctx.mode, ctx.finding_ids)
    }
}

/// Build short-name → (concept_id, owner_file) map for OT009.
fn build_ot009_canonicals(
    air: &AirWorkspace,
    section: &OtSection,
) -> BTreeMap<String, (String, String)> {
    let mut canonicals: BTreeMap<String, (String, String)> = BTreeMap::new();
    for (concept_id, entry) in &section.concepts {
        let symbol = &entry.canonical.symbol;
        let Some(short) = symbol.rsplit("::").next() else {
            continue;
        };
        let Some(file_path) = file_of_symbol(air, symbol) else {
            continue;
        };
        canonicals.insert(short.to_string(), (concept_id.clone(), file_path));
    }
    canonicals
}

/// Find the canonical referenced by this function's signature, if any.
/// Returns `(concept_id, owner_file)` for the first matching canonical.
fn ot009_signature_match<'a>(
    f: &locus_air::AirFunction,
    canonicals: &'a BTreeMap<String, (String, String)>,
) -> Option<(&'a str, &'a str)> {
    for (short, (concept, owner)) in canonicals {
        let hits = f.params.iter().any(|(_, t)| type_text_references(t, short))
            || f.return_type
                .as_deref()
                .is_some_and(|t| type_text_references(t, short));
        if hits {
            return Some((concept.as_str(), owner.as_str()));
        }
    }
    None
}

pub(crate) fn produce_findings(
    air: &AirWorkspace,
    section: &OtSection,
    mode: CheckMode,
    finding_ids: &FindingIdMinter,
) -> Vec<RuleFinding> {
    let canonicals = build_ot009_canonicals(air, section);
    if canonicals.is_empty() {
        return Vec::new();
    }
    let accepted_converters: BTreeSet<&str> = section
        .concepts
        .values()
        .flat_map(|e| e.converters.iter().map(|c| c.symbol.as_str()))
        .collect();

    let confidence = 0.75;
    let Some(severity) = Severity::from_confidence(confidence, mode) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Function(f) = item else {
                    continue;
                };
                if accepted_converters.contains(f.symbol.as_str()) {
                    continue;
                }
                let Some(prefix) = matched_validate_prefix(&f.name) else {
                    continue;
                };
                let Some((concept_id, owner_file)) = ot009_signature_match(f, &canonicals) else {
                    continue;
                };
                if file.path == owner_file {
                    continue; // validator inside the canonical's own module is fine
                }
                out.push(ot009_finding(
                    f,
                    prefix,
                    concept_id,
                    owner_file,
                    confidence,
                    severity,
                    finding_ids,
                ));
            }
        }
    }
    out
}

fn ot009_finding(
    f: &locus_air::AirFunction,
    prefix: &str,
    concept_id: &str,
    owner_file: &str,
    confidence: f32,
    severity: Severity,
    finding_ids: &FindingIdMinter,
) -> RuleFinding {
    RuleFinding {
        id: finding_ids.next(),
        source: FindingSource::RegisteredRule(OT009_ID),
        rule_id: Some(OT009_ID),
        paradigm_id: Some(OT_PARADIGM),
        default_severity: severity,
        span: Some(f.span.clone()),
        concept: Some(concept_id.to_string()),
        message: format!(
            "`{}` looks like {prefix} for canonical `{concept_id}` but lives \
             outside the owner module and outside any accepted converter",
            f.symbol
        ),
        evidence: vec![],
        why: vec![
            format!("function name starts with `{prefix}` (validation/normalization shape)"),
            format!("signature references canonical for `{concept_id}`"),
            format!("owner module: `{owner_file}`"),
            format!("inference confidence: {confidence:.2}"),
        ],
        suggested_fix: Some(format!(
            "move this into the owner of `{concept_id}` (so the canonical \
             enforces its own invariants), or accept this function as a \
             converter via `locus init` if it's the legitimate edge"
        )),
        diagnostic_code: None,
    }
}

/// Returns the matched prefix if `name` starts with one of the
/// validation/normalization shape prefixes recognised by OT009.
fn matched_validate_prefix(name: &str) -> Option<&'static str> {
    const PREFIXES: &[&str] = &[
        "validate_",
        "is_valid_",
        "check_",
        "verify_",
        "ensure_",
        "normalize_",
        "sanitize_",
        "canonicalize_",
        "parse_",
        "clean_",
    ];
    PREFIXES.iter().copied().find(|p| name.starts_with(p))
}
