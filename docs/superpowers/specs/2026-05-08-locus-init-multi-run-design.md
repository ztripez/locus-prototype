# Locus init — multi-run scan-and-report design

**Status:** draft (brainstorming output, pending review)
**Date:** 2026-05-08
**Supersedes (in priority):** the front of `docs/CLI_AGENT_INTERFACE.md`. The agent-output / explain / query / debt / prune surface from that doc is *not cancelled* — it moves behind this work.
**Related:** `docs/PARADIGMS.md` (paradigm semantics), `docs/CLI_AGENT_INTERFACE.md` (the broader CLI plan, on ice while this lands).

## Why this spec exists

Locus recently flipped to a noisy default: empty paradigm sections fire `LOCUS002`, and structural/numeric rules (CX, MO, ER, DC, AB) fire on un-onboarded code with built-in defaults. That stops LLMs from cheating by ignoring vacant rules — but it also means the very first thing an agent (or human) sees on a fresh repo is a wall of warnings.

`locus init` is the bridge from "wall of warnings" to "Locus understands this codebase." Today it only promotes `// ot:` source hints into `locus.lock` and adds canonicals/boundaries from a fresh scan. That covers OT and almost nothing else; 18 of the 19 paradigms remain vacant after a clean `locus init`.

This spec redefines `locus init` as a **scan-and-report** command that, in addition to its current source-hint promotion behaviour, prints a checklist of the *exact CLI commands* an agent should run next to make the lockfile sound for this codebase. Agents (or humans) execute commands one at a time and re-run `locus init` until the checklist is empty.

## Design constraints

These choices were settled during brainstorming and are not up for debate inside this spec:

- **No daemon, no JSON-RPC, no MCP server.** Each agent turn is one CLI invocation reading terse stdout. Token cost has to stay low.
- **No persistent architecture model on disk.** Only `locus.lock` (and the existing `.locus/last-check.json` from `check`) live in the workspace. Heuristic state is transient — recomputed every `init` run from `AirWorkspace + Lockfile`.
- **`init` never auto-writes inferred decisions.** Only strictly-mechanical promotions (existing `// ot:` source hints) commit silently. Every heuristic-detected layer / cluster / feature / threshold becomes a `locus <verb> ...` command on the checklist; the agent decides whether to run it.
- **Aggressive name+shape clustering with confirmation.** Heuristics propose; the agent confirms by running the suggested `accept` command, or splits a cluster by running two `accept` commands instead of one.
- **Per-paradigm `init.rs`** owns each paradigm's heuristics. OT already has `crates/locus-core/src/paradigms/one_truth/init.rs`; the pattern extends to the other 18 paradigms.
- **Text output only for now.** A `--format json` mode can come later if a programmatic consumer appears; not in this spec.

## Behaviour contract

A run of `locus init --workspace .` does the following, in order:

1. **Scan.** Build `AirWorkspace`. Load existing `locus.lock` (or empty).
2. **Promote source hints (existing behaviour).** Apply `// ot: canonical`, `// ot: boundary`, `// ot: converter` hints into `locus.lock`. This is the only auto-write step. Existing accepted entries are preserved; entries with no source backing them and no `--prune` flag are kept (matching today's behaviour).
3. **Run heuristics per paradigm.** Each paradigm's `init.rs` exposes:
   ```rust
   pub fn suggest(
       air: &AirWorkspace,
       section: &<Paradigm>Section,
       acknowledged_empty: bool,
   ) -> Vec<Suggestion>;
   ```
   `Suggestion` (defined in `locus-core`) is a paradigm-neutral struct carrying a category, a one-line headline, a few `why` lines, and one or more `Command` blocks the agent can run. `Suggestion`s are *not* fired as `Diagnostic`s — they are init-only.
4. **Aggregate + render.** All paradigm suggestions are gathered, sorted by category, and printed as the checklist (format below).
5. **Write AGENTS.md handoff block** if the existing `--agent-instructions` flag asks for it (current behaviour; small markered block; never the rule catalogue).
6. **Exit code:**
   - `0` if no unresolved suggestions remain (all paradigms either onboarded or in `acknowledged_empty`);
   - `1` if the checklist is non-empty.

Re-running `init` after the agent applies suggested commands re-derives state from the updated lockfile and AIR; the checklist shrinks. The loop terminates either when the checklist is empty or when every remaining paradigm has been added to `acknowledged_empty`.

### `Suggestion` and `Command` shape

```rust
// crates/locus-core/src/init.rs (new module)

pub struct Suggestion {
    pub category: SuggestionCategory,
    pub headline: String,                // one line
    pub why: Vec<String>,                 // 0–4 short reason lines
    pub options: Vec<CommandOption>,      // 1–N alternative resolutions
}

pub enum SuggestionCategory {
    Concept,            // OT cluster
    Layer,              // domain/application/boundary/etc. assignment
    Feature,            // DG/FO feature partition
    Threshold,          // CX/MO/RM/CR numeric dials
    ParadigmVacant,     // generic LOCUS002 nudge with onboarding seed
    Switch,             // DA enabled, DC require_public_docs
}

pub struct CommandOption {
    pub label: String,                    // "if same concept", "if separate concepts", "skip"
    pub commands: Vec<String>,            // shell-ready `locus ...` invocations
}
```

The renderer turns each `Suggestion` into one block of the checklist. The structure matters more than the prose: every command must be valid as-typed, with no `<placeholder>` slots remaining. (Where the agent has to fill in a glob or path it didn't already detect, the command line uses a literal `"<glob>"` / `"<symbol>"` placeholder and the `why` block calls it out.)

## Output format

```
LOCUS INIT — workspace `.`
auto-applied: <N> source hints promoted
unresolved: <M>

[concept] cluster `order` — order::domain::Order vs order::api::OrderDto
  field overlap 3/5; same-stem; cross-layer (domain ↔ api)
  if same concept:
    locus accept canonical order::domain::Order --concept order.order
    locus accept boundary  order::api::OrderDto --concept order.order \
                                                --boundary api.v1 --direction outbound
  if separate concepts:
    locus accept canonical order::domain::Order --concept order.order
    locus accept canonical order::api::OrderDto --concept order.order_dto

[layer] crate `payments`: no domain layer detected
  no `*::domain::*` modules found; required by BO/ER/FL/RM
  specify:
    locus bo add-domain-path "payments::core::*"
  or skip:
    locus init --acknowledge-empty BO

[feature] no DG/FO features defined
  top-level modules look like features: identity, order, payments
  define:
    locus dg define-feature --name identity --module "identity::*"
    locus dg define-feature --name order    --module "order::*"
    locus dg define-feature --name payments --module "payments::*"
  or skip:
    locus init --acknowledge-empty DG,FO

[paradigm-vacant] RW has no runtime owners
    locus rw accept-runtime-owner "<glob>"
  or skip:
    locus init --acknowledge-empty RW

re-run `locus init` after applying changes.
```

Format rules:

- One block per `Suggestion`, separated by a blank line.
- First line is `[<category>] <headline>` so an agent can grep / regex.
- `why:` lines indent two spaces.
- Each `CommandOption` is rendered as `<label>:` followed by indented commands.
- The trailing `re-run` line always appears.
- Stable ordering: `Concept` → `Layer` → `Feature` → `Threshold` → `Switch` → `ParadigmVacant`. Within a category, deterministic order (alphabetical by paradigm prefix, then by primary key — concept id, layer name, feature name, etc.).

## Heuristics (per paradigm)

Each paradigm's `init.rs` is responsible for emitting `Suggestion`s for its own unfilled lockfile fields. Where a question crosses paradigms (e.g. "where is the domain layer?" is shared by BO/ER/FL/RM/PA), each paradigm emits the same suggestion under its own prefix; the renderer de-duplicates suggestions with identical `Command` sets. (De-dup happens on byte-equality of the rendered command list; concept-cluster suggestions never collide because they carry concept ids, layer suggestions collide deliberately and render once with a combined `why` line: `required by BO/ER/FL/RM/PA`.)

The detailed heuristic per paradigm:

### One Truth (OT) — concept clustering
- Cluster types by **name stem** (`User` / `UserResponse` / `UserDto` share stem `User`).
- For each cluster, score:
  - field-set Jaccard overlap between non-canonical members and the canonical candidate;
  - module-path proximity (cross-layer = +signal; same-layer = -signal toward "different concepts");
  - presence of an existing `From`/`TryFrom` impl (boost: that's the converter the user wants to register).
- Confidence ≥ ~0.95 (e.g. `From` impl + ≥80% field overlap + cross-layer): emit `[concept]` suggestion with **single** option ("accept this cluster").
- Confidence ~0.7–0.95: emit `[concept]` suggestion with **two** options ("if same concept" vs "if separate concepts").
- Confidence < 0.7: do not suggest. (Keeps the checklist short on first runs.)

### Boundary Ownership (BO) — domain & canonical paths
- Detect `domain_paths` from path conventions: any module matching `*::domain::*` or `*::core::*` not already in `acknowledged_empty`.
- Detect `canonical_paths` from `// ot: canonical` annotations and from OT `concepts` that have a canonical symbol.
- Suggest `locus bo add-domain-path "<glob>"` for each detected layer; if none detected, emit a `[layer]` suggestion with the literal placeholder.

### Error Taxonomy (ER), Failure Lineage (FL), Responsibility (RM), Port/Adapter (PA)
- All re-use BO's `domain_paths` heuristic. Same suggestion set; renderer de-dups.
- ER additionally proposes `forbidden_error_types` from any `Result<_, String>` shapes seen in domain code.
- FL additionally proposes `invariant_owner_paths` from `*::tests::*` patterns and `#[cfg(test)]` modules.
- RM additionally proposes role-paths (handler/repository/validator) from path conventions: `*::handlers::*`, `*::repo*`, `*::repositories::*`, `*::validators::*`.
- PA proposes `application_paths` from `*::application::*`, `*::usecases::*`.

### Dependency Graph (DG) & Feature Ownership (FO) — features
- Detect candidate features from top-level workspace modules (one level below crate root).
- Both paradigms emit the same suggestion (renderer de-dups). Combined `why` line: `required by DG, FO`.
- `forbid-edge` is **not** suggested by init — it requires direction intent the user has to state.

### Composition Root (CR)
- Detect `composition_root_paths` from `main.rs`, crate `lib.rs` files with high `Construct`-action density (already an OT-known fact).
- Threshold suggestions for `wiring_density_threshold` if codebase p95 differs from default ±50%.

### Config/Data (CF)
- Detect `config_paths` from `*::config::*`, `*::settings::*`. Defaults already cover `forbidden_literal_kinds` and file patterns; no suggestion unless user lockfile has overridden them oddly.

### Documentation (DC)
- `[switch]` suggestion for `require_public_docs`: present default (`false`), let agent flip on by running a setter command.
- `forbidden_doc_phrases` keep their built-in defaults; init does not propose more.

### Module Ownership (MO), Complexity Budget (CX), Responsibility (RM thresholds), Composition Root (CR threshold)
- Compute p50/p95 of relevant statistic over current AIR (function lines, file lines, public items per file, fan-out, public types per file, action kinds per function).
- If p95 is within 1.5× of the spec default: no suggestion (defaults are fine).
- Otherwise emit `[threshold]` suggestion with the proposed value as a setter command.

### Observability (OB)
- Detect modules with high `Logging` fact density → suggest `observer_paths`.
- Otherwise emit `[paradigm-vacant]` with seed.

### Runtime Work (RW)
- Cannot be detected from std-rt loader alone (the spec acknowledges this — `HotPath`, `RuntimeStateOwner`, `BackgroundWorker` are marker-driven). Init emits `[paradigm-vacant]` with seed commands and the literal `<glob>` placeholder.

### Test Architecture (TA)
- Detect `test_paths` from `tests/` directories and `#[cfg(test)]` modules.
- `canonical_name_patterns`, `canonical_field_sets` flow from OT's accepted concepts (no separate detection).

### Utility Discipline (UT)
- Detect `utility_paths` from `*::util::*`, `*::common::*`, `*::helpers::*`.

### Abstraction Discipline (AB), Demand-Driven (DA)
- Defaults cover the common cases. AB only emits a suggestion if AB001 fires more than N times on a fresh check (heuristic threshold). DA stays disabled by default (`enabled: false`) so no suggestion until the user opts in.

### `LOCUS002` integration
For any paradigm whose section is empty AND not in `acknowledged_empty` AND has no specific suggestion above, init emits a generic `[paradigm-vacant]` suggestion: paradigm name, headline, the seed command set, and the `--acknowledge-empty <PREFIX>` escape. This is the same message `LOCUS002` would surface, just collected in init's checklist.

## CLI surface this spec adds or requires

This spec is intentionally narrow: it is the init flow only. The agent commands the checklist points at must exist. Most already do.

**Already shipped, no work needed:**
- `locus accept canonical | boundary`
- `locus bo add-domain-path | add-forbidden-import`
- `locus dg define-feature | forbid-edge | add-shared-path`
- 14 other per-paradigm subcommands that today only carry the verbs `init`'s checklist needs.

**Missing — required by init's checklist and shipped as part of this spec:**
- `locus accept converter <symbol> --concept <id> [--from <symbol>] [--to <symbol>] [--reason <text>]` (OT005 already wants this; lockfile schema gains `paradigms.OT.converters[]`).
- `locus rw accept-runtime-owner <pattern> [--reason <text>]` (entire RW subcommand absent today; RW lockfile schema already has `runtime_owner_paths`).
- `locus init --acknowledge-empty <PREFIXES>` (writes `Lockfile.acknowledged_empty`).

**Out of scope for this spec, deferred to the broader CLI plan:**
- `accept protocol-translation`, `rw accept-background-worker / accept-hot-path`, `cf accept-decision-owner / forbid-id-pattern`, `fl accept-failure-sink / accept-retry-policy / allow-discard`, `locus allow`, `locus debt`, `locus prune`, `locus explain`, `locus query`, `locus check --format agent`, `locus agent instructions`. None of these are required for init's checklist to function — init can `[paradigm-vacant]`-seed those paradigms via the verbs that *do* exist (or via `--acknowledge-empty`) and the broader CLI plan picks them up later.

## Phased rollout

This work is sequenced in roughly phase-of-day-sized chunks. Each phase is independently shippable.

**Phase 1 — `Suggestion` infrastructure**
- New `crates/locus-core/src/init.rs` module: `Suggestion`, `SuggestionCategory`, `CommandOption` types; aggregator that consumes per-paradigm `init::suggest` outputs and de-duplicates byte-identical command sets.
- `Lockfile.acknowledged_empty: Vec<String>` (already exists per recent CLAUDE.md change — verify and re-use; do not duplicate).
- `init` subcommand wires the aggregator into its existing flow; renders the checklist; preserves source-hint promotion.
- New flag `locus init --acknowledge-empty <PREFIXES>` (comma-separated; merges into the lockfile, no duplicates).

**Phase 2 — Path-convention heuristics (highest leverage, lowest risk)**
- Layer detection (BO/ER/FL/RM/PA `domain_paths` + RM role paths + PA `application_paths`).
- Composition root detection (CR).
- Test path detection (TA).
- Utility path detection (UT).
- Config path detection (CF).
- After this phase, a fresh repo with conventional `crate::user::{domain,api,...}` layout gets a useful onboarding checklist on the first `init` run.

**Phase 3 — Concept clustering (OT)**
- Stem extraction, field-set Jaccard, cross-layer scoring, `From`/`TryFrom` discovery.
- Confidence-tier suggestion rendering (single-option vs two-option).
- Adds `locus accept converter` mutator (mechanical; OT005 already consumes the lockfile field).

**Phase 4 — Feature partitioning (DG/FO)**
- Top-level module enumeration; suggestion de-dup across paradigms; `define-feature` commands rendered.

**Phase 5 — Threshold dial-in (CX/MO/RM/CR)**
- p50/p95 computation over AIR.
- Setter commands for non-default thresholds. (Some setter verbs already exist; verify per paradigm.)

**Phase 6 — Vacancy seeding for the long tail**
- `[paradigm-vacant]` suggestions for OB/RW/AB/DA/DC and any paradigm without phase 2–5 detection.
- Adds the missing RW subcommand (`accept-runtime-owner`).

**Phase 7 — Snapshot / fixture coverage**
- `tests/fixtures/sample-crate` gets exercised end-to-end: `locus init` → snapshot of checklist output; expected lockfile state after walking the suggestions.
- A second fixture seeded with a concept-cluster ambiguity (User / UserResponse + From impl) so the OT phase has a regression home.

## Testing strategy

- **Per-paradigm `init::suggest` unit tests** with hand-built `AirWorkspace` fixtures. One test per category × signal-shape per paradigm.
- **Snapshot tests** (`insta`) on the rendered checklist for the sample crate at each phase boundary. Catches format regressions and ordering drift.
- **Round-trip test**: `locus init` → run every `if same concept` / single-option command in the checklist → `locus init` again → checklist is empty (or only `[paradigm-vacant]` items remain). Run against the sample crate and the seeded concept-cluster fixture.
- **De-duplication test**: a fixture where BO and ER both want the same `domain_paths` glob; assert one rendered block, combined `why` line.
- **Self-application**: `locus init --workspace .` against this repo never proposes a command that would fail when executed (every command is well-formed and resolves).

## Risks

1. **Heuristic false positives in OT clustering.** Aggressive name+shape clustering will sometimes propose a single-concept resolution where the agent wants two. Mitigation — confidence tiering: high-confidence offers a single option, mid-confidence offers two options. Agent always has the "split into two canonicals" path.
2. **Checklist length on large workspaces.** A 50-crate workspace with no onboarding state could emit hundreds of suggestions. Mitigation — phase-2 path detection auto-scales (one suggestion per crate, not per file), and `[paradigm-vacant]` items collapse to one per paradigm. Add a `--limit <N>` flag if real-world repos are still overwhelming after phase 6.
3. **De-dup drift.** Byte-equality on rendered commands is fragile if commands gain optional flags. Mitigation — keep the canonical command renderer in one place (`Suggestion::render`); de-dup before whitespace formatting.
4. **`acknowledged_empty` UX.** Once a paradigm is acknowledged-empty, init won't suggest it again, but the user might *want* a nudge later. Mitigation — `LOCUS002` continues to fire for paradigms in `acknowledged_empty=false`; once the user removes a prefix from the list, init suggestions return automatically on the next run.

## Non-goals

- This spec does **not** define `locus check --format agent`, `locus explain`, `locus query`, `locus debt`, `locus prune`, the AGENTS.md content beyond the existing markers, or the broader oracle surface. Those are the deferred work in `docs/CLI_AGENT_INTERFACE.md`.
- This spec does **not** introduce a daemon, an MCP server, a JSON-RPC protocol, or any persistent on-disk state beyond `locus.lock`.
- This spec does **not** propose any LLM-in-the-loop reasoning during `check`; heuristics here run only inside `init` and never affect rule-engine pass/fail.
- This spec does **not** change rule semantics; it only changes onboarding ergonomics.

## Open questions

These are flagged for review; none block writing the implementation plan.

1. **Does `acknowledged_empty` already exist on `Lockfile`?** The recent CLAUDE.md edit refers to it; needs a concrete check during phase 1 implementation. If not, phase 1 adds the field.
2. **Confidence thresholds** in OT clustering (~0.95 for single-option vs 0.7 for two-option) are stated as design intent here; final numbers come from running phase-3 heuristics against the sample crate + lors corpus and tuning.
3. **Threshold delta tolerance** — "p95 within 1.5× of default → no suggestion" is a starting point; revisit during phase 5.
