//! Direct unit coverage of the small policy-internal helpers.

use super::super::emit::is_governance_code_shaped;
use super::super::enforcement::severity_for;
use crate::diagnostics::{CheckMode, Severity};
use crate::governance::arch::ConceptEnforcement;
use crate::governance::decision::DecisionStatus;

#[test]
fn is_governance_code_shaped_helper() {
    assert!(is_governance_code_shaped("LOCUS001"));
    assert!(is_governance_code_shaped("LOCUS999"));
    assert!(!is_governance_code_shaped("CX001"));
    assert!(!is_governance_code_shaped("LOCUS"));
    assert!(!is_governance_code_shaped("LOCUS1"));
    assert!(!is_governance_code_shaped("LOCUS0001"));
}

#[test]
fn severity_for_helper_round_trip() {
    // Direct unit coverage of the (enforcement × mode) → (severity,
    // status) mapping. Pins the table so future refactors can't
    // silently invert the semantics.
    assert_eq!(
        severity_for(ConceptEnforcement::Advisory, CheckMode::Human),
        (Severity::Advisory, DecisionStatus::Advisory)
    );
    assert_eq!(
        severity_for(ConceptEnforcement::Advisory, CheckMode::AgentStrict),
        (Severity::Advisory, DecisionStatus::Advisory)
    );
    assert_eq!(
        severity_for(ConceptEnforcement::Enforced, CheckMode::Human),
        (Severity::Warning, DecisionStatus::Active)
    );
    assert_eq!(
        severity_for(ConceptEnforcement::Enforced, CheckMode::AgentStrict),
        (Severity::Fatal, DecisionStatus::Active)
    );
}
