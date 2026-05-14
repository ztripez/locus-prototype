//! `RustdocJsonBackend` — first concrete [`SemanticAdapter`] implementation.
//!
//! Shells out to `cargo +nightly rustdoc -- -Zunstable-options
//! --output-format json` per workspace package, reads the resulting
//! JSON, and emits one [`ResolvedConversion`] per `impl From<T> for U`
//! / `impl TryFrom<T> for U` it finds with fully-qualified endpoint
//! paths and resolved trait identity.
//!
//! Why rustdoc JSON for the first concrete backend (vs `ra-ap-*`): see
//! `docs/superpowers/specs/2026-05-13-rustc-semantic-spike.md` §
//! "Phase-2 backend pivot."
//!
//! ## Module layout
//!
//! - [`backend`] — `RustdocJsonBackend` struct + `SemanticAdapter`
//!   trait impl.
//! - [`cargo_invoke`] — `cargo metadata` / `cargo rustdoc` shelling
//!   and JSON loading.
//! - [`walk`] — rustdoc-types JSON → [`ResolvedConversion`] translation.
//!
//! ## Cost
//!
//! Requires a nightly toolchain available as `cargo +nightly`.
//! Backends that can't satisfy that prerequisite report
//! [`AdapterError::BackendUnavailable`] instead of panicking; callers
//! can fall back to the syntactic adapter's heuristic output.
//!
//! ## Known limitations
//!
//! - **public-API only.** rustdoc JSON normally excludes private
//!   items. `From` / `TryFrom` impls are public almost by definition,
//!   so this rarely bites in practice.
//! - **no call-target resolution.** Out of scope for this backend;
//!   that's the rust-analyzer-backed second-backend story.

// locus: ot canonical

#[allow(unused_imports)] // re-export used by downstream crates
use crate::{AdapterError, ResolvedConversion, SemanticAdapter};

mod backend;
mod cargo_invoke;
mod walk;

pub use backend::RustdocJsonBackend;
