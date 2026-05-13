//! SARIF v2.1.0 formatter.
//!
//! Emits a single `run` with one `tool.driver` named `Locus`. Each
//! diagnostic becomes one `result`; one `reportingDescriptor` per
//! distinct `rule_id` populates `tool.driver.rules`.
//!
//! Reference: SARIF v2.1.0 OASIS standard.
//! <https://docs.oasis-open.org/sarif/sarif/v2.1.0/os/sarif-v2.1.0-os.html>
//!
//! Mapping summary:
//! - `Severity::Fatal`   → `level: "error"`
//! - `Severity::Warning` → `level: "warning"`
//! - `Severity::Advisory`→ `level: "note"`
//! - rule id → `result.ruleId` and the matching `reportingDescriptor.id`
//! - span    → `result.locations[0].physicalLocation`
//! - `why` lines, suggested fix, and decision metadata land in
//!   `result.properties` so the SARIF stays schema-clean while still
//!   carrying Locus-specific context.
//!
//! GitHub code-scanning, Azure DevOps, and most SARIF viewers ingest
//! this shape without driver-specific plumbing.

use std::collections::BTreeMap;
use std::io::{self, Write};

use locus_core::governance::{DecisionStatus, SeverityChange};
use locus_core::{Diagnostic, Severity};
use serde_json::{Map, Value, json};

use crate::{DecisionRecord, TOOL_INFORMATION_URI, TOOL_NAME, TOOL_VERSION};

/// SARIF schema URL pinned to the OASIS v2.1.0 standard. SARIF
/// consumers use this to select the JSON Schema for validation.
pub const SARIF_SCHEMA: &str =
    "https://docs.oasis-open.org/sarif/sarif/v2.1.0/os/schemas/sarif-schema-2.1.0.json";

pub const SARIF_VERSION: &str = "2.1.0";

pub fn write<W: Write>(out: &mut W, records: &[DecisionRecord]) -> io::Result<()> {
    let value = build(records);
    serde_json::to_writer_pretty(&mut *out, &value)?;
    writeln!(out)
}

fn build(records: &[DecisionRecord]) -> Value {
    let rules = collect_rules(records);
    let results: Vec<Value> = records.iter().map(render_result).collect();
    json!({
        "$schema": SARIF_SCHEMA,
        "version": SARIF_VERSION,
        "runs": [
            {
                "tool": {
                    "driver": {
                        "name": TOOL_NAME,
                        "version": TOOL_VERSION,
                        "informationUri": TOOL_INFORMATION_URI,
                        "rules": rules,
                    }
                },
                "results": results,
            }
        ],
    })
}

/// One `reportingDescriptor` per distinct `rule_id`. Iteration is
/// alphabetic for deterministic output (snapshot-friendly).
fn collect_rules(records: &[DecisionRecord]) -> Vec<Value> {
    let mut by_id: BTreeMap<&str, &Diagnostic> = BTreeMap::new();
    for r in records {
        // First diagnostic per id wins. Rule descriptors don't carry
        // per-finding context so any sample is fine.
        by_id
            .entry(r.diagnostic.rule_id.as_str())
            .or_insert(&r.diagnostic);
    }
    by_id
        .into_iter()
        .map(|(id, d)| {
            json!({
                "id": id,
                "name": id,
                "shortDescription": { "text": short_description_for(id, d) },
                "defaultConfiguration": { "level": level_for(d.severity) },
            })
        })
        .collect()
}

/// Use the first-seen diagnostic's message as a stand-in description.
/// SARIF rule metadata is optional — a richer catalogue can ship later
/// by reading from `docs/PARADIGMS.md` per rule id.
fn short_description_for(id: &str, d: &Diagnostic) -> String {
    if d.message.is_empty() {
        id.to_string()
    } else {
        d.message.clone()
    }
}

fn render_result(r: &DecisionRecord) -> Value {
    let d = &r.diagnostic;
    let mut result = Map::new();
    result.insert("ruleId".into(), Value::String(d.rule_id.clone()));
    result.insert("level".into(), Value::String(level_for(d.severity).into()));
    result.insert(
        "message".into(),
        json!({ "text": if d.message.is_empty() { d.rule_id.clone() } else { d.message.clone() } }),
    );
    result.insert("locations".into(), Value::Array(vec![physical_location(d)]));
    let props = result_properties(r);
    if !props.is_empty() {
        result.insert("properties".into(), Value::Object(props));
    }
    Value::Object(result)
}

fn physical_location(d: &Diagnostic) -> Value {
    // SARIF requires line >= 1; the governance pipeline can emit
    // `line_start: 0` for synthetic `<governance>` spans (see
    // `pipeline::synthetic_governance_span`). Clamp to 1 so SARIF
    // consumers don't reject the result.
    let line_start = d.span.line_start.max(1);
    let line_end = d.span.line_end.max(line_start);
    json!({
        "physicalLocation": {
            "artifactLocation": {
                "uri": d.span.file,
                "uriBaseId": "%SRCROOT%",
            },
            "region": {
                "startLine": line_start,
                "endLine": line_end,
            }
        }
    })
}

fn result_properties(r: &DecisionRecord) -> Map<String, Value> {
    let d = &r.diagnostic;
    let mut props = Map::new();
    if let Some(c) = &d.concept {
        props.insert("concept".into(), Value::String(c.clone()));
    }
    if !d.why.is_empty() {
        props.insert(
            "why".into(),
            Value::Array(d.why.iter().cloned().map(Value::String).collect()),
        );
    }
    if let Some(fix) = &d.suggested_fix {
        props.insert("suggestedFix".into(), Value::String(fix.clone()));
    }
    if let Some(dec) = &r.decision {
        props.insert("policyId".into(), Value::String(dec.policy_id.clone()));
        props.insert(
            "decisionStatus".into(),
            Value::String(status_label(&dec.status).into()),
        );
        props.insert(
            "severityChange".into(),
            severity_change_value(&dec.severity_change),
        );
        if !dec.rationale.is_empty() {
            props.insert(
                "rationale".into(),
                Value::Array(dec.rationale.iter().cloned().map(Value::String).collect()),
            );
        }
    }
    props
}

fn level_for(s: Severity) -> &'static str {
    match s {
        Severity::Fatal => "error",
        Severity::Warning => "warning",
        Severity::Advisory => "note",
    }
}

fn status_label(s: &DecisionStatus) -> &'static str {
    match s {
        DecisionStatus::Active => "Active",
        DecisionStatus::Advisory => "Advisory",
        DecisionStatus::SuppressedByPolicy => "SuppressedByPolicy",
        DecisionStatus::AcceptedException => "AcceptedException",
        DecisionStatus::KnownTransitionDebt => "KnownTransitionDebt",
    }
}

fn severity_change_value(sc: &SeverityChange) -> Value {
    match sc {
        SeverityChange::Unchanged => Value::String("Unchanged".into()),
        SeverityChange::Downgraded { from } => json!({
            "kind": "Downgraded",
            "from": severity_name(*from),
        }),
        SeverityChange::Elevated { from } => json!({
            "kind": "Elevated",
            "from": severity_name(*from),
        }),
    }
}

fn severity_name(s: Severity) -> &'static str {
    match s {
        Severity::Fatal => "Fatal",
        Severity::Warning => "Warning",
        Severity::Advisory => "Advisory",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DecisionMetadata;
    use locus_air::AirSpan;

    fn fatal_diagnostic(id: &str, file: &str) -> Diagnostic {
        Diagnostic {
            rule_id: id.into(),
            severity: Severity::Fatal,
            span: AirSpan::new(file, 12, 14),
            concept: Some("user".into()),
            message: "duplicate canonical".into(),
            why: vec!["seen at src/a.rs".into()],
            suggested_fix: Some("locus accept canonical pkg::User".into()),
        }
    }

    #[test]
    fn shape_matches_sarif_v210() {
        let recs = vec![DecisionRecord::from_diagnostic(fatal_diagnostic(
            "OT001",
            "src/foo.rs",
        ))];
        let v = build(&recs);
        assert_eq!(v["$schema"], SARIF_SCHEMA);
        assert_eq!(v["version"], "2.1.0");
        assert_eq!(v["runs"][0]["tool"]["driver"]["name"], "Locus");
        assert!(v["runs"][0]["tool"]["driver"]["version"].is_string());
        assert_eq!(
            v["runs"][0]["tool"]["driver"]["informationUri"],
            TOOL_INFORMATION_URI
        );
    }

    #[test]
    fn severity_maps_to_sarif_levels() {
        let mk = |sev| Diagnostic {
            rule_id: "X".into(),
            severity: sev,
            span: AirSpan::new("a.rs", 1, 1),
            concept: None,
            message: "m".into(),
            why: Vec::new(),
            suggested_fix: None,
        };
        let recs = vec![
            DecisionRecord::from_diagnostic(mk(Severity::Fatal)),
            DecisionRecord::from_diagnostic(mk(Severity::Warning)),
            DecisionRecord::from_diagnostic(mk(Severity::Advisory)),
        ];
        let v = build(&recs);
        let results = v["runs"][0]["results"].as_array().unwrap();
        assert_eq!(results[0]["level"], "error");
        assert_eq!(results[1]["level"], "warning");
        assert_eq!(results[2]["level"], "note");
    }

    #[test]
    fn result_carries_span_in_physical_location() {
        let recs = vec![DecisionRecord::from_diagnostic(fatal_diagnostic(
            "OT001",
            "src/foo.rs",
        ))];
        let v = build(&recs);
        let loc = &v["runs"][0]["results"][0]["locations"][0]["physicalLocation"];
        assert_eq!(loc["artifactLocation"]["uri"], "src/foo.rs");
        assert_eq!(loc["artifactLocation"]["uriBaseId"], "%SRCROOT%");
        assert_eq!(loc["region"]["startLine"], 12);
        assert_eq!(loc["region"]["endLine"], 14);
    }

    #[test]
    fn synthetic_zero_lines_are_clamped_to_one() {
        let d = Diagnostic {
            rule_id: "LOCUS004".into(),
            severity: Severity::Advisory,
            span: AirSpan::new("<governance>", 0, 0),
            concept: None,
            message: "arch drift".into(),
            why: Vec::new(),
            suggested_fix: None,
        };
        let v = build(&[DecisionRecord::from_diagnostic(d)]);
        let region = &v["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"];
        assert_eq!(region["startLine"], 1);
        assert_eq!(region["endLine"], 1);
    }

    #[test]
    fn properties_carry_concept_why_suggested_fix() {
        let recs = vec![DecisionRecord::from_diagnostic(fatal_diagnostic(
            "OT001",
            "src/foo.rs",
        ))];
        let v = build(&recs);
        let props = &v["runs"][0]["results"][0]["properties"];
        assert_eq!(props["concept"], "user");
        assert_eq!(props["why"][0], "seen at src/a.rs");
        assert_eq!(props["suggestedFix"], "locus accept canonical pkg::User");
    }

    #[test]
    fn properties_carry_decision_metadata_when_present() {
        let r = DecisionRecord::with_decision(
            fatal_diagnostic("OT001", "src/foo.rs"),
            DecisionMetadata {
                policy_id: "default-pass-through".into(),
                status: DecisionStatus::KnownTransitionDebt,
                severity_change: SeverityChange::Downgraded {
                    from: Severity::Fatal,
                },
                rationale: vec!["legacy".into()],
            },
        );
        let v = build(&[r]);
        let props = &v["runs"][0]["results"][0]["properties"];
        assert_eq!(props["policyId"], "default-pass-through");
        assert_eq!(props["decisionStatus"], "KnownTransitionDebt");
        assert_eq!(props["severityChange"]["kind"], "Downgraded");
        assert_eq!(props["severityChange"]["from"], "Fatal");
        assert_eq!(props["rationale"][0], "legacy");
    }

    #[test]
    fn rules_section_dedupes_by_rule_id_and_sorts_alphabetically() {
        let mk = |id: &str| Diagnostic {
            rule_id: id.to_string(),
            severity: Severity::Warning,
            span: AirSpan::new("a.rs", 1, 1),
            concept: None,
            message: format!("msg for {id}"),
            why: Vec::new(),
            suggested_fix: None,
        };
        let recs: Vec<DecisionRecord> = ["DG001", "OT001", "DG001"]
            .iter()
            .map(|id| DecisionRecord::from_diagnostic(mk(id)))
            .collect();
        let v = build(&recs);
        let rules = v["runs"][0]["tool"]["driver"]["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0]["id"], "DG001");
        assert_eq!(rules[1]["id"], "OT001");
        assert_eq!(rules[0]["defaultConfiguration"]["level"], "warning");
    }

    #[test]
    fn empty_records_emit_zero_results_and_zero_rules() {
        let v = build(&[]);
        assert_eq!(v["runs"][0]["results"].as_array().unwrap().len(), 0);
        assert_eq!(
            v["runs"][0]["tool"]["driver"]["rules"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }
}
