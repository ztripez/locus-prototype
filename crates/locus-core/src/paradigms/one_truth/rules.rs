//! OT rules.
//!
//! Each rule lives in its own sub-module under `rules/`. This file is the
//! registration entry point — it re-exports each rule's static singleton
//! so call sites in registries can reference `rules::ot001::OT001_RULE` etc.
//!
//! Implemented:
//! - [`ot001`]: duplicate canonical for a single concept
//! - [`ot002`]: undeclared concept-shaped type (warning by default)
//! - [`ot003`]: boundary type leaked into a non-boundary function signature
//! - [`ot004`]: direct canonical construction outside owner / accepted converter
//! - [`ot005`]: accepted boundary with no accepted converter
//! - [`ot006`]: unregistered conversion between accepted endpoints
//! - [`ot007`]: adapter-to-adapter conversion (both endpoints are boundaries)
//! - [`ot008`]: domain-shaped method on an accepted boundary
//! - [`ot009`]: scattered validation/normalization outside the canonical owner
//! - [`ot010`]: shadow enum overlapping an accepted canonical enum
//! - [`ot011`]: shadow newtype/value object overlapping a canonical value object
//! - [`ot012`]: primitive-typed field where a canonical value object is expected
//!
//! All rules except OT001/OT002 are lockfile-driven — they stay silent until
//! `locus init` (or `locus accept`) has populated the OT section. This is
//! deliberate: pre-onboarding, we don't have the data to distinguish
//! intent from drift.

mod helpers;

// Re-export types consumed by tests (rules_tests.rs uses `use super::*;`).
// These are the same types that the old monolithic rules.rs imported at file
// scope, which made them visible to the inline test module.
#[cfg(test)]
pub(crate) use super::lockfile_schema::OtSection;
#[cfg(test)]
pub(crate) use crate::diagnostics::{CheckMode, Severity};
#[cfg(test)]
pub(crate) use locus_air::{ActionKind, AirItem, HintKind};

pub mod ot001;
pub mod ot002;
pub mod ot003;
pub mod ot004;
pub mod ot005;
pub mod ot006;
pub mod ot007;
pub mod ot008;
pub mod ot009;
pub mod ot010;
pub mod ot011;
pub mod ot012;

// All rules migrated to RuleDefinition (#71 P4).
pub use ot001::OT001_RULE;
pub use ot002::{OT002_RULE, Ot002Rule};
pub use ot003::OT003_RULE;
pub use ot004::OT004_RULE;
pub use ot005::OT005_RULE;
pub use ot006::OT006_RULE;
pub use ot007::OT007_RULE;
pub use ot008::OT008_RULE;
pub use ot009::OT009_RULE;
pub use ot010::OT010_RULE;
pub use ot011::OT011_RULE;
pub use ot012::OT012_RULE;

// Test-only re-exports so rules_tests.rs (compiled as a submodule of this
// file) can call private helpers via `use super::*;` as before the split.
#[cfg(test)]
pub(crate) use helpers::{is_primitive_type_text, snake_to_pascal, type_text_references};

#[cfg(test)]
#[path = "rules_tests.rs"]
mod rules_tests;
