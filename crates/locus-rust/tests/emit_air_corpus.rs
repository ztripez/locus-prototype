//! Integration test against a real-world corpus.
//!
//! Skipped (silently passes) when `LOCUS_TEST_CORPUS` is unset. When set, the
//! test scans the corpus and asserts a few coarse invariants — no panic, at
//! least one package, plausible item count. We deliberately do NOT snapshot
//! the corpus output; it churns and the diff is unreviewable.

use locus_air::AirItem;
use std::path::PathBuf;

#[test]
fn scans_corpus_when_env_set() {
    let Some(corpus) = std::env::var_os("LOCUS_TEST_CORPUS") else {
        eprintln!("LOCUS_TEST_CORPUS unset; skipping corpus scan");
        return;
    };
    let path = PathBuf::from(corpus);
    assert!(
        path.is_dir(),
        "LOCUS_TEST_CORPUS={} is not a directory",
        path.display()
    );

    let air = locus_rust::scan(&path).expect("corpus scan succeeds");
    assert!(!air.packages.is_empty(), "corpus has at least one package");

    let total_items: usize = air
        .packages
        .iter()
        .flat_map(|p| p.files.iter())
        .map(|f| f.items.len())
        .sum();
    assert!(
        total_items > 100,
        "expected >100 AIR items across the corpus, got {total_items}"
    );

    // Sanity-check the data: at least one type and one function should exist.
    let kinds = air
        .packages
        .iter()
        .flat_map(|p| p.files.iter())
        .flat_map(|f| f.items.iter())
        .fold((0usize, 0usize, 0usize), |(t, fns, c), it| match it {
            AirItem::Type(_) => (t + 1, fns, c),
            AirItem::Function(_) => (t, fns + 1, c),
            AirItem::Conversion(_) => (t, fns, c + 1),
            _ => (t, fns, c),
        });
    assert!(kinds.0 > 0, "expected at least one Type");
    assert!(kinds.1 > 0, "expected at least one Function");
    eprintln!(
        "corpus scan: packages={}, types={}, fns={}, conversions={}, items={}",
        air.packages.len(),
        kinds.0,
        kinds.1,
        kinds.2,
        total_items
    );
}
