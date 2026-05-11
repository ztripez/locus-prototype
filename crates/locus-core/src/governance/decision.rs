//! Policy decisions over rule findings.
//!
//! `DecisionStatus` describes architectural state; `SeverityChange`
//! describes severity mutation. They are deliberately orthogonal: a
//! finding can be `Active` and `Downgraded` (rule fires, policy lowered
//! severity), or `KnownTransitionDebt` and `Unchanged` (visible debt,
//! severity untouched).

// locus: ot canonical

use crate::diagnostics::Severity;
use crate::governance::ids::{FindingId, PolicyId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecisionStatus {
    /// Normal violation. Emitted as Diagnostic.
    Active,
    /// Informational. Emitted as Diagnostic with Advisory severity.
    Advisory,
    /// Recorded but NOT emitted. Reserved for ExceptionPolicy migration.
    SuppressedByPolicy,
    /// Recorded but NOT emitted. Reserved for ExceptionPolicy migration.
    AcceptedException,
    /// Emitted. Visible migration backlog from the legacy adapter.
    KnownTransitionDebt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeverityChange {
    Unchanged,
    Downgraded { from: Severity },
    Elevated { from: Severity },
}

// `Decision` derives `Serialize` only — it contains `PolicyId` which
// stores `&'static str`. See `ids.rs` for the rationale.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Decision {
    pub finding_id: FindingId,
    pub policy: PolicyId,
    pub severity: Severity,
    pub status: DecisionStatus,
    pub severity_change: SeverityChange,
    pub rationale: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_serializes_with_all_fields() {
        let d = Decision {
            finding_id: FindingId::from_raw_for_test(42),
            policy: PolicyId::new("default-pass-through"),
            severity: Severity::Warning,
            status: DecisionStatus::Active,
            severity_change: SeverityChange::Unchanged,
            rationale: vec!["pass-through default".into()],
        };
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["finding_id"], 42);
        assert_eq!(json["policy"], "default-pass-through");
        assert_eq!(json["severity"], "Warning");
        assert_eq!(json["status"], "Active");
        assert_eq!(json["severity_change"], "Unchanged");
        assert_eq!(json["rationale"][0], "pass-through default");
    }

    #[test]
    fn severity_change_variants_round_trip() {
        for sc in [
            SeverityChange::Unchanged,
            SeverityChange::Downgraded { from: Severity::Fatal },
            SeverityChange::Elevated { from: Severity::Warning },
        ] {
            let json = serde_json::to_string(&sc).unwrap();
            let back: SeverityChange = serde_json::from_str(&json).unwrap();
            assert_eq!(sc, back);
        }
    }

    #[test]
    fn decision_status_distinguishes_suppressed_from_accepted_from_debt() {
        assert_ne!(DecisionStatus::SuppressedByPolicy, DecisionStatus::AcceptedException);
        assert_ne!(DecisionStatus::SuppressedByPolicy, DecisionStatus::KnownTransitionDebt);
        assert_ne!(DecisionStatus::AcceptedException, DecisionStatus::KnownTransitionDebt);
    }
}
