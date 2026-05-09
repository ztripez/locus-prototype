# CLAUDE.md / AGENTS.md

This file is the per-repo dev-handoff for Claude Code (and other agents — `CLAUDE.md` symlinks to this file). It describes *current state* and how to keep working on it. For the underlying architectural rules, see `docs/`.

## Reading order

1. **[`README.md`](README.md)** — what Locus is and isn't, in two screens.
2. **[`docs/AGENT_GUARDRAILS.md`](docs/AGENT_GUARDRAILS.md)** — non-negotiables for agents working on Locus itself (determinism, no LLM in `check`, no broad ignores, etc.). Read before adding anything to the rule engine.
3. **[`docs/PARADIGMS.md`](docs/PARADIGMS.md)** — full umbrella spec; every paradigm Locus is meant to guard. Use as the source of truth for paradigm semantics, source-fact taxonomy, and the architectural-authority framing. Paradigm 1 carries summary rule entries (OT001–OT012), source-hint forms, and severity tiers.
4. **[`docs/project-jumpoff.md`](docs/project-jumpoff.md)** — the original OT-paradigm deep dive. Read for full spec content (CLI command surface, lockfile examples, generator design, exception format). Pre-dates the multi-paradigm reframing, so treat its top-level "Locus is …" framing as historical; the rule definitions and AIR examples remain authoritative.
5. **[`docs/superpowers/specs/2026-05-09-dogfood-false-positive-ledger.md`](docs/superpowers/specs/2026-05-09-dogfood-false-positive-ledger.md)** — active triage ledger for Epic #1. Use this to track per-rule TP/FP/onboarding/debt classification and strictness-graduation evidence.

## Project status

19 paradigms registered; **84 rules implemented** (up from 41 — see `docs/PARADIGMS.md` "Implementation status (snapshot)" for the per-paradigm rule list). Two loaders ship: **`std-rt`** produces 6 language-level fact kinds (`SpawnedWork`, `ConfigRead`, `Logging`, `BlockingCall`, `PersistenceWrite`, `ExternalIo` from stdlib patterns); **`markers`** promotes `// ot: marks <fact_kind>` source hints into facts for the 5 kinds the loader tier can't auto-recognise (`HotPath`, `RequestContext`, `BoundaryEntry`, `RuntimeStateOwner`, `BackgroundWorker`). Highlight set:

- **OT** (Canonical Domain Ownership) — OT001–OT012 implemented. End-to-end wiring: AIR emission, paradigm host, lockfile, `locus init / accept canonical|boundary / check` CLI.
- **DG** (Dependency Graph / Direction) — DG001 (forbidden import), DG002 (dependency cycle via Tarjan SCC), DG003 (cross-feature internals reach), DG004 (shared module reaching feature). Lockfile carries `forbidden_edges`, `features` (with `public_api` patterns), and `shared_paths`. CLI mutators: `locus dg forbid-edge`, `locus dg define-feature`, `locus dg add-shared-path`.
- **FL** (Failure Lineage) — 9 rules: FL001 (boundary error in domain signature), FL002 (panic-shaped callees), FL003 (`.ok()`/`.err()` silent discard), FL004 (`let _ = call(...);`), FL005 (partial `if let Ok/Err`), FL006 (`map_err(|_|)` losing source context), FL007 (catch-all `Err(_)` arm with silent body), FL011 (bare `_` arm as failure sink), FL013 (lossy stringification in `Result<_, String>`). All share `invariant_owner_paths`; matcher accepts segment-anywhere `*::tests::*` patterns so inline `mod tests {}` blocks are correctly carved out.
- **CX** (Complexity Budget) — 4 rules: CX001 per-function lines (default 50), CX002 per-module lines (default 400), CX007 per-file public-API count (default 30), CX008 per-function fan-out under accepted orchestration paths.
- **Every paradigm now ships at least 2 rules.** Largest expansions: MO (4 rules), UT (5), FL (6), CX (4), TA (4), RM (4), ER (4), DA (3), BO (3), PA (3), OB (3), DC (3), RW (3), AB (2), CR (2), FO (2), CF (1+stub).
- **CF002 is a stub** — lockfile fields ship today; rule body deferred until a filesystem-aware loader lands (consumes `AirWorkspace`, not the filesystem directly).

**Cross-paradigm infrastructure shipped:**
- `Severity::from_confidence(c, mode)` — spec's 0.50/0.70/0.90 tier helper, used by inference-shaped rules.
- `locus_core::exceptions` — `// ot: allow XX###` source hints + `Lockfile.exceptions[]` lockfile entries. Expired exceptions emit `LOCUS001` warnings instead of silently re-firing.
- **`LOCUS002` (vacancy nudge)** — emitted once per vacant-by-definition paradigm whose declaration lists are empty. The user either populates the paradigm's section or adds the prefix to `Lockfile.acknowledged_empty` to silence it. Fires for BO/PA/CR/RW/DA/UT/ER/FL/DG/CF/RM/TA/FO when un-onboarded.
- **Default posture: noisy until configured, narrow with the lockfile.** Numeric/structural rules (CX001/CX002/CX007/MO001/MO002, plus structural rules like ER001/ER007/DC002/DC004/AB001/AB002) fire on un-onboarded code with built-in defaults. Vacant-by-definition paradigms emit `LOCUS002` instead of silence. Configuration narrows via `paradigms.<prefix>.*` overrides, `acknowledged_empty`, or `// ot: allow XX###` exception hints.

Locus's own source is annotated. `locus check --workspace .` against the
unconfigured repo (no `locus.lock` at the root) intentionally emits a
mix of warnings (CX/MO/DC/ER) and `LOCUS002` advisories — those are the
"noisy default" working as designed. Self-application clean-status now
means *zero unexpected fatals*, not zero warnings.

Workspace layout:

```
crates/
  locus-air/       # paradigm-neutral data + serde, schema v8 (FactKind aligned with spec)
  locus-core/      # paradigm host + 19 paradigm modules, shared diagnostics + lockfile + exceptions
  locus-rust/      # cargo_metadata + walkdir + syn + ot: hints + import scanning + clean type renderer
  locus-cli/       # binary `locus`: emit-air | init | accept canonical|boundary | check + per-paradigm mutators
  locus-report/    # STUB; populated when SARIF/JSON formatters are needed
tests/fixtures/sample-crate/   # standalone fixture; NOT a workspace member
```

## Naming

- **Locus** is the tool (Cargo crate, CLI binary `locus`, lockfile `locus.lock`).
- **OT / "one truth"** is one paradigm. It survives in the rule prefix (`OT###`), the source-hint syntax (`// ot: canonical`, `// ot: boundary`, …), and the module `crates/locus-core/src/paradigms/one_truth/`.
- Future paradigms get their own prefixes (`DG###`, `CF###`, …) and their own modules under `paradigms/`.

## Architecture

Two-layer separation, strictly enforced:

1. **AIR is paradigm-neutral source facts.** Language adapters (`locus-rust`, future `locus-ts`, …) emit AIR — they record what *is* in source, not what it *means*. AIR symbols are package-prefixed (`pkg_name::module::Type`) so cross-crate types in a workspace don't collide.
2. **Paradigm modules consume AIR.** Each paradigm under `crates/locus-core/src/paradigms/` interprets AIR through its own lens. Paradigms share `locus-core`'s diagnostic + lockfile infrastructure but never import each other.

If you reach for `syn`/`cargo_metadata` from `locus-core`, stop — that belongs in the language adapter. If you add paradigm-specific reasoning to `locus-rust`, stop — that belongs in a paradigm module.

## Self-application (dogfooding)

Locus must be able to scan its own source. Annotate types at creation time, not retroactively:

- `// ot: canonical` on `locus-air` types (`AirWorkspace`, `AirType`, `AirField`, …) — the canonical representation of "source facts in a workspace."
- `// ot: boundary <concept> <boundary>` on `clap`-derive arg structs in `locus-cli` (CLI input shape) and on lockfile-on-disk types in `locus-core` (file format).
- `// ot: converter` on `From`/`TryFrom` impls or free functions moving data between layers.

If `locus check --workspace .` ever stops being clean, *that* is the regression to investigate first.

## Test corpus

`crates/locus-rust/tests/emit_air_corpus.rs` is gated on **`LOCUS_TEST_CORPUS`**. Unset → skips silently. Recorded path:

```
LOCUS_TEST_CORPUS=/mnt/code/projects/sides/lors
```

(17-crate Bevy/anatom/governance workspace, ~190 `.rs` files. Locus scans it in ~1.2s, emits 621 type / 1822 fn / 19 conversion AIR items, all symbols globally unique.)

Explicit run:

```bash
LOCUS_TEST_CORPUS=/mnt/code/projects/sides/lors \
  cargo test -p locus-rust --test emit_air_corpus -- --nocapture
```

## Non-negotiables (apply to every paradigm)

These are the in-repo restatement of `docs/AGENT_GUARDRAILS.md` — read that doc for the full reasoning.

- **No proc macros as the authoring surface.** Source hints are compact `// ot:` (or future `// dg:`, `// cf:`) comments only.
- **No required runtime/compile-time dependency** in projects being checked.
- **No hand-authored semantic config.** Accepted ownership lives in a generated `locus.lock`. A small structural YAML (paths, generated globs) is allowed; a giant rule DSL is not.
- **Blocking rules must be deterministic.** No LLM-in-the-loop for fail/pass decisions in `locus check`. Optional advisory modes may exist later.
- **Inference-first UX.** Verbose annotations are a UX failure. The tool infers role; users accept ambiguous cases via CLI (`locus accept …`).
- **Make the canonical path shorter than the shadow path** — generators are part of the product, not a nice-to-have.
- **Source facts vs. accepted ownership.** Adapters emit facts; paradigms apply rules; lockfile is the acceptance record. Never let one bleed into another.
- **Noisy by default; configuration narrows.** Numeric and structural rules fire on un-onboarded code using built-in defaults. Vacant-by-definition paradigms (BO/PA/CR/RW/DA/UT/ER/FL/DG/CF/RM/TA/FO) emit `LOCUS002` until the user populates the relevant declaration list or adds the prefix to `Lockfile.acknowledged_empty`. Silence is a user act, not the default — when adding a rule, it should fire on its earliest meaningful evidence, with configuration shrinking the surface (overrides, exempt paths, `// ot: allow`).
- **Every rule implementation needs an independent doc-conformance sign-off.** Before merging any new `XX###` rule, dispatch a separate agent to read the rule's spec entry in `docs/PARADIGMS.md` (and `docs/project-jumpoff.md` for OT) alongside the implementation, and confirm: detection logic matches the spec; default severity matches the severity-tier table; agent-strict elevation matches the spec; lockfile schema additions are namespaced under `paradigms.<prefix>` and documented; the rule fires by default on its earliest meaningful evidence (or, for vacant-by-definition paradigms, the paradigm emits `LOCUS002` instead of silence). The reviewer is independent of the implementer — pass file paths and line numbers, not your own reasoning. If the reviewer flags drift, fix it before marking the rule done.

## AIR shape gotcha

`AirItem` is an externally tagged enum (`#[serde(tag = "kind")]`), so the discriminant occupies the JSON key `kind`. `AirType.kind` and `AirUsage.kind` are therefore serde-renamed to `type_kind` / `usage_kind` in JSON to avoid duplicate keys. The Rust field names stay `kind`. If you add another `AirItem` variant whose payload struct has a `kind` field, do the same rename.

## CLI workflow (current)

```bash
# Inspect the workspace as paradigm-neutral facts.
locus emit-air --workspace . --pretty

# Capture annotated canonicals + boundaries from a fresh scan.
locus init --workspace .

# Onboard a codebase that has no `// ot:` annotations yet.
locus accept canonical pkg::module::Type [--concept <id>]
locus accept boundary  pkg::module::Dto  --concept <id> [--boundary <name>]

# Declare DG forbidden edges (architectural direction).
locus dg forbid-edge --from "pkg::domain::*" --to "pkg::api::*" [--reason "..."]

# Run all enabled paradigms; exit non-zero on Fatal.
locus check --workspace .                # human mode (warnings)
locus check --workspace . --agent-strict # warnings → fatal
locus check --workspace . --changed      # filter to PR-modified files
locus check --workspace . --changed --baseline origin/develop  # custom baseline
locus check --workspace . --changed --agent-strict  # CI shape: fail only on new violations


# Canonical changed-only strict gate (Epic #1 workflow)
LOCUS_BASELINE=origin/main scripts/check-changed-strict.sh .
```

**`--changed` semantics:** combines three git queries — `git diff baseline HEAD` (committed
changes), `git diff HEAD` (working-tree changes), `git ls-files --others --exclude-standard`
(untracked but unignored). Default baseline tries `origin/main` → `origin/master` → `main` →
`master` → `HEAD~1`. The filter is applied after exception suppression so `// ot: allow`
hints on changed code still suppress, and `LOCUS001` expired-exception warnings still surface.

## Implementation roadmap

- ✅ AIR emission v13 (language-agnostic naming pass on top of v12 — `EnumMatch` → `DiscriminatedMatch`, `Visibility::Crate` → `Module`, `CallKind::Macro`/`DiscardKind::Macro` → `Meta`, `ArmBodyShape::Propagate` → `ErrorPropagation`, `AirItem::PartialIfLet` → `PartialResultMatch` with typed enum variant, `ConversionMechanism::From/TryFrom/InherentMethod/FreeFn` → `InfallibleAdapter/FallibleAdapter/InstanceMethod/FreeFunction` plus new `FactoryFunction`, `AirImpl` → `AirImplBlock` with `interface`/`target_type`/`dispatch`, unified `decorators` field replacing `derives`+`attrs`, added `path_segments` and `symbol_segments` for delimiter-portable matching, added `FallbackPattern` enum on `AirFallbackCall`. The Rust adapter remains the only adapter today; the schema is now ready for TS / Python / Go / Swift adapters to plug in without parallel-AIR shapes for shared concepts).
- ✅ OT paradigm: OT001–OT012 all shipped; `init` / `accept canonical|boundary` / `check` end-to-end with `--agent-strict` elevation.
- ✅ DG paradigm: DG001–DG004 (forbidden imports, cycles via Tarjan, cross-feature internals reach, shared-module reaching feature).
- ✅ FL paradigm: FL001 (boundary error in domain), FL002 (panic-shaped callees), FL003 (silent-discard `.ok()`/`.err()`), FL004 (`let _ = call(...);` discards), FL005 (partial `if let Ok/Err = ...` without `else`).
- ✅ Cross-paradigm: `Severity::from_confidence` tier helper; `// ot: allow` + lockfile exceptions wired through the CLI's `check` pipeline.
- ✅ Second rules for DC, ER, UT, RM (DC002 doc-residue phrases, ER002 string-shaped errors, UT002 forbidden imports, RM002 converter side-effects).
- ✅ **Silent-error coverage gap nearly closed** — FL004 (`let _ =`), FL005 (partial `if let`), FL006 (`map_err(|_|)`), FL007 (catch-all `Err(_)` arm body), FL011 (bare `_` failure sink), ER005 (catch-all error mapping). AIR v10 adds `MatchArm` + `ClosureMethodCall` items. Still missing: `result.unwrap_or(literal)` (needs literal-shape capture on the default arg), spawned-task failures with no sink (needs `RuntimeStateOwner`/`BackgroundWorker` loader output), retry-loop shapes.
- ✅ **stdlib-fact rules online** — std-rt loader emits `BlockingCall`, `PersistenceWrite`, `ExternalIo` from stdlib call shapes (no framework dep needed; the architectural concepts are paradigm-neutral per spec). 5 new rules consume them: BO005 (persistence in domain), PA003 (external IO in app without port), RW002 (blocking call outside runtime owner), RM005 (validator IO), RM006 (domain method writing to storage).
- ✅ **User-marker mechanism online** — `// ot: marks <fact_kind>` source hints (parsed by hint scanner, promoted to `AirFact` by the new `markers` loader) cover the 5 reserved fact kinds the loader tier can't auto-recognise (`HotPath`, `RequestContext`, `BoundaryEntry`, `RuntimeStateOwner`, `BackgroundWorker`) plus user overrides for the std-rt-recognised kinds. 3 new rules consume markers: RW005 (blocking inside hot_path), RW006 (spawn inside hot_path), OB004 (boundary_entry without logging). Framework-specific loaders (Bevy `Update` → `HotPath`, axum `#[handler]` → `RequestContext`, etc.) plug in alongside markers as they land.
- ✅ Second rules for every paradigm (16 paradigms beyond OT/DG now each ship 2–6 rules; see `docs/PARADIGMS.md` snapshot).
- ✅ **Tractable visitor-work gap closed** — `unwrap_or` shape, retry-loop detection, scrutinee literal capture all shipped in AIR v12. The 4 rules they unlock (FL010, FL012, CF002, CF003) all online.
- ✅ **CX002 module-line budget** — caps total file line count (default 400). Per-module overrides via `paradigms.CX.module_overrides`. Brings the CX rule count to 4 (CX001/CX002/CX007/CX008).
- ✅ **Convention flip: noisy default + LOCUS002 vacancy nudge.** CX001/CX002/MO001/MO002 now fire on un-onboarded code with built-in fallback budgets (no longer silent on default sections). Vacant-by-definition paradigms (BO/PA/CR/RW/DA/UT/ER/FL/DG/CF/RM/TA/FO) emit `LOCUS002` Advisory diagnostics until the user populates declarations or adds the prefix to `Lockfile.acknowledged_empty`. Self-check on the unconfigured Locus repo emits ~132 warnings (CX/MO/DC/ER) + 13 LOCUS002 advisories.
- 🔜 Framework-specific loaders (Bevy `Update` → HotPath, axum `#[handler]` → RequestContext, sqlx → PersistenceWrite, reqwest → ExternalIo, …) remain the next infrastructure step. The architectural concepts and consuming rules are all in place; framework loaders are pure recognition-surface expansion.
- ✅ **`locus init` multi-run scan-and-report** — `init` now emits a checklist of `locus <verb> ...` commands per detected layer / concept cluster / feature / threshold; categories are `[concept] [layer] [feature] [threshold] [switch] [paradigm-vacant]`. Five vacancy seeds (AB/DA/DC/OB/RW) surface for un-onboarded paradigms; `--acknowledge-empty <PREFIXES>` silences them. New mutators: `accept converter`, `rw accept-runtime-owner`, `er add-domain-path`, `rm add-domain-path`, `pa add-application-path`. Heuristics live per-paradigm in `paradigms/<p>/init.rs` plus cross-paradigm helpers in `locus-core::init`. Spec: `docs/superpowers/specs/2026-05-08-locus-init-multi-run-design.md`; plan: `docs/superpowers/plans/2026-05-08-locus-init-multi-run.md`.
- ✅ `locus debt` — inventory of every suppression (`// ot: allow` hints + `Lockfile.exceptions`), classified Active / Expired / Unbounded, sorted with expired first. Text and `--json` modes. Backed by `locus_core::exceptions::collect_exceptions`.
- 🔜 Remaining CLI oracle commands: `locus explain`, `locus query <kind>`, `locus graph`, `locus prune`, `locus add adapter`. Spec: `docs/PARADIGMS.md` §"Locus as an Architectural Oracle".
- ✅ `--changed` / `--baseline` for diff-aware checking. Combines `git diff baseline HEAD` + working-tree diff + untracked-but-not-ignored. `--patch <file>` mode (single patch file) is a stretch goal.
- 🔜 SARIF / JSON formatters in `locus-report` (currently a stub; CLI hand-rolls output).
- Then: deterministic loaders (`docs/PARADIGMS.md` covers the loader system) for framework-specific normalized facts. Loader output enriches AIR with `hot_path`, `request_context`, `blocking_call`, etc. that future paradigms (AC, TX, SE) consume.

## Common commands

```bash
cargo build --workspace
cargo test --workspace
cargo test -p locus-rust hints::tests        # single test by path
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo run -p locus-cli -- emit-air --workspace tests/fixtures/sample-crate --pretty
cargo run -p locus-cli -- check    --workspace tests/fixtures/sample-crate
```

No CI is configured.
