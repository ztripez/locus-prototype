# Rustc-backed semantic adapter — phase-1 spike

Issue: [#111](https://github.com/ztripez/locus/issues/111).
Depends on: [#110](https://github.com/ztripez/locus/issues/110) (adapter-boundary doc).

This is the phase-1 spike design note. It records what landed in this
PR and what phase-2 work follows, so the architectural contract — trait
shape, AIR provenance, OT consumer preference — can be reviewed without
the heavyweight `ra-ap-*` backend muddying the discussion.

## What this spike delivers

1. **`locus-rust-semantic` crate skeleton** (`crates/locus-rust-semantic`)
   - `SemanticAdapter` trait — the contract every backend implements.
   - `ResolvedConversion` value type — the fact shape backends emit for
     `impl From<T> for U` / `impl TryFrom<T> for U`. Carries fully-
     qualified type paths, not the bare names today's syntactic adapter
     renders.
   - `TestBackend` — in-process adapter that returns hand-built facts.
     Lets the consumer side (OT converter detection) ship a real,
     tested preference for `SemanticResolved` over `Heuristic` before
     the `ra-ap-*` integration lands.

2. **AIR schema v14** (`crates/locus-air`)
   - New `FactProvenance` enum: `SourceHint` / `Syntactic` / `Heuristic`
     / `SemanticResolved { backend }`.
   - New `SemanticBackend` enum: `RustAnalyzer` / `RustdocJson` /
     `RustcDriver`. Backends not yet implemented are listed so adding
     one later is not a schema bump.
   - `AirConversion.provenance: Option<FactProvenance>` — optional so
     v13 wire data deserialises as `None` (= unknown, defaults to
     `Heuristic`-equivalent at the consumer side).
   - `FactProvenance::rank()` returns a strict ordering so consumers
     can dedup by "keep the most-resolved record."

3. **`locus-rust` tags its emissions** (`crates/locus-rust/src/visitor.rs`)
   - All four `AirConversion` emission sites (free-fn converter,
     `From`/`TryFrom` trait impl, inherent-method converter, free-fn
     converter from `convert_*` / `map_*`) now set
     `provenance: Some(FactProvenance::Heuristic)`. That's the
     honest label for today's adapter per `docs/RUST_ADAPTER.md`.

4. **OT consumer preference** (`crates/locus-core/src/paradigms/one_truth/`)
   - New `helpers::prefer_higher_provenance` deduplicates conversions
     by `(file, line_start, line_end, mechanism)`, keeping the highest-
     rank `FactProvenance` when more than one record covers the same
     impl block.
   - `OT006` and `OT007` consume the helper instead of iterating
     `file.items` directly. With no semantic adapter integrated yet
     this is a no-op in practice (only one record per impl in today's
     output), but it's the consumer-side guarantee phase-2 needs.

5. **End-to-end integration test**
   (`crates/locus-core/tests/semantic_provenance_spike.rs`)
   - `TestBackend` emits a `SemanticResolved` record for the same impl
     line where the syntactic adapter emitted a `Heuristic` record.
   - Asserts OT006 fires **once** (proving the dedup wins, not twice
     which is what would happen without it).
   - Plus two regression-safety tests: heuristic-only still fires;
     duplicate heuristic records still dedupe.

## Cold-scan timing baseline

`locus check --workspace .` against this repo, release build, 5 runs:

```
run 1: 0.427s
run 2: 0.425s
run 3: 0.430s
run 4: 0.432s
run 5: 0.426s
```

Workspace size at time of measurement: 247 `.rs` files, ~57,657 LOC
total. Median 0.427s, range 0.005s. The "cold" / "warm" distinction
is essentially noise at this scale because `locus check` re-scans from
disk every time today — there's no cache to warm.

These numbers are the **before** baseline. Phase 2 must report an
**after** comparison once `RustAnalyzerBackend` is plugged in: cold
load (rust-analyzer's salsa DB has to be built from scratch) is
expected to be in the 5–15s range for a workspace this size; warm
runs against a persisted DB should land under 1s if the cache works.

## Phase 2 — out of scope for this PR

The architectural contract above is the deliverable for phase 1. The
following items are explicitly deferred so the contract can be merged
and reviewed first:

- **`RustAnalyzerBackend` implementation.** Concrete `SemanticAdapter`
  impl against `ra-ap-syntax`, `ra-ap-hir`, `ra-ap-ide-db`, and
  `ra-ap-load-cargo`. Expected dep-tree size: 150–300 transitive
  crates depending on which ra-ap-* family version we pin.
- **Workspace loading + caching.** `LoadCargoConfig` setup, sysroot
  resolution, salsa DB persistence across `locus check` runs.
- **CLI wiring.** A `locus check --semantic-rust` flag that loads the
  semantic adapter and merges its facts into the AIR before paradigm
  rules run. Default off until the cost story is understood.
- **`RustdocJsonBackend` (optional).** Declaration-only backend for
  projects where running rust-analyzer's full pipeline is too heavy.
  Implements the same trait; emits a subset of the facts (no call-site
  or expression resolution).
- **Failure-mode policy.** What happens when the workspace doesn't
  compile? Suggested model: `AdapterError::PartialResolution` carries
  resolved facts + a per-crate failure list; rules see facts marked
  with a future `SemanticPartial` provenance variant. To be designed
  before the CLI wiring lands.
- **Real rust-analyzer-backed timing measurements.** Cold load, warm
  load, salsa DB size on this repo + the `LOCUS_TEST_CORPUS`
  workspace.

## Rule-classification reminder (from #111 research comment)

This spike does **not** change which rules are advisory vs enforced.
But for reviewers picking up phase 2:

| Class | Paradigms | What changes when semantic facts arrive |
|---|---|---|
| Syn-permanent | MO, CX, DG, AB, UT | Nothing — these stay syntactic |
| Advisory-until-semantic | BO, PA, RW, OB, FL | Phase-2 promotes to enforced once `SemanticResolved` provenance is reachable for the underlying call/receiver facts |
| Already benefiting in phase 1 | OT (converter detection) | This spike's consumer side; semantic record wins when present |

## Open questions for the design discussion

1. **Backend behind a feature flag?** `locus-rust-semantic` currently
   has zero heavyweight deps. Phase 2's `RustAnalyzerBackend` should
   probably live behind a `rust-analyzer` cargo feature so users who
   only want syntactic checks don't pay the dep tree.
2. **Provenance on other AIR records.** `AirItem::Impl`, `AirCallSite`,
   `AirFact` — should they also gain `FactProvenance`? The minimum
   spike only covers `AirConversion`. Phase-2 rules (BO/PA/RW) will
   need it on `AirCallSite` at minimum.
3. **CLI default.** `locus check --semantic-rust` opt-in vs opt-out.
   Suggested: opt-in until cold-cache timing is measured.
4. **Strict-policy hooks.** A future Policy Guard rule could refuse to
   fire Fatal on a `Heuristic`-only finding. Out of scope for the
   spike; recorded so the AIR shape stays usable for it.
