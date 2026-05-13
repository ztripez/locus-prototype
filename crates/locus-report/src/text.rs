//! Human-readable text formatter.
//!
//! The shape matches the pre-#29 CLI output verbatim — a per-diagnostic
//! block with `label[rule_id]: message`, file/line, optional concept,
//! `why` reasons, suggested fix, then a trailing severity summary.

use std::io::{self, Write};

use locus_core::{Diagnostic, Severity};

use crate::DecisionRecord;

pub fn write<W: Write>(out: &mut W, records: &[DecisionRecord]) -> io::Result<()> {
    if records.is_empty() {
        writeln!(out, "no diagnostics — workspace is clean.")?;
        return Ok(());
    }
    let mut fatal = 0usize;
    let mut warning = 0usize;
    let mut advisory = 0usize;
    for r in records {
        let d = &r.diagnostic;
        let label = match d.severity {
            Severity::Fatal => {
                fatal += 1;
                "error"
            }
            Severity::Warning => {
                warning += 1;
                "warning"
            }
            Severity::Advisory => {
                advisory += 1;
                "info"
            }
        };
        render_one(out, d, label)?;
    }
    writeln!(
        out,
        "summary: {fatal} error(s), {warning} warning(s), {advisory} advisory."
    )?;
    Ok(())
}

fn render_one<W: Write>(out: &mut W, d: &Diagnostic, label: &str) -> io::Result<()> {
    writeln!(
        out,
        "{label}[{}]: {}\n  --> {}:{}",
        d.rule_id, d.message, d.span.file, d.span.line_start
    )?;
    if let Some(c) = &d.concept {
        writeln!(out, "  concept: {c}")?;
    }
    for reason in &d.why {
        writeln!(out, "  - {reason}")?;
    }
    if let Some(fix) = &d.suggested_fix {
        writeln!(out, "  fix: {fix}")?;
    }
    writeln!(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::AirSpan;

    #[test]
    fn empty_records_print_clean_workspace_message() {
        let mut out = Vec::new();
        write(&mut out, &[]).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert_eq!(s, "no diagnostics — workspace is clean.\n");
    }

    #[test]
    fn renders_severity_label_concept_and_why() {
        let d = Diagnostic {
            rule_id: "OT001".into(),
            severity: Severity::Fatal,
            span: AirSpan::new("src/foo.rs", 12, 14),
            concept: Some("user".into()),
            message: "duplicate canonical".into(),
            why: vec!["seen at src/a.rs".into(), "seen at src/b.rs".into()],
            suggested_fix: Some("locus accept canonical pkg::User".into()),
        };
        let mut out = Vec::new();
        write(&mut out, &[DecisionRecord::from_diagnostic(d)]).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("error[OT001]: duplicate canonical"));
        assert!(s.contains("--> src/foo.rs:12"));
        assert!(s.contains("concept: user"));
        assert!(s.contains("- seen at src/a.rs"));
        assert!(s.contains("fix: locus accept canonical pkg::User"));
        assert!(s.contains("summary: 1 error(s), 0 warning(s), 0 advisory."));
    }

    #[test]
    fn severity_summary_counts_each_tier() {
        let mk = |sev| Diagnostic {
            rule_id: "X".into(),
            severity: sev,
            span: AirSpan::new("a.rs", 1, 1),
            concept: None,
            message: "m".into(),
            why: Vec::new(),
            suggested_fix: None,
        };
        let records = vec![
            DecisionRecord::from_diagnostic(mk(Severity::Fatal)),
            DecisionRecord::from_diagnostic(mk(Severity::Warning)),
            DecisionRecord::from_diagnostic(mk(Severity::Warning)),
            DecisionRecord::from_diagnostic(mk(Severity::Advisory)),
        ];
        let mut out = Vec::new();
        write(&mut out, &records).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.ends_with("summary: 1 error(s), 2 warning(s), 1 advisory.\n"));
    }
}
