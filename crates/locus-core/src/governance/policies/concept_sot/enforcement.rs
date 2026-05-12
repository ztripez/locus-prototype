//! Severity table for LOCUS005.
//!
//! Maps a concept's declared `enforcement` (Advisory or Enforced)
//! together with the current `CheckMode` (`Human` or `AgentStrict`) to
//! the `(Severity, DecisionStatus)` pair emitted by
//! `ConceptSourceOfTruthPolicy`. Advisory pins to
//! `(Advisory, Advisory)` regardless of mode. Enforced pins to
//! `(mode.elevate(Warning), Active)` — Warning under Human, Fatal
//! under AgentStrict. The unknown-concept-id branch bypasses this and
//! stays Advisory because the signal is config-quality, not an SoT
//! bypass.

// locus: ot canonical

use crate::diagnostics::{CheckMode, Severity};
use crate::governance::arch::ConceptEnforcement;
use crate::governance::decision::DecisionStatus;

/// Map a concept's declared `enforcement` mode (× the current
/// `CheckMode`) to the `(Severity, DecisionStatus)` pair for the
/// emitted LOCUS005:
///
/// - `Advisory` → `(Advisory, DecisionStatus::Advisory)` — the
///   post-#100 MVP behavior; visible but never a gate.
/// - `Enforced` → `(mode.elevate(Warning), DecisionStatus::Active)` —
///   Warning under `Human`, Fatal under `AgentStrict`. Counted as a
///   normal violation in the decision-status taxonomy.
///
/// The unknown-concept-id branch deliberately bypasses this and stays
/// pinned to `Advisory` because that finding is a config-quality
/// signal, not a SoT bypass.
pub(super) fn severity_for(
    enforcement: ConceptEnforcement,
    mode: CheckMode,
) -> (Severity, DecisionStatus) {
    match enforcement {
        ConceptEnforcement::Advisory => (Severity::Advisory, DecisionStatus::Advisory),
        ConceptEnforcement::Enforced => (mode.elevate(Severity::Warning), DecisionStatus::Active),
    }
}
