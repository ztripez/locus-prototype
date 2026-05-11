//! `RuleFinding` and its store.
//!
//! Findings are the substrate that policies decide over. A finding is
//! emitted by either a registered rule (`FindingSource::RegisteredRule`),
//! the legacy compat adapter (`FindingSource::LegacyDiagnostic`), or a
//! policy itself (`FindingSource::Policy`). Typed evidence variants live
//! in `evidence`.

// locus: ot canonical

use crate::diagnostics::Severity;
use crate::governance::evidence::Evidence;
use crate::governance::ids::{FindingId, ParadigmId, PolicyId, RuleId};
use locus_air::AirSpan;
use serde::Serialize;
use std::collections::BTreeMap;

// RuleFinding and its source/store derive `Serialize` (we emit findings
// as JSON for SARIF/observe in future epics) but NOT `Deserialize`:
// transitively contain RuleId/ParadigmId/PolicyId which store
// `&'static str` and can only be constructed at compile time.

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum FindingSource {
    RegisteredRule(RuleId),
    LegacyDiagnostic {
        rule_code: String,
        paradigm: Option<ParadigmId>,
    },
    Policy(PolicyId),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RuleFinding {
    pub id: FindingId,
    pub source: FindingSource,
    pub rule_id: Option<RuleId>,
    pub paradigm_id: Option<ParadigmId>,
    pub default_severity: Severity,
    pub span: Option<AirSpan>,
    pub concept: Option<String>,
    pub message: String,
    pub evidence: Vec<Evidence>,
    pub why: Vec<String>,
    pub suggested_fix: Option<String>,
    /// Governance/policy diagnostic code (e.g. `"LOCUS003"`) to emit when
    /// distinct from rule_id / source. Resolved against
    /// `GovernanceDiagnosticRegistry`; unresolved codes are an internal
    /// error caught by RegistryIntegrityPolicy.
    pub diagnostic_code: Option<String>,
}

/// Id-keyed store of all findings produced in a pipeline run. Backed by a
/// `BTreeMap` so iteration is in id order — FindingIdMinter is sequential,
/// so this matches insertion order and keeps output deterministic for
/// golden snapshots.
#[derive(Debug, Default)]
pub struct FindingStore {
    findings: BTreeMap<FindingId, RuleFinding>,
}

impl FindingStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, f: RuleFinding) {
        self.findings.insert(f.id, f);
    }

    pub fn get(&self, id: FindingId) -> Option<&RuleFinding> {
        self.findings.get(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &RuleFinding> {
        self.findings.values()
    }

    pub fn len(&self) -> usize {
        self.findings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.findings.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finding_store_returns_findings_in_id_order() {
        let mut store = FindingStore::new();
        store.insert(sample_finding(FindingId::from_raw_for_test(2), "MSG2"));
        store.insert(sample_finding(FindingId::from_raw_for_test(0), "MSG0"));
        store.insert(sample_finding(FindingId::from_raw_for_test(1), "MSG1"));
        let messages: Vec<&str> = store.iter().map(|f| f.message.as_str()).collect();
        assert_eq!(messages, vec!["MSG0", "MSG1", "MSG2"]);
    }

    #[test]
    fn finding_serializes_with_all_fields() {
        let f = sample_finding(FindingId::from_raw_for_test(7), "ok");
        let json = serde_json::to_value(&f).unwrap();
        assert_eq!(json["id"], 7);
        assert_eq!(json["rule_id"], "CX001");
        assert_eq!(json["paradigm_id"], "CX");
        assert_eq!(json["message"], "ok");
        assert_eq!(json["default_severity"], "Warning");
        assert!(json["span"].is_object());
        assert!(json["evidence"].is_array());
        assert!(json["diagnostic_code"].is_null());
    }

    fn sample_finding(id: FindingId, msg: &str) -> RuleFinding {
        RuleFinding {
            id,
            source: FindingSource::RegisteredRule(RuleId::new("CX001")),
            rule_id: Some(RuleId::new("CX001")),
            paradigm_id: Some(ParadigmId::new("CX")),
            default_severity: Severity::Warning,
            span: Some(AirSpan::new("src/foo.rs", 1, 1)),
            concept: None,
            message: msg.to_string(),
            evidence: Vec::new(),
            why: Vec::new(),
            suggested_fix: None,
            diagnostic_code: None,
        }
    }
}
