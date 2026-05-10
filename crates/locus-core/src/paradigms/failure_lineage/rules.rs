//! FL rule implementations.
//!
//! Each rule lives in its own sub-module under `rules/`. This file is the
//! registration entry point — it re-exports each rule's public function so
//! call sites in `mod.rs` continue to write `rules::fl001(...)`.
//!
//! Implemented:
//! - [`fl001`]: a function in a domain module returns `Result<_, E>` where E
//!   is a declared boundary error type.
//! - [`fl002`]: a "panic-shaped" callee (`unwrap` / `expect` /
//!   `unwrap_or_default` / `panic` / `todo` / `unimplemented`) fires from a
//!   file whose `module_path` is not in `invariant_owner_paths`.
//! - [`fl003`]: a silent-discard method call (`.ok()` / `.err()` /
//!   `.unwrap_or_else()`) outside `invariant_owner_paths`.
//! - [`fl004`]: a `let _ = expr;` discarded binding outside
//!   `invariant_owner_paths`, where `expr` is a call (`Method` /
//!   `Function` / `Macro`) and the callee isn't on the
//!   `silent_discard_allowed_callees` allowlist.
//! - [`fl005`]: an `if let Ok(...) = expr { ... }` or `if let Err(...) =
//!   expr { ... }` with no `else` branch outside `invariant_owner_paths`.
//! - [`fl006`]: a `.map_err(|_| ...)` call that discards the closure's
//!   error argument outside `invariant_owner_paths`.
//! - [`fl007`]: a catch-all `Err(_) => <silent>` match arm outside
//!   `invariant_owner_paths`.
//! - [`fl010`]: a `.unwrap_or(...)` / `.or(...)` call whose default
//!   argument is a `Literal` or `Call` outside `invariant_owner_paths`.
//! - [`fl011`]: a bare `_ => <silent>` arm outside `invariant_owner_paths`.
//! - [`fl012`]: a `loop` / `for` / `while` whose body uses `?` and has
//!   at least one `break`, outside `retry_policy_owner_paths`.
//! - [`fl013`]: a function returning `Result<_, String>` or `Result<_, &str>`
//!   that contains a call site stringifying via `to_string` / `format!` /
//!   `format` / `display`.

mod helpers;

// Re-export types consumed by tests (rules_tests.rs uses `use super::*;`).
// These are the same types that the old monolithic rules.rs imported at file
// scope, which made them visible to the inline test module.
#[cfg(test)]
pub(crate) use super::lockfile_schema::FlSection;
#[cfg(test)]
pub(crate) use crate::diagnostics::{CheckMode, Severity};
#[cfg(test)]
pub(crate) use locus_air::{AirCallSite, AirClosureMethodCall, AirItem, AirMatchArm, CallKind};

pub mod fl001;
pub mod fl002;
pub mod fl003;
pub mod fl004;
pub mod fl005;
pub mod fl006;
pub mod fl007;
pub mod fl010;
pub mod fl011;
pub mod fl012;
pub mod fl013;

pub use fl001::fl001;
pub use fl002::fl002;
pub use fl003::fl003;
pub use fl004::fl004;
pub use fl005::fl005;
pub use fl006::fl006;
pub use fl007::fl007;
pub use fl010::fl010;
pub use fl011::fl011;
pub use fl012::fl012;
pub use fl013::fl013;

// Test-only re-exports so rules_tests.rs (compiled as a submodule of this
// file) can call private helpers via `use super::*;` as before the split.
#[cfg(test)]
pub(crate) use helpers::extract_result_error_type;

#[cfg(test)]
#[path = "rules_tests.rs"]
mod rules_tests;
