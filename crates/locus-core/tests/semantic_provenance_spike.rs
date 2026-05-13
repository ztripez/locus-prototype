//! Spike integration test (#111): semantic adapters that emit
//! `SemanticResolved` `AirConversion`s for the same impl block as the
//! syntactic `Heuristic` emissions must win at the consumer side.
//!
//! Today's `locus-rust` tags every converter as `Heuristic` (last-segment
//! text equality on `From`/`TryFrom`, name-shape match on inherent
//! methods, etc.). When `locus-rust-semantic`'s
//! `RustAnalyzerBackend` lands in phase 2, it will overlay
//! `SemanticResolved` records on the same impl spans with fully-
//! qualified type paths. This test pins the consumer-side guarantee:
//! OT converter rules prefer the semantic record.
//!
//! Spec / scope: see
//! `docs/superpowers/specs/2026-05-13-rustc-semantic-spike.md`.

use std::collections::BTreeMap;
use std::path::Path;

use locus_air::{
    AIR_SCHEMA_VERSION, AirConversion, AirFile, AirItem, AirPackage, AirSpan, AirWorkspace,
    ConversionMechanism, FactProvenance, SemanticBackend,
};
use locus_core::paradigms::one_truth::lockfile_schema::{
    AcceptedBoundary, AcceptedCanonical, ConceptEntry, OtSection, Source,
};
use locus_core::{CheckMode, Lockfile, governance};
use locus_rust_semantic::{ResolvedConversion, SemanticAdapter, TestBackend};

/// Build the same impl-block AIR shape that the syntactic adapter would
/// emit today: an `AirConversion` tagged `Heuristic`, with bare type
/// names (no canonical paths).
fn syntactic_conversion(file: &str, line: u32) -> AirItem {
    AirItem::Conversion(AirConversion {
        from: "UserDto".into(),
        to: "User".into(),
        mechanism: ConversionMechanism::FallibleAdapter,
        symbol: "crate::dto::impl TryFrom<UserDto> for User".into(),
        span: AirSpan::new(file, line, line),
        provenance: Some(FactProvenance::Heuristic),
    })
}

/// What a semantic backend would emit for the same impl block: fully-
/// qualified type paths and `SemanticResolved` provenance.
fn semantic_conversion(file: &str, line: u32) -> ResolvedConversion {
    ResolvedConversion::new(
        "crate::dto::UserDto",
        "crate::identity::User",
        ConversionMechanism::FallibleAdapter,
        "crate::dto::impl TryFrom<UserDto> for User",
        AirSpan::new(file, line, line),
        SemanticBackend::RustAnalyzer,
    )
}

fn lockfile_with_concept() -> Lockfile {
    // A minimal OT section that registers `User` as the canonical and
    // `UserDto` as a boundary — no converter is accepted, so OT006 will
    // fire if it sees the (User, UserDto) edge at all.
    let mut concepts = BTreeMap::new();
    concepts.insert(
        "user".to_string(),
        ConceptEntry {
            canonical: AcceptedCanonical {
                symbol: "crate::identity::User".into(),
                source: Source::Hint,
            },
            boundaries: vec![AcceptedBoundary {
                symbol: "crate::dto::UserDto".into(),
                boundary: Some("dto".into()),
                source: Source::Hint,
            }],
            converters: Vec::new(),
        },
    );
    let section = OtSection {
        concepts,
        converter_paths: Vec::new(),
    };
    let mut lf = Lockfile::empty();
    lf.paradigms.insert(
        "OT".into(),
        serde_json::to_value(&section).expect("ot section serialises"),
    );
    lf
}

fn workspace_with_items(items: Vec<AirItem>) -> AirWorkspace {
    AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "spike-pkg".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: "src/converters.rs".into(),
                module_path: Some("spike_pkg::converters".to_string()),
                items,
                hints: Vec::new(),
                parse_error: None,
                line_count: 50,
            }],
        }],
        facts: Vec::new(),
    }
}

#[test]
fn test_backend_emits_resolved_conversion() {
    // Confirm the TestBackend round-trips a hand-built fact with
    // SemanticResolved provenance. This is the "the trait works"
    // assertion before we get to the consumer-side behaviour.
    let backend = TestBackend::new().with_fact(semantic_conversion("src/converters.rs", 12));
    let facts = backend
        .resolve_conversions(Path::new("/"))
        .expect("test backend cannot fail");
    assert_eq!(facts.len(), 1);
    assert_eq!(
        facts[0].air.provenance,
        Some(FactProvenance::SemanticResolved {
            backend: SemanticBackend::RustAnalyzer
        }),
    );
    assert_eq!(facts[0].air.from, "crate::dto::UserDto");
    assert_eq!(facts[0].air.to, "crate::identity::User");
}

#[test]
fn ot_consumer_prefers_semantic_resolution_over_heuristic() {
    // Same impl block emitted twice: once by today's syntactic adapter
    // (Heuristic), once by a future semantic adapter (SemanticResolved).
    // The OT rules must see only the resolved record — confirmed by
    // checking that OT006 fires exactly once on this file (instead of
    // twice, which is what would happen without dedup).
    let backend = TestBackend::new().with_fact(semantic_conversion("src/converters.rs", 12));
    let resolved = backend
        .resolve_conversions(Path::new("/"))
        .expect("test backend cannot fail");

    let mut items = vec![syntactic_conversion("src/converters.rs", 12)];
    items.extend(resolved.into_iter().map(|r| AirItem::Conversion(r.air)));

    let air = workspace_with_items(items);
    let lockfile = lockfile_with_concept();

    let out = governance::run(&air, &lockfile, CheckMode::Human);

    let ot006: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "OT006")
        .collect();
    assert_eq!(
        ot006.len(),
        1,
        "expected exactly one OT006 (semantic record wins, heuristic is \
         deduped away); got {} diagnostics: {ot006:?}",
        ot006.len(),
    );
}

#[test]
fn ot_consumer_falls_back_to_heuristic_when_no_semantic_record_present() {
    // When no semantic adapter has run, OT must still fire on the
    // heuristic record. This is the "phase-1 doesn't break phase-0"
    // assertion.
    let air = workspace_with_items(vec![syntactic_conversion("src/converters.rs", 12)]);
    let lockfile = lockfile_with_concept();
    let out = governance::run(&air, &lockfile, CheckMode::Human);
    let ot006: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "OT006")
        .collect();
    assert_eq!(
        ot006.len(),
        1,
        "expected exactly one OT006 from the heuristic record alone; \
         got {ot006:?}"
    );
}

#[test]
fn ot_consumer_dedupes_two_heuristic_records_on_the_same_impl() {
    // Defensive: if the syntactic adapter ever double-emits (e.g. if
    // both the trait-impl and inherent-method paths fired for the same
    // span — they shouldn't, but the dedup logic should be safe), OT
    // still fires once. This is the "same-rank, same-impl ⇒ one
    // finding" guarantee.
    let air = workspace_with_items(vec![
        syntactic_conversion("src/converters.rs", 12),
        syntactic_conversion("src/converters.rs", 12),
    ]);
    let lockfile = lockfile_with_concept();
    let out = governance::run(&air, &lockfile, CheckMode::Human);
    let ot006: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "OT006")
        .collect();
    assert_eq!(
        ot006.len(),
        1,
        "duplicate heuristic records on the same impl must dedupe; \
         got {ot006:?}"
    );
}
