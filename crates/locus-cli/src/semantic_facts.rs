//! `--semantic-rust` plumbing for `locus check`.
//!
//! Invokes [`locus_rust_semantic::RustdocJsonBackend`] against the
//! workspace, then merges each resolved [`ResolvedConversion`] into the
//! matching `AirFile.items` so OT's converter rules see the
//! `SemanticResolved` records alongside the syntactic adapter's
//! `Heuristic` ones. The OT consumer
//! ([`locus_core::paradigms::one_truth::rules::helpers::prefer_higher_provenance`])
//! handles dedup + provenance preference.
//!
//! ## Failure policy
//!
//! Semantic-rust is opt-in. The CLI must not fail `locus check` when
//! the backend itself fails — fall back to syntactic facts and warn:
//!
//! - `AdapterError::BackendUnavailable` — nightly toolchain missing,
//!   rustdoc JSON format-version mismatch, etc. Print a one-line
//!   advisory to stderr; return Ok with the AIR unchanged.
//! - `AdapterError::WorkspaceFailed` — workspace doesn't compile.
//!   Print a louder advisory (semantic facts would be unsound without
//!   compilation); return Ok with the AIR unchanged.
//!
//! Either way the syntactic adapter's emissions are still in the AIR,
//! so `locus check` produces the same baseline output it would
//! without the flag.
//!
//! ## Span matching
//!
//! `RustdocJsonBackend` emits spans whose `file` is workspace-relative
//! (e.g. `src/lib.rs`). `locus-rust`'s scanner emits absolute paths
//! (e.g. `/path/to/crate/src/lib.rs`). The matcher is suffix-based:
//! the file whose `path.ends_with(span.file)` wins. Unambiguous in
//! practice — each crate has one `src/lib.rs` and the workspace tree
//! disambiguates the rest. Records whose span doesn't match any
//! scanned file are dropped with a per-record stderr note.

// locus: ot boundary cli.check cli

use std::io::Write;
use std::path::Path;

use locus_air::{AirItem, AirWorkspace};
use locus_rust_semantic::{AdapterError, RustdocJsonBackend, SemanticAdapter};

pub fn merge_semantic_conversions(air: &mut AirWorkspace, workspace_root: &Path) {
    let backend = RustdocJsonBackend::new();
    let resolved = match backend.resolve_conversions(workspace_root) {
        Ok(r) => r,
        Err(AdapterError::BackendUnavailable(msg)) => {
            warn(&format!(
                "semantic-rust skipped: {msg} (continuing with syntactic adapter)"
            ));
            return;
        }
        Err(AdapterError::WorkspaceFailed { message }) => {
            warn(&format!(
                "semantic-rust skipped: workspace did not compile: {message} \
                 (continuing with syntactic adapter)"
            ));
            return;
        }
    };
    let appended = merge_into_files(air, resolved);
    if appended == 0 {
        warn("semantic-rust: backend produced no resolved conversions");
    }
}

/// Append each resolved record to the matching file's items. Returns
/// how many records were placed. A record whose span doesn't match
/// any file gets skipped with a per-record warning — that means the
/// backend resolved an impl in a file `locus-rust` didn't scan, which
/// would be unexpected.
fn merge_into_files(
    air: &mut AirWorkspace,
    resolved: Vec<locus_rust_semantic::ResolvedConversion>,
) -> usize {
    let mut placed = 0usize;
    for record in resolved {
        let target_file = record.air.span.file.clone();
        let placed_here = place_record(air, &target_file, record);
        if placed_here {
            placed += 1;
        }
    }
    placed
}

fn place_record(
    air: &mut AirWorkspace,
    target_file: &str,
    record: locus_rust_semantic::ResolvedConversion,
) -> bool {
    for pkg in &mut air.packages {
        for file in &mut pkg.files {
            if file.path == target_file || file.path.ends_with(target_file) {
                file.items.push(AirItem::Conversion(record.air));
                return true;
            }
        }
    }
    warn(&format!(
        "semantic-rust: no scanned file matched span `{target_file}` — dropping resolved conversion",
    ));
    false
}

fn warn(msg: &str) {
    let stderr = std::io::stderr();
    let mut h = stderr.lock();
    let _ = writeln!(h, "warning: {msg}");
}
