//! Semantic Rust adapter for Locus.
//!
//! Where `locus-rust` is the fast syntactic adapter (walks `syn` AST,
//! emits Layer 1/2/3/4 records per `docs/RUST_ADAPTER.md`), this crate
//! is the high-fidelity adapter: it resolves names, types, traits, and
//! eventually macro expansions through a real semantic backend.
//!
//! ## Architecture
//!
//! - [`SemanticAdapter`] — the trait every backend implements. Stable
//!   contract; new backends extend it additively.
//! - [`ResolvedConversion`] — the spike's chosen fact shape. Carries
//!   fully-qualified canonical type paths and resolved trait identity
//!   — what `locus-rust`'s syntactic heuristic can't produce.
//! - [`TestBackend`] — in-process adapter that returns hand-built
//!   facts. Used by `locus-core`'s OT integration tests.
//!
//! ## Concrete backends
//!
//! - [`RustdocJsonBackend`] (default feature `rustdoc-json`) — shells
//!   out to nightly `cargo rustdoc -- --output-format json` and parses
//!   the result. Emits resolved `From<T>` / `TryFrom<T>` impl records
//!   with `SemanticBackend::RustdocJson` provenance. Cost: requires a
//!   nightly toolchain.
//! - `RustAnalyzerBackend` — **not implemented yet**. Will land when
//!   call-target resolution becomes the limiting factor (see the
//!   pivot note in
//!   `docs/superpowers/specs/2026-05-13-rustc-semantic-spike.md`).
//!
//! ## Feature flags
//!
//! - `rustdoc-json` (on by default) pulls in `rustdoc-types` +
//!   `serde_json` and exposes [`RustdocJsonBackend`]. Disable with
//!   `default-features = false` if you only want the trait + types
//!   (e.g. for tests, or to wire a different backend behind the same
//!   trait).

use locus_air::{AirConversion, AirSpan, ConversionMechanism, FactProvenance, SemanticBackend};

#[cfg(feature = "rustdoc-json")]
pub mod rustdoc_json;
pub mod test_backend;

#[cfg(feature = "rustdoc-json")]
pub use rustdoc_json::RustdocJsonBackend;
pub use test_backend::TestBackend;

/// Errors a semantic adapter can return. Backends typically fail when
/// the target workspace does not compile, when the toolchain is wrong,
/// or when the backend's dependencies aren't available.
///
/// Adapters are expected to **degrade**, not abort: a workspace that
/// only partially compiles should still yield facts for the parts that
/// resolve. `AdapterError::PartialResolution` carries the resolved
/// facts plus a per-crate failure list.
// locus: ot canonical
#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("backend dependency unavailable: {0}")]
    BackendUnavailable(String),
    #[error("workspace does not compile: {message}")]
    WorkspaceFailed { message: String },
}

/// Contract every semantic backend implements. Kept intentionally narrow
/// in the spike — the only resolved fact this milestone exposes is
/// converter resolution. Future methods (resolved call targets,
/// resolved trait method receivers, …) extend this trait additively.
///
/// Implementations must be deterministic given the same workspace
/// source + toolchain.
// locus: ot canonical
// locus: allow AB001 reason="architectural seam for #111 phase 2; the second impl (RustAnalyzerBackend) lands in the follow-up PR" expires="2026-08-13"
pub trait SemanticAdapter: Send + Sync {
    /// Stable identifier for the backend, used in diagnostics and the
    /// AIR provenance enum. Must match the corresponding
    /// `locus_air::SemanticBackend` variant.
    fn name(&self) -> &'static str;

    /// Resolve `impl From<T>` / `impl TryFrom<T>` blocks in the
    /// workspace rooted at `workspace_root` and return them as fully-
    /// qualified [`ResolvedConversion`] records.
    ///
    /// The returned records' `AirConversion::provenance` is set to
    /// `SemanticResolved { backend }` so OT converter detection can
    /// prefer them over the heuristic emissions from `locus-rust`.
    fn resolve_conversions(
        &self,
        workspace_root: &std::path::Path,
    ) -> Result<Vec<ResolvedConversion>, AdapterError>;
}

/// Adapter-tier output of converter resolution. Wraps an `AirConversion`
/// already tagged with `SemanticResolved` provenance — callers can drop
/// this directly into an `AirWorkspace` without further translation.
///
/// Kept as a separate value type (rather than just `AirConversion`) so
/// future fact shapes — e.g. `ResolvedCallTarget`, `ResolvedTraitImpl`
/// — can grow alongside without forcing every consumer to handle every
/// variant.
// locus: ot canonical
#[derive(Debug, Clone)]
pub struct ResolvedConversion {
    pub air: AirConversion,
}

impl ResolvedConversion {
    /// Build a resolved-conversion record tagged with the given backend.
    /// The `from` / `to` strings are expected to be **fully-qualified**
    /// type paths (e.g. `core::option::Option<crate::User>`), not the
    /// bare names the syntactic adapter renders.
    pub fn new(
        from: impl Into<String>,
        to: impl Into<String>,
        mechanism: ConversionMechanism,
        symbol: impl Into<String>,
        span: AirSpan,
        backend: SemanticBackend,
    ) -> Self {
        Self {
            air: AirConversion {
                from: from.into(),
                to: to.into(),
                mechanism,
                symbol: symbol.into(),
                span,
                provenance: Some(FactProvenance::SemanticResolved { backend }),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_conversion_carries_semantic_backend_provenance() {
        let span = AirSpan::new("t.rs", 1, 1);
        let fact = ResolvedConversion::new(
            "core::option::Option<u32>",
            "core::result::Result<u32, MyError>",
            ConversionMechanism::FallibleAdapter,
            "pkg::MyError::try_from",
            span,
            SemanticBackend::RustAnalyzer,
        );
        assert_eq!(
            fact.air.provenance,
            Some(FactProvenance::SemanticResolved {
                backend: SemanticBackend::RustAnalyzer
            })
        );
    }

    #[test]
    fn resolved_conversion_provenance_outranks_heuristic() {
        // The consumer-side preference logic relies on
        // `FactProvenance::rank` — pin the ordering this crate depends on.
        let semantic = FactProvenance::SemanticResolved {
            backend: SemanticBackend::RustAnalyzer,
        };
        assert!(semantic.rank() > FactProvenance::Heuristic.rank());
        assert!(semantic.rank() > FactProvenance::Syntactic.rank());
        assert!(semantic.rank() > FactProvenance::SourceHint.rank());
    }
}
