//! DG rules.
//!
//! DG001–DG004 all migrated to `RuleDefinition` in P2/P4 (#71).

pub mod dg001;
pub mod dg002;
pub mod dg003;
pub mod dg004;
pub(super) mod helpers;

// Imports re-exported into the inline test module (rules_tests.rs uses `use super::*`).
#[cfg(test)]
#[allow(unused_imports)]
use crate::diagnostics::{CheckMode, Severity};
#[cfg(test)]
#[allow(unused_imports)]
use locus_air::{AirItem, AirWorkspace};

#[cfg(test)]
#[path = "../rules_tests.rs"]
mod rules_tests;
