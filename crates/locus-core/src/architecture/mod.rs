//! Architecture facts loaded from structured sources.
//!
//! Boundary between source-specific parsers (OpenAPI, ADRs, ...) and
//! governance policy logic. Source loaders construct
//! [`ArchitectureFacts`]; policies consume them. Every fact carries a
//! [`SourceRef`] so diagnostics can explain where a declaration came from.
//!
//! Documents do not decide. Documents declare. Policies decide.
//!
//! Empty facts are cheap (default constructor allocates nothing); current
//! behavior of "no architecture sources loaded" maps to
//! `ArchitectureFacts::default()`.
//!
//! ## Module layout
//!
//! Each fact type lives in its own file so the contract surface stays
//! within MO001's 5-public-types-per-module budget. New fact kinds
//! should follow the same pattern.

pub mod boundary;
pub mod concept;
pub mod contract;
pub mod converter;
pub mod debt;
pub mod facts;
pub mod module_ownership;
pub mod source;

pub use boundary::{BoundaryFact, BoundaryKind};
pub use concept::ConceptFact;
pub use contract::ContractFact;
pub use converter::ConverterFact;
pub use debt::{DebtFact, DebtTarget};
pub use facts::ArchitectureFacts;
pub use module_ownership::ModuleOwnershipFact;
pub use source::SourceRef;

#[cfg(test)]
#[path = "../architecture_tests.rs"]
mod tests;
