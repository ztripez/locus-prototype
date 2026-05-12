//! DG rules.
//!
//! DG001–DG004 all migrated to `RuleDefinition` in P2/P4 (#71).

pub mod dg001;
pub mod dg002;
pub mod dg003;
pub mod dg004;
pub(super) mod helpers;

// Imports used by rules_tests.rs via `use super::*`.
use crate::diagnostics::{CheckMode, Severity};
use locus_air::{AirItem, AirWorkspace};
use super::lockfile_schema::{DgSection, FeatureDefinition};

#[cfg(test)]
#[path = "../rules_tests.rs"]
mod rules_tests;
