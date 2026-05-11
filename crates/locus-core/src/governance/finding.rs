//! `RuleFinding` and the evidence it carries.
//!
//! Findings are the substrate that policies decide over. A finding is
//! emitted by either a registered rule (`FindingSource::RegisteredRule`),
//! the legacy compat adapter (`FindingSource::LegacyDiagnostic`), or a
//! policy itself (`FindingSource::Policy`).

// locus: ot canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Confidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Evidence {
    /// Typed variant for complexity-budget rules (CX001/CX002).
    ComplexityBudget {
        lines: u32,
        budget: u32,
        override_match: Option<String>,
    },
    /// Typed variant for inference-shaped ownership rules (OT002, etc.).
    InferenceConfidence {
        score: Confidence,
        signals: Vec<String>,
    },
    /// Catch-all for migrated rules whose schema is not yet typed.
    Structured(serde_json::Value),
    /// Adapter payload — original diagnostic prose, no schema.
    Legacy(LegacyEvidence),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LegacyEvidence {
    pub original_message: String,
    pub original_why: Vec<String>,
    pub original_suggested_fix: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complexity_budget_round_trips_through_json() {
        let e = Evidence::ComplexityBudget {
            lines: 73,
            budget: 50,
            override_match: Some("custom/path".to_string()),
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: Evidence = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn confidence_carries_through_inference_evidence() {
        let e = Evidence::InferenceConfidence {
            score: Confidence::High,
            signals: vec!["matched canonical suffix".into()],
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: Evidence = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn legacy_evidence_preserves_original_diagnostic_fields() {
        let le = LegacyEvidence {
            original_message: "X".into(),
            original_why: vec!["a".into(), "b".into()],
            original_suggested_fix: None,
        };
        let e = Evidence::Legacy(le.clone());
        match e {
            Evidence::Legacy(out) => assert_eq!(out, le),
            _ => panic!("wrong variant"),
        }
    }
}
