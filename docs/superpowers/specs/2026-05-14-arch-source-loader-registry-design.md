# Architecture source loader registry — design

Slice of epic [#106](https://github.com/ztripez/locus/issues/106) — sub-issue [#108](https://github.com/ztripez/locus/issues/108).

## Goal

Add a deterministic loader registry that walks the architecture sources declared in `.locus/arch.json`, invokes the matching `ArchSourceLoader`, and merges the emitted facts into a single `ArchitectureFacts` value carried on `GovernanceOutput`.

```text
.locus/arch.json [.sources]
        ↓
ArchSourceRegistry  ── dispatch by kind
        ↓
ArchSourceLoader::load(config, ctx) → ArchLoadResult { facts, errors }
        ↓
runner::load_all  ── merge + dedup
        ↓
GovernanceOutput.architecture_facts  +  LOCUS006 findings
```

Documents do not decide. Documents declare. Policies decide.

## Non-goals

Carried verbatim from #108:

- No actual OpenAPI extraction (#109).
- No markdown fenced-block parsing (separate sub-issue).
- No policy decisions consuming the new facts (separate sub-issue).
- No recursive scan of the workspace without explicit configuration.

Additional v1 non-goals from clarification:

- No glob expansion. One source entry = one concrete file path. Globs land when a loader (likely markdown ADRs) actually demands them.
- No concrete loaders. `ArchSourceRegistry::standard()` ships empty; #109 lands the first real loader as a pure trait impl + registration.

## Configuration

`.locus/arch.json` gains a `sources` field alongside the existing `policies` and `concepts`:

```json
{
  "policies": ["registry-integrity", "registry-coherence", "arch-source-health", "..."],
  "concepts": [{ "id": "rule", "...": "..." }],
  "sources": [
    { "kind": "openapi",  "path": "docs/openapi.yaml" },
    { "kind": "markdown", "path": "docs/architecture/overview.md" }
  ]
}
```

`sources` is `#[serde(default)]`. A workspace with no `sources` (or no arch.json at all) sees current behavior unchanged.

`ArchSourceConfig::path` is workspace-relative. Resolution happens inside the loader via `ArchLoadContext::workspace_root`.

## Module layout

New code lives in `crates/locus-core/src/architecture/loader/`:

```text
crates/locus-core/src/architecture/
├── boundary.rs         (existing)
├── concept.rs          (existing)
├── contract.rs         (existing)
├── converter.rs        (existing)
├── debt.rs             (existing)
├── facts.rs            (existing)
├── module_ownership.rs (existing)
├── source.rs           (existing)
├── loader/
│   ├── mod.rs          re-exports
│   ├── config.rs       ArchSourceConfig
│   ├── context.rs      ArchLoadContext
│   ├── result.rs       ArchLoadResult, ArchLoadError
│   ├── trait_.rs       ArchSourceLoader trait
│   ├── registry.rs     ArchSourceRegistry
│   └── runner.rs       load_all (orchestrator)
└── mod.rs              pub mod loader; pub use loader::{...}
```

Each file stays under MO001's 5-public-types-per-module budget. The orchestrator is intentionally separated from the registry so the registry can be mocked in tests without dragging the runner along.

Two existing files are touched:

- `crates/locus-core/src/governance/arch.rs` — `ArchDeclaration` gains `pub sources: Vec<ArchSourceConfig>`.
- `crates/locus-core/src/governance/pipeline.rs` — `GovernanceOutput` gains `pub architecture_facts: ArchitectureFacts`; `run` invokes the runner; collected errors flow into the policy chain.

One new policy module:

- `crates/locus-core/src/governance/policies/arch_source_health.rs` — `ArchSourceHealthPolicy`, owner of LOCUS006.

## Public types

```rust
// architecture/loader/config.rs

#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct ArchSourceConfig {
    pub kind: String,
    pub path: String,
}

// architecture/loader/context.rs

pub struct ArchLoadContext<'a> {
    pub workspace_root: &'a Path,
}

// architecture/loader/result.rs

#[derive(Debug, Default)]
pub struct ArchLoadResult {
    pub facts: ArchitectureFacts,
    pub errors: Vec<ArchLoadError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchLoadError {
    UnknownKind { kind: String, path: String },
    FileMissing { path: String },
    MalformedSource { path: String, detail: String },
    FactConflict { fact_kind: &'static str, id: String, sources: Vec<SourceRef> },
}

// architecture/loader/trait_.rs

pub trait ArchSourceLoader: Send + Sync {
    fn kind(&self) -> &'static str;
    fn load(&self, config: &ArchSourceConfig, ctx: &ArchLoadContext) -> ArchLoadResult;
}

// architecture/loader/registry.rs

pub struct ArchSourceRegistry {
    loaders: Vec<Box<dyn ArchSourceLoader>>,
}

impl ArchSourceRegistry {
    pub fn standard() -> Self { Self { loaders: Vec::new() } }
    pub fn register(&mut self, loader: Box<dyn ArchSourceLoader>);
    pub fn find(&self, kind: &str) -> Option<&dyn ArchSourceLoader>;
}

// architecture/loader/runner.rs

pub fn load_all(
    sources: &[ArchSourceConfig],
    registry: &ArchSourceRegistry,
    ctx: &ArchLoadContext,
) -> ArchLoadResult;
```

Only `ArchitectureFacts` remains re-exported at the crate root. `ArchSourceConfig`, `ArchSourceLoader`, `ArchSourceRegistry`, `ArchLoadContext`, `ArchLoadResult`, and `ArchLoadError` are exported through `locus_core::architecture::loader::*`.

## Data flow

`governance::pipeline::run` and `run_with_workspace_root` already load `.locus/arch.json` via `Arch::load`. The new flow inserts the registry walk between arch parsing and policy execution:

```text
run(workspace_root)
  ├─ Arch::load(workspace_root)
  │     → ArchLoadOutcome::Present(decl)
  │
  ├─ registry = ArchSourceRegistry::standard()
  ├─ ctx = ArchLoadContext { workspace_root }
  │
  ├─ result = runner::load_all(&decl.sources, &registry, &ctx)
  ├─ result.facts.sort()
  │
  ├─ findings.extend(result.errors.into_iter().map(error_to_finding))
  ├─ ... existing policy chain (now including ArchSourceHealthPolicy) ...
  │
  └─ GovernanceOutput {
       diagnostics,                          // existing — now includes LOCUS006 entries
       architecture_facts: result.facts,     // NEW
       ...                                   // existing fields
     }
```

`ArchLoadOutcome::Missing` and `ArchLoadOutcome::Invalid` short-circuit before runner execution — `sources` is unreachable without a parsed arch.json. Coherence-side complaints about the missing/invalid file remain LOCUS004 territory.

`run_with_arch` (the test-only injection seam) keeps working: callers passing a custom `ArchLoadOutcome::Present(decl)` get the same sources walk.

## Duplicate / conflict semantics

Per-type identity used for collision detection:

| Fact | Identity |
|------|----------|
| `ConceptFact` | `id` |
| `BoundaryFact` | `id` |
| `ContractFact` | `(source_kind, operation)` |
| `ConverterFact` | `(from_concept, to_concept)` |
| `ModuleOwnershipFact` | `module` |
| `DebtFact` | `(target, reason)` |

`runner::load_all` invokes every loader in source-config order, then walks each fact vector:

- **Bit-exact duplicate** — `PartialEq` matches across the identity and the full body. Drop the later occurrence silently. Legitimate when two sources independently re-emit the same concept.
- **Identity collision with differing content** — keep the first by source-config order, emit `ArchLoadError::FactConflict` once per collision with both `SourceRef`s attached.

Source-config order is the only ordering signal used. No FS-walk-order leakage; no insertion-order dependency.

## Error materialization — LOCUS006

A new policy `ArchSourceHealthPolicy` owns LOCUS006 and decides every `RuleFinding` emitted by the runner. Following the LOCUS003/004/005 precedent:

- One LOCUS code per policy.
- Default severity `Advisory` (config-quality signal, not an architectural violation).
- `--agent-strict` escalates Advisory → Warning via existing severity logic. Not Fatal.

Message templates per error kind:

| Error | Message |
|------|---------|
| `UnknownKind { kind, path }` | `arch.json source kind '<kind>' has no registered loader (path: <path>)` |
| `FileMissing { path }` | `arch.json source path '<path>' not found` |
| `MalformedSource { path, detail }` | `arch.json source '<path>' failed to parse: <detail>` |
| `FactConflict { fact_kind, id, sources }` | `<fact_kind> '<id>' declared by multiple sources with conflicting content: <source1>, <source2>` |

Span: file-level on `.locus/arch.json` for `UnknownKind`, `FileMissing`, and `MalformedSource`. Line-level is best-effort and not promised by the spec — serde does not consistently surface line/column for `Vec` element errors. `FactConflict` points at the first conflicting source's `SourceRef.path` when present, otherwise falls back to `.locus/arch.json`.

`GovernanceDiagnosticRegistry::standard()` registers `("LOCUS006", PolicyId::new("arch-source-health"))`.

`PolicyRegistry::standard()` registers `ArchSourceHealthPolicy`.

`.locus/arch.json` for the Locus repo itself gains `"arch-source-health"` in its `policies` list.

## Acceptance criteria mapping

Issue #108's six ACs map onto this design as follows:

| AC | Where satisfied |
|----|-----------------|
| Loader registry exists and can be invoked with zero or more source configs. | `ArchSourceRegistry` + `runner::load_all`. |
| Loading is deterministic across platforms. | Source-config order is the only ordering signal; `ArchitectureFacts::sort()` runs after merge. |
| Missing or malformed sources are represented as structured errors/findings. | `ArchLoadError` variants → LOCUS006 via `ArchSourceHealthPolicy`. |
| Multiple loaders can contribute to a single merged `ArchitectureFacts` value. | `runner::load_all` walks every configured source and folds via `ArchitectureFacts::extend`. |
| Duplicate handling is explicit and tested. | Bit-exact dedup is silent; identity-collision emits `FactConflict`. Both paths covered by the test matrix below. |
| No configured architecture sources preserves current behavior. | Empty `sources` short-circuits the runner; `GovernanceOutput.architecture_facts` is `ArchitectureFacts::default()`. |

## Testing

Per-module unit tests live next to each new file (the sibling-file `architecture_tests.rs` pattern stays for the existing fact types).

Mock loaders for the runner's tests:

```rust
#[cfg(test)]
struct MockLoader { kind: &'static str, facts: ArchitectureFacts }
#[cfg(test)]
struct MockFailingLoader { kind: &'static str, error: ArchLoadError }
```

Test matrix in `loader/runner.rs`:

1. Empty sources + empty registry → `ArchLoadResult::default()`. Zero findings.
2. Single source, registered loader emits facts → facts merged, zero errors.
3. Single source, **unregistered kind** → `UnknownKind` error.
4. Single source, **missing file** (loader-reported) → `FileMissing` error.
5. Two sources, same loader, **bit-exact duplicate fact** → dedup silent.
6. Two sources, same identity, **different content** → `FactConflict`.
7. **Determinism** — same set of configs declared in different order produces identical `ArchitectureFacts` after `sort()` and identical conflict orderings (when conflict source order is canonicalised by SourceRef sort).
8. **Integration** — `governance::pipeline::run_with_workspace_root` over a temp workspace whose `.locus/arch.json` contains real `sources` entries: `GovernanceOutput.architecture_facts` populated; LOCUS006 finding shape verified.

Policy tests in `policies/arch_source_health.rs` mirror the existing `policies/registry_coherence.rs` shape: each `ArchLoadError` variant gets a `decide` test with assertions on diagnostic code, severity, and message.

## Out of scope (explicitly deferred)

- Concrete loaders. The OpenAPI loader is #109; markdown / ADR loaders are separate sub-issues.
- Glob expansion in `ArchSourceConfig.path`.
- Wiring `architecture_facts` into `RuleContext` or `PolicyContext` (separate sub-issue under #106).
- Stale-source detection (separate sub-issue under #106).
- Promoting `ArchDeclaration.concepts` into `ConceptFact`s. The two shapes look similar but carry different semantics (`enforcement` vs. `source_of_truth: Option<String>`) and that bridge belongs in the policy-wiring slice.

## Open follow-ups

- Document `arch-source-health` in `docs/PARADIGMS.md` / `docs/superpowers/specs/2026-05-11-governance-spine-design.md` alongside the existing LOCUS00X catalogue.
- Add an `arch.json` example with `sources` to the dogfood audit when the first real loader (#109) lands.
