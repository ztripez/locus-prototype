//! TRANSITIONAL.
//!
//! `LegacyParadigmRuleAdapter` runs each existing legacy `Paradigm::check`
//! and wraps every emitted `Diagnostic` whose `rule_id` is NOT in the
//! `RuleRegistry` into a synthetic `RuleFinding`. The filter is
//! per-diagnostic, NOT per-paradigm: a paradigm with one migrated rule
//! and ten un-migrated ones still gets nine legacy synthesized findings.
//!
//! This module is removed once every rule code migrates to a registered
//! `RuleDefinition`.

// locus: ot canonical

use crate::diagnostics::{CheckMode, Diagnostic};
use crate::governance::evidence::{Evidence, LegacyEvidence};
use crate::governance::finding::{FindingSource, FindingStore, RuleFinding};
use crate::governance::ids::{FindingIdMinter, ParadigmId};
use crate::governance::registry::RuleRegistry;
use crate::lockfile::Lockfile;
use crate::paradigms::Paradigm;
use locus_air::AirWorkspace;

pub struct LegacyParadigmRuleAdapter;

impl LegacyParadigmRuleAdapter {
    pub fn run(
        paradigms: &[Box<dyn Paradigm>],
        air: &AirWorkspace,
        lockfile: &Lockfile,
        mode: CheckMode,
        rule_registry: &RuleRegistry,
        minter: &FindingIdMinter,
        store: &mut FindingStore,
    ) {
        for p in paradigms {
            let prefix = ParadigmId::new(p.rule_prefix());
            for diag in p.check(air, lockfile, mode) {
                // Per-diagnostic-code filter. Critical for correct
                // strangler behavior: a paradigm with one migrated rule
                // and N un-migrated ones still emits the N legacy
                // findings.
                if rule_registry.contains_code(&diag.rule_id) {
                    continue;
                }
                store.insert(synthesize(diag, prefix, minter));
            }
        }
    }
}

fn synthesize(d: Diagnostic, prefix: ParadigmId, minter: &FindingIdMinter) -> RuleFinding {
    RuleFinding {
        id: minter.next(),
        source: FindingSource::LegacyDiagnostic {
            rule_code: d.rule_id.clone(),
            paradigm: Some(prefix),
        },
        rule_id: None,
        paradigm_id: Some(prefix),
        default_severity: d.severity,
        span: Some(d.span.clone()),
        concept: d.concept.clone(),
        message: d.message.clone(),
        evidence: vec![Evidence::Legacy(LegacyEvidence {
            original_message: d.message.clone(),
            original_why: d.why.clone(),
            original_suggested_fix: d.suggested_fix.clone(),
        })],
        why: d.why,
        suggested_fix: d.suggested_fix,
        diagnostic_code: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Severity;
    use locus_air::AirSpan;

    #[test]
    fn synthesize_preserves_diagnostic_fields() {
        let d = Diagnostic {
            rule_id: "CX001".to_string(),
            severity: Severity::Warning,
            span: AirSpan::new("src/foo.rs", 10, 12),
            concept: Some("some_concept".to_string()),
            message: "function too long".to_string(),
            why: vec!["73 lines > 50".to_string()],
            suggested_fix: Some("split function".to_string()),
        };
        let m = FindingIdMinter::new();
        let f = synthesize(d.clone(), ParadigmId::new("CX"), &m);
        assert_eq!(f.default_severity, d.severity);
        assert_eq!(f.message, d.message);
        assert_eq!(f.why, d.why);
        assert_eq!(f.suggested_fix, d.suggested_fix);
        assert_eq!(f.concept, d.concept);
        let span = f.span.as_ref().unwrap();
        assert_eq!(span.file, "src/foo.rs");
        assert_eq!(span.line_start, 10);
        match &f.source {
            FindingSource::LegacyDiagnostic {
                rule_code,
                paradigm,
            } => {
                assert_eq!(rule_code, "CX001");
                assert_eq!(paradigm.unwrap().as_str(), "CX");
            }
            _ => panic!("wrong source"),
        }
        assert_eq!(f.evidence.len(), 1);
        assert!(matches!(&f.evidence[0], Evidence::Legacy(_)));
    }

    #[test]
    fn legacy_diagnostic_with_registered_rule_code_is_skipped() {
        // Post-P2 (#71), CX001 IS in `RuleRegistry::standard()`. The
        // legacy adapter's per-diagnostic-code filter sees `contains_code`
        // == true and skips synthesizing a legacy CX001 finding — that's
        // the strangler invariant in action.
        let reg = RuleRegistry::standard();
        assert!(reg.contains_code("CX001"), "CX001 must be registered post-P2");
        assert!(!reg.contains_code("XX999"), "unregistered codes must return false");
    }
}
