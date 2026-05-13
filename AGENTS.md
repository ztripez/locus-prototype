# CLAUDE.md / AGENTS.md

This file is the per-repo dev-handoff for Claude Code (and other agents — `CLAUDE.md` symlinks to this file). It describes *current state* and how to keep working on it. For the underlying architectural rules, see `docs/`.

## Reading order

1. **[`README.md`](README.md)** — what Locus is and isn't, in two screens.
2. **[`docs/AGENT_GUARDRAILS.md`](docs/AGENT_GUARDRAILS.md)** — non-negotiables for agents working on Locus itself (determinism, no LLM in `check`, no broad ignores, etc.). Read before adding anything to the rule engine.
3. **[`docs/superpowers/specs/2026-05-11-governance-spine-design.md`](docs/superpowers/specs/2026-05-11-governance-spine-design.md)** — active architecture-governance transition spec for epic #71. Read this before touching rule execution, diagnostics, registries, policies, or the legacy `Paradigm::check` path.
4. **[`docs/PARADIGMS.md`](docs/PARADIGMS.md)** — full umbrella spec; every paradigm Locus is meant to guard. Use as the source of truth for paradigm semantics, source-fact taxonomy, and the architectural-authority framing. Paradigm 1 carries summary rule entries (OT001–OT012), source-hint forms, and severity tiers.
5. **[`docs/project-jumpoff.md`](docs/project-jumpoff.md)** — the original OT-paradigm deep dive. Read for full spec content (CLI command surface, lockfile examples, generator design, exception format). Pre-dates the multi-paradigm reframing, so treat its top-level "Locus is …" framing as historical; the rule definitions and AIR examples remain authoritative.
6. **[`docs/superpowers/specs/2026-05-09-dogfood-false-positive-ledger.md`](docs/superpowers/specs/2026-05-09-dogfood-false-positive-ledger.md)** — active triage ledger for Epic #1. Use this to track per-rule TP/FP/onboarding/debt classification and strictness-graduation evidence.
7. **[`docs/RUST_ADAPTER.md`](docs/RUST_ADAPTER.md)** — semantic-boundary contract for `crates/locus-rust`. Read before writing any new rule: the four-layer breakdown (raw scan / syn AST / rendered text / heuristic inference) and the rule-author checklist tell you whether the adapter can actually deliver the facts your rule depends on.

## Active roadmap: architecture governance spine

Epic **#71** is the active roadmap. Locus is transitioning from a rule-runner linter into a deterministic architecture-governance engine:

```text
rules / sensors -> findings / evidence -> policy decisions -> diagnostics
```

A rule finding is not the final diagnostic. Rules are sensors/evidence producers. Policies are first-class governance logic that decides final severity, status, rationale, and output in architectural context. Policies are not simple rule bundles.

Registries for rules, paradigms, policies, and governance diagnostic codes are the preferred modularity direction. New rules must implement `RuleDefinition` and register through the governance spine. The legacy `Paradigm::check() -> Vec<Diagnostic>` path is transitional compatibility only; do not add new rules there unless explicitly instructed.

Prioritize #71–#76 work unless the maintainer explicitly redirects. Public release polish, broad new paradigm packs, framework loaders, SARIF/JSON reporters, and release documentation are iced until the governance spine stabilizes.

Dogfood discipline remains strict: do not weaken rules, raise budgets, add broad exemptions, or hide findings to make `locus check --workspace .` pass. If a finding is real debt, keep it visible or add a narrow, justified, time-bounded exception with ownership metadata. Do not implement future AC/TX/SE-style paradigm packs before the decision/policy pipeline exists.

### Dogfood drift is not "later"

A PR that increases Locus's own `locus check --workspace .` count is a failed review unless the maintainer explicitly accepts tracked debt.

Do **not** write "visible debt" in the PR body and move on. First refactor so the dogfood summary returns to the pre-PR baseline. If that is genuinely not reasonable in the same PR, split the work or create a narrow tracked-debt record with:

- exact rule id and path,
- why the debt exists,
- owner,
- expiry date,
- linked issue,
- allowed delta.

Broad allows, budget bumps, vague follow-ups, and "later me" cleanup are not accepted. Implementation growth that triggers CX/MO/etc. should normally be fixed by splitting or moving code, not by accepting a higher warning count. If a PR reduces dogfood findings, ratchet the baseline/documentation downward rather than letting later PRs reintroduce them.

### Architecture declaration (`#75`)

Locus dogfoods its own governance via `.locus/arch.json`:

```json
{
  "policies": [
    "registry-integrity",
    "registry-coherence",
    "default-pass-through"
  ]
}
```

- **`registry-integrity`** (LOCUS003) — emits when a rule code observed at runtime has no registered `RuleDefinition`. Strangler-completion signal.
- **`registry-coherence`** (LOCUS004) — emits when the declared policy list drifts from the registered policy set, when a registered rule references an unknown paradigm at runtime, when a registered paradigm references an unknown rule, or when `.locus/arch.json` is missing/malformed.

Both run **advisory-only**: they surface drift without blocking CI. The MVP framing is deliberate — future iterations can elevate severity once arch-drift behavior is well-understood. This is the first architecture-governance MVP per #75, not the final observation/mutation engine — public release polish, full architecture style catalogue, and git-history observation are non-goals for the MVP.

### Per-concept enforcement (`#99`)

Each concept in `.locus/arch.json` accepts an optional `enforcement` field:

```json
{
  "id": "rule",
  "source_of_truth": "RuleDefinition",
  "registry": "RuleRegistry",
  "enforcement": "advisory"
}
```

- **`advisory`** (default) — LOCUS005 bypass findings render at `Advisory` severity with `DecisionStatus::Advisory`. Visible but not a gate. Use during onboarding or when the concept's SoT contract is still evolving.
- **`enforced`** — LOCUS005 bypass findings render at `Warning` under normal `locus check`, elevated to `Fatal` under `--agent-strict`, with `DecisionStatus::Active`. Use when the concept's SoT contract is firm and bypasses must block CI.

The unknown-concept-id path (a typo in `arch.json`, e.g. `id: "ruel"`) stays pinned to Advisory regardless of any declared `enforcement` value — it's a config-quality signal, not a real SoT bypass.

Legacy diagnostics (`FindingSource::LegacyDiagnostic`) are never affected by `enforcement` — they remain under LOCUS003 `KnownTransitionDebt` regardless.

Locus's own four concepts (`rule`, `paradigm`, `policy`, `governance-code`) ship at `advisory` — the mechanism is in place, but no graduation is included with #99. Flipping individual concepts to `enforced` is a separate maintainer decision tracked against the dogfood audit.

## Project status

20 paradigms registered; **90 rules implemented** (up from 41 — see `docs/PARADIGMS.md` "Implementation status (snapshot)" for the per-paradigm rule list). Two loaders ship: **`std-rt`** produces 6 language-level fact kinds (`SpawnedWork`, `ConfigRead`, `Logging`, `BlockingCall`, `PersistenceWrite`, `ExternalIo` from stdlib patterns); **`markers`** promotes `// locus: fact <fact_kind>` source hints into facts for the 5 kinds the loader tier can't auto-recognise (`HotPath`, `RequestContext`, `BoundaryEntry`, `RuntimeStateOwner`, `BackgroundWorker`). Highlight set:

- **OT** (Canonical Domain Ownership) — OT001–OT012 implemented. End-to-end wiring: AIR emission, paradigm host, lockfile, `locus init / accept canonical|boundary / check` CLI.
- **DG** (Dependency Graph / Direction) — DG001 (forbidden import), DG002 (dependency cycle via Tarjan SCC), DG003 (cross-feature internals reach), DG004 (shared module reaching feature). Lockfile carries `forbidden_edges`, `features` (with `public_api` patterns), and `shared_paths`. CLI mutators: `locus dg forbid-edge`, `locus dg define-feature`, `locus dg add-shared-path`.
- **FL** (Failure Lineage) — 9 rules: FL001 (boundary error in domain signature), FL002 (panic-shaped callees), FL003 (`.ok()`/`.err()` silent discard), FL004 (`let _ = call(...);`), FL005 (partial `if let Ok/Err`), FL006 (`map_err(|_|)` losing source context), FL007 (catch-all `Err(_)` arm with silent body), FL011 (bare `_` arm as failure sink), FL013 (lossy stringification in `Result<_, String>`). All share `invariant_owner_paths`; matcher accepts segment-anywhere `*::tests::*` patterns so inline `mod tests {}` blocks are correctly carved out.
- **CX** (Complexity Budget) — 4 rules: CX001 per-function lines (default 50), CX002 per-module lines (default 400), CX007 per-file public-API count (default 30), CX008 per-function fan-out under accepted orchestration paths.
- **Every paradigm now ships at least 2 rules.** Largest expansions: MO (4 rules), UT (5), FL (6), CX (4), TA (4), RM (4), ER (4), DA (3), BO (3), PA (3), OB (3), DC (3), RW (3), AB (2), CR (2), FO (2), CF (1+stub).
- **CF002 is a stub** — lockfile fields ship today; rule body deferred until a filesystem-aware loader lands (consumes `AirWorkspace`, not the filesystem directly).

**Cross-paradigm infrastructure shipped:**
- `Severity::from_confidence(c, mode)` — spec's 0.50/0.70/0.90 tier helper, used by inference-shaped rules.
- `locus_core::exceptions` — `// locus: allow XX###` source hints + `Lockfile.exceptions[]` lockfile entries. Expired exceptions emit `LOCUS001` warnings instead of silently re-firing.
- **`LOCUS002` (vacancy nudge)** — emitted once per vacant-by-definition paradigm whose declaration lists are empty. The user either populates the paradigm's section or adds the prefix to `Lockfile.acknowledged_empty` to silence it. Fires for BO/PA/CR/RW/DA/UT/ER/FL/DG/CF/RM/TA/FO when un-onboarded.
- **Default posture: noisy until configured, narrow with the lockfile.** Numeric/structural rules (CX001/CX002/CX007/MO001/MO002, plus structural rules like ER001/ER007/DC002/DC004/AB001/AB002) fire on un-onboarded code with built-in defaults. Vacant-by-definition paradigms emit `LOCUS002` instead of silence. Configuration narrows via `paradigms.<prefix>.*` overrides, `acknowledged_empty`, or `// locus: allow XX###` exception hints.
- **Policy Guard (`PG000`/`PG001`/`PG002`/`PG003`/`PG004`/`PG006`)** — agents cannot clear `--agent-strict` by widening policy. Compares current `.locus/lock.json` to `git show <baseline>:.locus/lock.json`; fires Fatal under strict on missing baseline (PG000), default- or override-budget raises (PG001), new overrides (PG002, regardless of metadata), new `exempt_paths` (PG003), new `acknowledged_empty` (PG004), and new overrides missing debt metadata (PG006). PG runs **after** `apply_exceptions` so lockfile exceptions cannot silence it. `--allow-policy-calibration` downgrades PG001–PG004 to Advisory but **not** PG000 (audit gap) or PG006 (justification gap). `--allow-missing-policy-baseline` silences PG000 when the gap is intentional. Spec: `docs/superpowers/specs/2026-05-09-policy-guard-paradigm.md` (#44).

Self-application status is not "zero findings."

Current dogfood status means: zero unexpected fatals under the current
lockfile and severity policy. Known remaining surfaces include CX001/CX002
warning debt, accepted lockfile exceptions, acknowledged-empty paradigms,
declared public API / converter authority, and policy suppressions
tracked in the dogfood audit.

Snapshot numbers live in
[`docs/superpowers/specs/2026-05-09-dogfood-audit.md`](docs/superpowers/specs/2026-05-09-dogfood-audit.md).
Update that audit when changing policy or dogfood claims.

Snapshot as of 2026-05-11: 0 active fatals, 103 warning debt
(64 CX001 + 39 CX002 advisory; down from 143 after umbrella #51's
refactor-first sweep), 16 accepted debt entries (14 lockfile
exceptions + 2 MO overrides with full metadata), 13 policy
suppressions without debt metadata (12 acknowledged_empty paradigms +
1 CX exempt_paths-covered CX007), 133 severity-tier demotions
(unchanged historical fact from PR #36), -30 net delta vs pre_36
baseline (refactor sweep reversed earlier source-drift accumulation).

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

- **Locus** is the tool (Cargo crate, CLI binary `locus`, lockfile `.locus/lock.json`).
- **OT / "one truth"** is one paradigm. It survives in the rule prefix (`OT###`), the source-hint syntax (`// locus: ot canonical`, `// locus: ot boundary`, …), and the module `crates/locus-core/src/paradigms/one_truth/`.
- Future paradigms get their own prefixes (`DG###`, `CF###`, …) and their own modules under `paradigms/`.

## Architecture

Two-layer separation, strictly enforced:

1. **AIR is paradigm-neutral source facts.** Language adapters (`locus-rust`, future `locus-ts`, …) emit AIR — they record what *is* in source, not what it *means*. AIR symbols are package-prefixed (`pkg_name::module::Type`) so cross-crate types in a workspace don't collide.
2. **Paradigm modules consume AIR.** Each paradigm under `crates/locus-core/src/paradigms/` interprets AIR through its own lens. Paradigms share `locus-core`'s diagnostic + lockfile infrastructure but never import each other.

If you reach for `syn`/`cargo_metadata` from `locus-core`, stop — that belongs in the language adapter. If you add paradigm-specific reasoning to `locus-rust`, stop — that belongs in a paradigm module.

**Governance spine (epic #71) — strangler in progress.** `crates/locus-core/src/governance/` hosts the new `rules → findings → policies → decisions → diagnostics` pipeline. `Paradigm::check` is **transitional**: new rules must implement `RuleDefinition` and register in `RuleRegistry::standard()` and the corresponding `ParadigmDefinition::rules()` slice. The legacy adapter wraps any diagnostic whose `rule_id` is not in the rule registry, so output stays byte-identical until a policy explicitly says otherwise. Spec: `docs/superpowers/specs/2026-05-11-governance-spine-design.md`; P1 plan: `docs/superpowers/plans/2026-05-11-governance-spine-p1.md`.

## Self-application (dogfooding)

Locus must be able to scan its own source. Annotate types at creation time, not retroactively:

- `// locus: ot canonical` on `locus-air` types (`AirWorkspace`, `AirType`, `AirField`, …) — the canonical representation of "source facts in a workspace."
- `// locus: ot boundary <concept> <boundary>` on `clap`-derive arg structs in `locus-cli` (CLI input shape) and on lockfile-on-disk types in `locus-core` (file format).
- `// locus: ot converter` on `From`/`TryFrom` impls or free functions moving data between layers.

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

- **No proc macros as the authoring surface.** Source hints are compact `// locus:` comments only (paradigm-scoped subforms like `// locus: ot canonical`, generic forms like `// locus: allow XX###` and `// locus: fact <fact_kind>`).
- **No required runtime/compile-time dependency** in projects being checked.
- **No hand-authored semantic config.** Accepted ownership lives in a generated `.locus/lock.json`. A small structural YAML (paths, generated globs) is allowed; a giant rule DSL is not.
- **Blocking rules must be deterministic.** No LLM-in-the-loop for fail/pass decisions in `locus check`. Optional advisory modes may exist later.
- **Inference-first UX.** Verbose annotations are a UX failure. The tool infers role; users accept ambiguous cases via CLI (`locus accept …`).
- **Make the canonical path shorter than the shadow path** — generators are part of the product, not a nice-to-have.
- **Source facts vs. accepted ownership.** Adapters emit facts; paradigms apply rules; lockfile is the acceptance record. Never let one bleed into another.
- **Noisy by default; configuration narrows.** Numeric and structural rules fire on un-onboarded code using built-in defaults. Vacant-by-definition paradigms (BO/PA/CR/RW/DA/UT/ER/FL/DG/CF/RM/TA/FO) emit `LOCUS002` until the user populates the relevant declaration list or adds the prefix to `Lockfile.acknowledged_empty`. Silence is a user act, not the default — when adding a rule, it should fire on its earliest meaningful evidence, with configuration shrinking the surface (overrides, exempt paths, `// locus: allow`).
- **Every rule implementation needs an independent doc-conformance sign-off.** Before merging any new `XX###` rule, dispatch a separate agent to read the rule's spec entry in `docs/PARADIGMS.md` (and `docs/project-jumpoff.md` for OT) alongside the implementation, and confirm: detection logic matches the spec; default severity matches the severity-tier table; agent-strict elevation matches the spec; lockfile schema additions are namespaced under `paradigms.<prefix>` and documented; the rule fires by default on its earliest meaningful evidence (or, for vacant-by-definition paradigms, the paradigm emits `LOCUS002` instead of silence). The reviewer is independent of the implementer — pass file paths and line numbers, not your own reasoning. If the reviewer flags drift, fix it before marking the rule done.

## AIR shape gotcha

`AirItem` is an externally tagged enum (`#[serde(tag = "kind")]`), so the discriminant occupies the JSON key `kind`. `AirType.kind` and `AirUsage.kind` are therefore serde-renamed to `type_kind` / `usage_kind` in JSON to avoid duplicate keys. The Rust field names stay `kind`. If you add another `AirItem` variant whose payload struct has a `kind` field, do the same rename.

## CLI workflow (current)

```bash
# Inspect the workspace as paradigm-neutral facts.
locus emit-air --workspace . --pretty

# Capture annotated canonicals + boundaries from a fresh scan.
locus init --workspace .

# Onboard a codebase that has no `// locus:` annotations yet.
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

# Output formats (issue #29). Text is default; json/sarif route through `locus-report`.
locus check --workspace . --format text   # human (default)
locus check --workspace . --format json   # stable schema, tooling-friendly
locus check --workspace . --format sarif  # SARIF v2.1.0 for CI ingest


# Canonical changed-only strict gate (Epic #1 workflow)
LOCUS_BASELINE=origin/main scripts/check-changed-strict.sh .
```

**`--changed` semantics:** combines three git queries — `git diff baseline HEAD` (committed
changes), `git diff HEAD` (working-tree changes), `git ls-files --others --exclude-standard`
(untracked but unignored). Default baseline tries `origin/main` → `origin/master` → `main` →
`master` → `HEAD~1`. The filter is applied after exception suppression so `// locus: allow`
hints on changed code still suppress, and `LOCUS001` expired-exception warnings still surface.

### `locus observe` — survey + advisory pressure

Read-only mode for understanding a workspace before enforcing anything:

```bash
locus observe --workspace .
```

Three sections in order: architecture survey (detected concepts, layers, hot files, crate
edges), advisory pressure (rule findings grouped by paradigm, all rendered as Advisory
regardless of underlying severity), and next declarations to consider (suggestions for
`.locus/lock.json` and `.locus/arch.json`). Always exits 0 — observe is not a gate.

Use `locus check` once you're ready to enforce.

### `locus query <kind>` — oracle lookup over AIR

Read-only architectural lookup. Answers "where does Locus see `<kind>` in this workspace?"

```bash
locus query canonical
locus query converter
locus query hot-path --json
```

Supported kinds: `canonical`, `boundary`, `converter`, plus the 11 normalized fact kinds
(`spawned-work`, `config-read`, `logging`, `external-io`, `persistence-write`,
`blocking-call`, `hot-path`, `request-context`, `boundary-entry`, `runtime-state-owner`,
`background-worker`).

Default output: aligned rows of `<symbol>  <path>:<line>`. Pass `--json` for a structured
array consumable by agents/tooling. Unknown kinds exit code 2 with the supported list on
stderr.

This is the oracle surface — `check` is the gate, `observe` is the survey, `query` is the
lookup. `query` is lockfile-free and runs over the AIR scan only; `check` and `observe`
both load `.locus/lock.json` (for rule policy and declarations respectively).

## Implementation roadmap

- ✅ AIR emission v13 (language-agnostic naming pass on top of v12 — `EnumMatch` → `DiscriminatedMatch`, `Visibility::Crate` → `Module`, `CallKind::Macro`/`DiscardKind::Macro` → `Meta`, `ArmBodyShape::Propagate` → `ErrorPropagation`, `AirItem::PartialIfLet` → `PartialResultMatch` with typed enum variant, `ConversionMechanism::From/TryFrom/InherentMethod/FreeFn` → `InfallibleAdapter/FallibleAdapter/InstanceMethod/FreeFunction` plus new `FactoryFunction`, `AirImpl` → `AirImplBlock` with `interface`/`target_type`/`dispatch`, unified `decorators` field replacing `derives`+`attrs`, added `path_segments` and `symbol_segments` for delimiter-portable matching, added `FallbackPattern` enum on `AirFallbackCall`. The Rust adapter remains the only adapter today; the schema is now ready for TS / Python / Go / Swift adapters to plug in without parallel-AIR shapes for shared concepts).
- ✅ OT paradigm: OT001–OT012 all shipped; `init` / `accept canonical|boundary` / `check` end-to-end with `--agent-strict` elevation.
- ✅ DG paradigm: DG001–DG004 (forbidden imports, cycles via Tarjan, cross-feature internals reach, shared-module reaching feature).
- ✅ FL paradigm: FL001 (boundary error in domain), FL002 (panic-shaped callees), FL003 (silent-discard `.ok()`/`.err()`), FL004 (`let _ = call(...);` discards), FL005 (partial `if let Ok/Err = ...` without `else`).
- ✅ Cross-paradigm: `Severity::from_confidence` tier helper; `// locus: allow` + lockfile exceptions wired through the CLI's `check` pipeline.
- ✅ Second rules for DC, ER, UT, RM (DC002 doc-residue phrases, ER002 string-shaped errors, UT002 forbidden imports, RM002 converter side-effects).
- ✅ **Silent-error coverage gap nearly closed** — FL004 (`let _ =`), FL005 (partial `if let`), FL006 (`map_err(|_|)`), FL007 (catch-all `Err(_)` arm body), FL011 (bare `_` failure sink), ER005 (catch-all error mapping). AIR v10 adds `MatchArm` + `ClosureMethodCall` items. Still missing: `result.unwrap_or(literal)` (needs literal-shape capture on the default arg), spawned-task failures with no sink (needs `RuntimeStateOwner`/`BackgroundWorker` loader output), retry-loop shapes.
- ✅ **stdlib-fact rules online** — std-rt loader emits `BlockingCall`, `PersistenceWrite`, `ExternalIo` from stdlib call shapes (no framework dep needed; the architectural concepts are paradigm-neutral per spec). 5 new rules consume them: BO005 (persistence in domain), PA003 (external IO in app without port), RW002 (blocking call outside runtime owner), RM005 (validator IO), RM006 (domain method writing to storage).
- ✅ **User-marker mechanism online** — `// locus: fact <fact_kind>` source hints (parsed by hint scanner, promoted to `AirFact` by the new `markers` loader) cover the 5 reserved fact kinds the loader tier can't auto-recognise (`HotPath`, `RequestContext`, `BoundaryEntry`, `RuntimeStateOwner`, `BackgroundWorker`) plus user overrides for the std-rt-recognised kinds. 3 new rules consume markers: RW005 (blocking inside hot_path), RW006 (spawn inside hot_path), OB004 (boundary_entry without logging). Framework-specific loaders (Bevy `Update` → `HotPath`, axum `#[handler]` → `RequestContext`, etc.) plug in alongside markers as they land.
- ✅ Second rules for every paradigm (16 paradigms beyond OT/DG now each ship 2–6 rules; see `docs/PARADIGMS.md` snapshot).
- ✅ **Tractable visitor-work gap closed** — `unwrap_or` shape, retry-loop detection, scrutinee literal capture all shipped in AIR v12. The 4 rules they unlock (FL010, FL012, CF002, CF003) all online.
- ✅ **CX002 module-line budget** — caps total file line count (default 400). Per-module overrides via `paradigms.CX.module_overrides`. Brings the CX rule count to 4 (CX001/CX002/CX007/CX008).
- ✅ **Convention flip: noisy default + LOCUS002 vacancy nudge.** CX001/CX002/MO001/MO002 now fire on un-onboarded code with built-in fallback budgets (no longer silent on default sections). Vacant-by-definition paradigms (BO/PA/CR/RW/DA/UT/ER/FL/DG/CF/RM/TA/FO) emit `LOCUS002` Advisory diagnostics until the user populates declarations or adds the prefix to `Lockfile.acknowledged_empty`. Self-check on the unconfigured Locus repo emits ~132 warnings (CX/MO/DC/ER) + 13 LOCUS002 advisories.
- 🔜 Framework-specific loaders (Bevy `Update` → HotPath, axum `#[handler]` → RequestContext, sqlx → PersistenceWrite, reqwest → ExternalIo, …) remain the next infrastructure step. The architectural concepts and consuming rules are all in place; framework loaders are pure recognition-surface expansion.
- ✅ **`locus init` multi-run scan-and-report** — `init` now emits a checklist of `locus <verb> ...` commands per detected layer / concept cluster / feature / threshold; categories are `[concept] [layer] [feature] [threshold] [switch] [paradigm-vacant]`. Five vacancy seeds (AB/DA/DC/OB/RW) surface for un-onboarded paradigms; `--acknowledge-empty <PREFIXES>` silences them. New mutators: `accept converter`, `rw accept-runtime-owner`, `er add-domain-path`, `rm add-domain-path`, `pa add-application-path`. Heuristics live per-paradigm in `paradigms/<p>/init.rs` plus cross-paradigm helpers in `locus-core::init`. Spec: `docs/superpowers/specs/2026-05-08-locus-init-multi-run-design.md`; plan: `docs/superpowers/plans/2026-05-08-locus-init-multi-run.md`.
- ✅ `locus debt` — inventory of every suppression (`// locus: allow` hints + `Lockfile.exceptions`), classified Active / Expired / Unbounded, sorted with expired first. Text and `--json` modes. Backed by `locus_core::exceptions::collect_exceptions`.
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
