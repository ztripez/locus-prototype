//! Stable JSON formatter.
//!
//! Stable JSON shape for `locus check --format json`. Tools consume
//! this directly; the schema below is the contract.
//!
//! ```text
//! {
//!   "schema_version": 1,
//!   "tool": { "name": "Locus", "version": "0.1.0" },
//!   "results": [
//!     {
//!       "rule_id": "OT001",
//!       "severity": "Fatal",
//!       "message": "...",
//!       "concept": "user",          // omitted when null
//!       "location": { "file": "src/foo.rs", "line_start": 12, "line_end": 14 },
//!       "why": ["..."],
//!       "suggested_fix": "...",     // omitted when null
//!       "decision": {                // omitted for non-policy diagnostics
//!         "policy_id": "default-pass-through",
//!         "status": "Active",
//!         "severity_change": "Unchanged",
//!         "rationale": ["..."]
//!       }
//!     }
//!   ],
//!   "summary": { "fatal": 1, "warning": 0, "advisory": 0 }
//! }
//! ```
//!
//! Schema version bumps on any breaking field change. Optional fields
//! (`concept`, `suggested_fix`, `decision`) are omitted entirely when
//! absent — never serialized as `null` — so downstream JSON-Schema
//! validation can mark them `required: false` cleanly.

use std::io::{self, Write};

use locus_core::Severity;
use locus_core::governance::{DecisionStatus, SeverityChange};
use serde_json::{Map, Value, json};

use crate::{DecisionRecord, TOOL_NAME, TOOL_VERSION};

/// JSON output schema version. Bump on any breaking field change.
pub const SCHEMA_VERSION: u32 = 1;

pub fn write<W: Write>(out: &mut W, records: &[DecisionRecord]) -> io::Result<()> {
    let value = build(records);
    serde_json::to_writer_pretty(&mut *out, &value)?;
    writeln!(out)
}

fn build(records: &[DecisionRecord]) -> Value {
    let (mut fatal, mut warning, mut advisory) = (0usize, 0usize, 0usize);
    let results: Vec<Value> = records
        .iter()
        .map(|r| {
            match r.diagnostic.severity {
                Severity::Fatal => fatal += 1,
                Severity::Warning => warning += 1,
                Severity::Advisory => advisory += 1,
            }
            render_result(r)
        })
        .collect();
    json!({
        "schema_version": SCHEMA_VERSION,
        "tool": {
            "name": TOOL_NAME,
            "version": TOOL_VERSION,
        },
        "results": results,
        "summary": {
            "fatal": fatal,
            "warning": warning,
            "advisory": advisory,
        },
    })
}

fn render_result(r: &DecisionRecord) -> Value {
    let d = &r.diagnostic;
    let mut map = Map::new();
    map.insert("rule_id".into(), Value::String(d.rule_id.clone()));
    map.insert("severity".into(), severity_to_json(d.severity));
    map.insert("message".into(), Value::String(d.message.clone()));
    if let Some(concept) = &d.concept {
        map.insert("concept".into(), Value::String(concept.clone()));
    }
    map.insert(
        "location".into(),
        json!({
            "file": d.span.file,
            "line_start": d.span.line_start,
            "line_end": d.span.line_end,
        }),
    );
    map.insert(
        "why".into(),
        Value::Array(d.why.iter().cloned().map(Value::String).collect()),
    );
    if let Some(fix) = &d.suggested_fix {
        map.insert("suggested_fix".into(), Value::String(fix.clone()));
    }
    if let Some(dec) = &r.decision {
        map.insert("decision".into(), render_decision(dec));
    }
    Value::Object(map)
}

fn render_decision(dec: &crate::DecisionMetadata) -> Value {
    json!({
        "policy_id": dec.policy_id,
        "status": status_label(&dec.status),
        "severity_change": severity_change_label(&dec.severity_change),
        "rationale": dec.rationale,
    })
}

fn severity_to_json(s: Severity) -> Value {
    Value::String(
        match s {
            Severity::Fatal => "Fatal",
            Severity::Warning => "Warning",
            Severity::Advisory => "Advisory",
        }
        .into(),
    )
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

fn severity_change_label(sc: &SeverityChange) -> Value {
    match sc {
        SeverityChange::Unchanged => Value::String("Unchanged".into()),
        SeverityChange::Downgraded { from } => json!({
            "kind": "Downgraded",
            "from": severity_to_json(*from),
        }),
        SeverityChange::Elevated { from } => json!({
            "kind": "Elevated",
            "from": severity_to_json(*from),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DecisionMetadata;
    use locus_air::AirSpan;
    use locus_core::Diagnostic;

    fn sample_diagnostic() -> Diagnostic {
        Diagnostic {
            rule_id: "OT001".into(),
            severity: Severity::Fatal,
            span: AirSpan::new("src/foo.rs", 12, 14),
            concept: Some("user".into()),
            message: "duplicate canonical".into(),
            why: vec!["seen at src/a.rs".into()],
            suggested_fix: Some("locus accept canonical pkg::User".into()),
        }
    }

    #[test]
    fn empty_records_emit_zero_summary_and_empty_results() {
        let v = build(&[]);
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["tool"]["name"], "Locus");
        assert!(v["results"].as_array().unwrap().is_empty());
        assert_eq!(v["summary"]["fatal"], 0);
        assert_eq!(v["summary"]["warning"], 0);
        assert_eq!(v["summary"]["advisory"], 0);
    }

    #[test]
    fn diagnostic_without_decision_omits_decision_field() {
        let r = DecisionRecord::from_diagnostic(sample_diagnostic());
        let v = build(&[r]);
        let res = &v["results"][0];
        assert_eq!(res["rule_id"], "OT001");
        assert_eq!(res["severity"], "Fatal");
        assert_eq!(res["concept"], "user");
        assert_eq!(res["location"]["file"], "src/foo.rs");
        assert_eq!(res["location"]["line_start"], 12);
        assert_eq!(res["why"][0], "seen at src/a.rs");
        assert_eq!(res["suggested_fix"], "locus accept canonical pkg::User");
        assert!(
            res.as_object().unwrap().get("decision").is_none(),
            "decision should be omitted when None, was: {res}"
        );
    }

    #[test]
    fn diagnostic_with_decision_renders_full_decision_block() {
        let r = DecisionRecord::with_decision(
            sample_diagnostic(),
            DecisionMetadata {
                policy_id: "default-pass-through".into(),
                status: DecisionStatus::KnownTransitionDebt,
                severity_change: SeverityChange::Downgraded {
                    from: Severity::Fatal,
                },
                rationale: vec!["legacy diagnostic preserved".into()],
            },
        );
        let v = build(&[r]);
        let dec = &v["results"][0]["decision"];
        assert_eq!(dec["policy_id"], "default-pass-through");
        assert_eq!(dec["status"], "KnownTransitionDebt");
        assert_eq!(dec["severity_change"]["kind"], "Downgraded");
        assert_eq!(dec["severity_change"]["from"], "Fatal");
        assert_eq!(dec["rationale"][0], "legacy diagnostic preserved");
    }

    #[test]
    fn severity_change_unchanged_renders_as_plain_string() {
        let r = DecisionRecord::with_decision(
            sample_diagnostic(),
            DecisionMetadata {
                policy_id: "default-pass-through".into(),
                status: DecisionStatus::Active,
                severity_change: SeverityChange::Unchanged,
                rationale: Vec::new(),
            },
        );
        let v = build(&[r]);
        assert_eq!(
            v["results"][0]["decision"]["severity_change"],
            Value::String("Unchanged".into())
        );
    }

    #[test]
    fn omits_concept_and_suggested_fix_when_absent() {
        let d = Diagnostic {
            rule_id: "DG001".into(),
            severity: Severity::Warning,
            span: AirSpan::new("src/x.rs", 1, 1),
            concept: None,
            message: "forbidden import".into(),
            why: Vec::new(),
            suggested_fix: None,
        };
        let v = build(&[DecisionRecord::from_diagnostic(d)]);
        let res = &v["results"][0];
        let obj = res.as_object().unwrap();
        assert!(!obj.contains_key("concept"));
        assert!(!obj.contains_key("suggested_fix"));
    }

    #[test]
    fn summary_counts_severities() {
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
            DecisionRecord::from_diagnostic(mk(Severity::Fatal)),
            DecisionRecord::from_diagnostic(mk(Severity::Warning)),
            DecisionRecord::from_diagnostic(mk(Severity::Advisory)),
            DecisionRecord::from_diagnostic(mk(Severity::Advisory)),
            DecisionRecord::from_diagnostic(mk(Severity::Advisory)),
        ];
        let v = build(&recs);
        assert_eq!(v["summary"]["fatal"], 2);
        assert_eq!(v["summary"]["warning"], 1);
        assert_eq!(v["summary"]["advisory"], 3);
    }
}
