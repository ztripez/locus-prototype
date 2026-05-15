# Rustc-backed semantic adapter — phase-1 spike

Issue: [#111](https://github.com/ztripez/locus/issues/111).
Depends on: [#110](https://github.com/ztripez/locus/issues/110) (adapter-boundary doc).

This is the phase-1 spike design note. It records what landed in PR
[#115](https://github.com/ztripez/locus/pull/115) (now merged) and what
phase-2 work follows, so the architectural contract — trait shape,
AIR provenance, OT consumer preference — could be reviewed
independently from the concrete backend.

## Phase-2 backend pivot (2026-05-14)

The phase-1 spike was framed as "rust-analyzer first, rustdoc JSON
optional second." Phase-2 implementation work turned up a real
obstacle: **`ra-ap-load-cargo` is not published to crates.io.** It's
~1050 lines of workspace-internal glue in the rust-analyzer repo. Its
dependencies are all published, so vendoring is possible, but the cost
is substantial: tracking rust-analyzer's weekly releases, salsa
database wiring, proc-macro server IPC.

For the spike's chosen fact (resolved `impl From<T>` /
`impl TryFrom<T>`), **rustdoc JSON is the strictly simpler tool** —
~200–400 lines vs ~1000+ vendored, no salsa, lighter dep tree, and it
already gives us fully-qualified type paths and resolved trait
identity. Call-target resolution (which is rust-analyzer's unique
strength) is **not** in this slice anyway.

Decision: **`RustdocJsonBackend` becomes the first concrete backend.**
`RustAnalyzerBackend` stays in the trait's `SemanticBackend` enum so
the AIR shape is stable, and lands as a follow-up when call-target
resolution becomes the limiting factor (e.g. for the BO/PA/RW
paradigms listed under "advisory-until-semantic" in the research
comment on #111).

The `SemanticAdapter` trait, `FactProvenance` enum, and OT consumer
preference all stay exactly as shipped in phase 1 — only the first
concrete backend changes.

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
     by `(file, line_start, line_end, mechanism, normalize(from),
     normalize(to))`, keeping the highest-rank `FactProvenance` when
     more than one record covers the same impl block.
   - **Overlay contract**: `normalize` strips module paths to the
     trailing identifier (`crate::dto::UserDto` → `UserDto`). This is
     what lets the semantic backend emit fully-qualified canonical
     endpoints (per `ResolvedConversion`'s contract) and have its
     records overlay on top of the syntactic adapter's bare-name
     emissions without the semantic backend degrading its fact shape.
   - Distinct impls on the same line (`impl From<A> for B {} impl
     From<C> for D {}`) are NOT collapsed because their normalized
     endpoints differ — this is the Codex P1 regression from #115.
   - **Known limitation**: generic endpoints carrying canonical paths
     inside their parameters (`Vec<crate::path::X>` vs `Vec<X>`) do
     NOT normalize to the same key today. Real-world conversion
     endpoints are usually concrete types; phase 2 can revisit when
     `RustAnalyzerBackend`'s actual emission shape is known.
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

These numbers are the **before** baseline.

### Phase-2 `RustdocJsonBackend` measurements

Measured on the same machine (release build):

| Workspace                              | Cold (target/ wiped) | Warm (target/ cached) |
|---|---|---|
| `tests/fixtures/semantic-conversions-fixture` (~30 LoC, 0 deps) | ~0.55s | ~0.49s |
| `crates/locus-air` (1062 LoC, depends on `serde`)               | ~6.3s  | (compile-cache dominated) |

Order-of-magnitude shape: rustdoc-JSON's cost is **the cost of a full
nightly `cargo build` of the crate plus rustdoc's JSON emission.** On
a tiny dep-free fixture that's ~500ms; on a single Locus crate with
`serde` it's ~6s; on the whole Locus workspace cold it would land in
the 30–60s range before any caching.

Implication for CLI integration (phase 3): semantic-rust must be
opt-in (`locus check --semantic-rust` or similar) and rely on
`target/doc/` reuse across `locus check` invocations, not run on every
invocation. Phase 1's 0.4s `locus check` baseline is preserved by
default; the semantic adapter is an explicit-cost upgrade.

A future `RustAnalyzerBackend` (whenever call-target resolution
becomes required) would have a different cost profile — salsa's
incremental queries can be sub-second after warm cache where rustdoc
JSON is always a fresh build of the crate.

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
- ~~**CLI wiring.**~~ Landed in #111 phase 3. `locus check
  --semantic-rust` opt-in flag invokes `RustdocJsonBackend` after the
  syntactic scan and appends resolved `AirConversion` records to the
  matching files before governance runs. Backend failures
  (`BackendUnavailable` for missing nightly, `WorkspaceFailed` for
  source that won't compile) emit a stderr advisory and fall back to
  syntactic-only — they never fail `locus check`. Implementation:
  `crates/locus-cli/src/semantic_facts.rs`. Span match is suffix-
  based: the backend emits workspace-relative spans, `locus-rust`
  emits absolute paths; the merger places each resolved record into
  the file whose path ends with the backend's span.
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
