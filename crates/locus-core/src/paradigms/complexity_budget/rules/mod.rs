//! CX rule implementations.
//!
//! CX001–CX002–CX007–CX008 all migrated to `RuleDefinition` in P2/P4 (#71).

pub mod cx001;
pub mod cx002;
pub mod cx007;
pub mod cx008;

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
