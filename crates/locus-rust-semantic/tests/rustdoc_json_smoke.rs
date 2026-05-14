//! Integration test for `RustdocJsonBackend` (#111 phase 2).
//!
//! Runs the backend end-to-end against
//! `tests/fixtures/semantic-conversions-fixture/`:
//!
//!   1. shells out to `cargo +nightly rustdoc -- -Zunstable-options
//!      --output-format json`,
//!   2. parses the resulting JSON via `rustdoc-types`,
//!   3. asserts the two user-written impls in the fixture surface as
//!      [`ResolvedConversion`] records with canonical paths,
//!      `SemanticBackend::RustdocJson` provenance, and the right
//!      `ConversionMechanism`.
//!
//! ## Why this test is gated
//!
//! The backend requires a nightly toolchain (`cargo +nightly rustdoc`).
//! CI environments without nightly will see `AdapterError::Backend-
//! Unavailable` from the backend; the test treats that as **skip**
//! rather than **fail**, mirroring how `emit_air_corpus.rs` gates on
//! `LOCUS_TEST_CORPUS`. To run the test, ensure `cargo +nightly` is
//! available.
//!
//! The fixture is deliberately tiny (~30 LoC, no external deps) so
//! the rustdoc build adds <1s to the test run.

#![cfg(feature = "rustdoc-json")]

use std::path::PathBuf;

use locus_air::{ConversionMechanism, FactProvenance, SemanticBackend};
use locus_rust_semantic::{AdapterError, RustdocJsonBackend, SemanticAdapter};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/semantic-conversions-fixture")
        .canonicalize()
        .expect("fixture path resolves")
}

/// Skip helper: nightly may be unavailable on minimal CI images. Treat
/// `BackendUnavailable` as "skip the test" rather than a failure so the
/// integration suite doesn't go red where the prereq isn't installed.
fn skip_if_nightly_missing(err: &AdapterError) -> bool {
    matches!(err, AdapterError::BackendUnavailable(_))
}

#[test]
fn resolves_from_and_tryfrom_impls_end_to_end() {
    let backend = RustdocJsonBackend::new();
    let resolved = match backend.resolve_conversions(&fixture_root()) {
        Ok(r) => r,
        Err(e) if skip_if_nightly_missing(&e) => {
            eprintln!("skipping: nightly toolchain unavailable: {e}");
            return;
        }
        Err(e) => panic!("backend failed: {e}"),
    };

    // Exactly the two user-written impls — `From<UserDto> for User` and
    // `TryFrom<&str> for UserId`. Compiler-implied blanket projections
    // (`impl<T> From<T> for T`, etc.) must be filtered out.
    assert_eq!(
        resolved.len(),
        2,
        "expected exactly two resolved impls, got {}: {resolved:#?}",
        resolved.len(),
    );

    // `impl From<UserDto> for User` — InfallibleAdapter. Canonical
    // path uses the crate name with hyphens replaced; the fixture's
    // package is `semantic-conversions-fixture` → crate
    // `semantic_conversions_fixture`. Match by mechanism + suffix on
    // the `to` type so the assertion survives crate-name renames.
    let into_user = resolved
        .iter()
        .find(|r| {
            r.air.mechanism == ConversionMechanism::InfallibleAdapter
                && (r.air.to == "User" || r.air.to.ends_with("::User"))
        })
        .expect("missing `impl From<UserDto> for User`");
    assert_eq!(
        into_user.air.mechanism,
        ConversionMechanism::InfallibleAdapter
    );
    assert!(
        into_user.air.from.ends_with("UserDto"),
        "expected from to end with `UserDto`; got `{}`",
        into_user.air.from,
    );
    assert_eq!(
        into_user.air.provenance,
        Some(FactProvenance::SemanticResolved {
            backend: SemanticBackend::RustdocJson
        }),
    );

    // `impl TryFrom<&str> for UserId` — FallibleAdapter.
    let into_user_id = resolved
        .iter()
        .find(|r| r.air.mechanism == ConversionMechanism::FallibleAdapter)
        .expect("missing `impl TryFrom<&str> for UserId`");
    assert!(
        into_user_id.air.to.ends_with("UserId"),
        "expected to end with `UserId`; got `{}`",
        into_user_id.air.to,
    );
    assert!(
        into_user_id.air.from.contains("str"),
        "expected from to mention `str`; got `{}`",
        into_user_id.air.from,
    );
    assert_eq!(
        into_user_id.air.provenance,
        Some(FactProvenance::SemanticResolved {
            backend: SemanticBackend::RustdocJson
        }),
    );

    // Every record should have a span pointing into the fixture's
    // `src/lib.rs`. Macro-expanded items return a workspace-root span;
    // we know the fixture has none, so this catches a regression in
    // span propagation.
    for r in &resolved {
        assert!(
            r.air.span.file.ends_with("lib.rs"),
            "span should point at fixture's lib.rs; got {:?}",
            r.air.span,
        );
        assert!(
            r.air.span.line_start > 0,
            "span line_start should be >0; got {:?}",
            r.air.span,
        );
    }
}
