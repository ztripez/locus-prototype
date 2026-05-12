//! CX rule implementations.
//!
//! CX001–CX002–CX007–CX008 all migrated to `RuleDefinition` in P2/P4 (#71).

pub mod cx001;
pub mod cx002;
pub mod cx007;
pub mod cx008;

// Imports used by rules_tests.rs via `use super::*`.
use super::lockfile_schema::{CxSection, matches_pattern};
use crate::diagnostics::{CheckMode, Severity};
use locus_air::{AirItem, AirWorkspace, Visibility};

#[cfg(test)]
#[path = "../rules_tests.rs"]
mod rules_tests;
