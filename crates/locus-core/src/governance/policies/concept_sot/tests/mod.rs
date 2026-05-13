//! Unit tests for `ConceptSourceOfTruthPolicy`.
//!
//! Split across submodules by behavior to keep each file under CX002's
//! 400-line module budget:
//!
//! - `fixtures` — shared finding/concept constructors and `run_with*`
//!   helpers (test-only). Re-exported via `pub(super) use` so sibling
//!   test modules can call them via `super::*`.
//! - `silent`   — cases where LOCUS005 must NOT fire (registered
//!   identifiers, empty arch declarations, legacy diagnostics).
//! - `firing`   — cases where LOCUS005 fires, plus dedupe across
//!   repeated bypass observations and unknown-concept-id emission.
//! - `advisory` — Advisory-mode semantics (stays Advisory under
//!   AgentStrict, unknown-concept stays Advisory).
//! - `enforced` — Enforced-mode semantics (Warning/Fatal elevation,
//!   span propagation, per-concept mode mixing).
//! - `helpers`  — direct unit coverage of the small helper fns
//!   (`severity_for`, `is_governance_code_shaped`).

mod advisory;
mod enforced;
mod firing;
mod fixtures;
mod helpers;
mod silent;

pub(super) use fixtures::*;
