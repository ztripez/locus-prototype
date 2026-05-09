# Locus — TODO

Pickup list for fresh contexts. Ordered by leverage. Each entry has
enough detail to start without the chat history.

## Small fixes (1–3h each)

### 1. `vacancy_seeds` ignores populated lockfile sections
**Where:** `crates/locus-core/src/init.rs` `vacancy_seeds`.
**Symptom:** After `locus dc enable`, the lockfile carries
`paradigms.DC.require_public_docs = true`, but the next `locus init`
still emits the DC vacancy nudge.
**Fix:** Skip prefixes whose lockfile section is non-null and
non-empty. Reuse the `section_is_empty` shape from `crates/locus-cli/
src/main.rs:1783` (recursive check on `Value::Null`/empty obj/empty
array). Add a test: pre-populate `paradigms.DC` with a toggle, assert
the seed is suppressed.

### 2. Seed text for DC/DA references commands that don't exist
**Where:** `crates/locus-cli/src/main.rs` ~1735, the `seeds` array
passed to `vacancy_seeds`.
**Today:** `locus dc toggle --require-public-docs true` and `locus da
toggle --enabled true`.
**Fix:** Real verbs are `locus dc enable` and `locus da enable`.
Update both seed entries.

### 3. Seed list missing 11 vacant-by-definition paradigms
**Where:** Same `seeds` array.
**Today:** Only RW/OB/AB/DA/DC are seeded. BO/CF/CR/ER/FL/FO/MO/PA/
RM/TA/UT emit `LOCUS002` advisories from `check` but `init` never
offers to onboard them — `init` reports `unresolved: 0` while `check`
prints 10+ advisories.
**Fix:** Extend the seed list with each paradigm's actual mutator
(BO `bo add-domain-path`, CR `cr add-composition-root`, etc.). Some
already have specific cross-paradigm suggestions (BO/ER/FL/RM share
the domain-layer suggestion) — leave those as-is, add the rest.

### 4. OT singleton-canonical promotion
**Where:** `crates/locus-core/src/paradigms/one_truth/init.rs`
`build_ot_section`.
**Symptom:** A `// ot: canonical` on a type with no name-stem peers
ends up in no cluster (`cluster_concepts` skips buckets with
`members.len() < 2`) and is silently dropped from the lockfile.
Self-run on the annotated repo reports `auto-applied: 0 source hints
promoted`.
**Fix:** After the cluster loop, walk the AIR for hint-tagged
canonicals not yet recorded; emit a `ConceptEntry` per type with
empty boundaries. Use `super::infer::stem_concept_id(&ty.name)` for
the concept id. Guard against double-insert if the cluster path
already covered the symbol. With this fix, self-run promotes 47
concepts. Test: `singleton_hinted_canonical_lands_in_section`.

Boundary-only singletons (`// ot: boundary <concept> <bnd>` without
a domain canonical) are a deeper schema question — `ConceptEntry`
requires a canonical. Defer.

Reference: `docs/superpowers/specs/2026-05-09-self-onboarding-
findings.md`.

## Oracle commands (each ~half-day)

Spec: `docs/PARADIGMS.md` §"Locus as an Architectural Oracle".
`locus debt` already shipped (commit `43c2300`).

### 5. `locus explain <RULE_ID>`
Look up a rule's spec entry from `docs/PARADIGMS.md` (e.g. `OT004`)
and print the section. Source-of-truth lives in markdown; the CLI is
just a parser+printer. Stretch: include the rule's current default
severity and any active overrides from the lockfile.

### 6. `locus query <kind>`
Filter AIR by item kind (`canonical`, `boundary`, `converter`,
`spawned-work`, `external-io`, …) and print matching symbols + spans.
Dual-use: agent debugging ("show me everything tagged `hot_path`")
and onboarding ("are there any persistence writes outside accepted
repos?"). Reuse the existing `paradigm_section` lookups; add a small
matcher per kind.

### 7. `locus graph`
Emit a Graphviz/Mermaid graph of declared features and their public-
API edges (DG). Optional flags: `--include cycles` (overlay DG002
SCCs), `--feature <name>` (subgraph). Output to stdout or `--out
graph.dot`.

### 8. `locus prune`
Remove expired exceptions from `Lockfile.exceptions`. Optional
`--also-hints` to suggest source-hint deletions (the CLI can't
edit source files unilaterally — surface them as a checklist like
`locus init` does).

### 9. `locus add adapter <name>`
Scaffold a new language-adapter crate (`locus-<name>`) with the AIR
emission contract stubbed and a smoke test pointing at a fixture.
Lower priority — only useful when the second adapter is actually
on the table.

## Loaders (multi-day, infra-shaped)

### 10. Framework-specific fact loaders
Loader output enriches AIR with normalized fact kinds that paradigms
already consume. Today only `std-rt` and `markers` ship. Each
loader is recognition-only — no rule changes required.

- **Bevy**: functions registered in `Update`/`FixedUpdate`/`PostUpdate`
  schedules → `HotPath` facts. Likely matches on
  `App::add_systems(Update, ...)` and similar.
- **axum**: `#[handler]` / `axum::Router::route(...)` registrations
  → `RequestContext` and `BoundaryEntry` facts.
- **sqlx**: `query!`/`query_as!`/`Pool::execute` calls →
  `PersistenceWrite` facts.
- **reqwest**: `Client::get/post/...` → `ExternalIo` facts.

Where to put them: probably `crates/locus-rust/src/loaders/<name>.rs`
or a new sibling crate per loader if they grow. Tests: per-loader
fixture corpus (`tests/fixtures/loaders/<name>/`) with assertions
that the expected `AirFact` rows are emitted.

Spec: `docs/PARADIGMS.md` covers the loader system.

### 11. SARIF / JSON formatters in `locus-report`
`crates/locus-report/` is currently a stub. The CLI hand-rolls
human-readable diagnostic output and `--json` is one-row-per-line.
Land:
- `locus-report::sarif::write(diagnostics, writer)` — SARIF v2.1.0,
  one run, one tool driver (`Locus`), one rule per active rule id.
  Map severity → SARIF level, span → location, `why` → message.
- `locus-report::json::write(diagnostics, writer)` — stable JSON
  shape covering everything `--json` emits today plus paradigm
  metadata.

CLI rewires `--format sarif|json|text`. Drives CI integration.

## Onboarding push (deferred — needs design first)

Self-onboarding the Locus repo surfaces 528+169 violations rooted in
two design questions, not just config tweaks. See
`docs/superpowers/specs/2026-05-09-self-onboarding-findings.md` for
the categorised baseline.

### 12. DG `public_api` design pass
Each crate currently has no declared `public_api` patterns, so every
cross-crate import is a DG003 violation (528 today). Real question:
*what is the public surface of `locus-core`, `locus-air`, `locus-
rust`?* Today's de-facto surface is "everything pub" — declaring a
real one forces narrowing decisions. Want an ADR before implementing.

### 13. OT cross-crate construction design pass
`locus-rust` constructs `locus-air` types as part of the AIR adapter
contract; OT004 fires 169× on these constructions. Today's `// ot:
converter` annotation is per-impl-block / per-function. Question: is
`locus-rust` *itself* a converter (one annotation covers the crate),
or does the AIR-construction concept need a glob form? Probably
needs a small spec change.

### 14. Then onboard the rest
DC (write the docs or add `*::tests::*` to `exempt_paths`), CX (raise
budgets where intentional, refactor the rest), MO/ER/LOCUS002 (local
config). Bulk work, mechanical once the design questions above
settle.

## Stretch / nice-to-have

### 15. `locus check --patch <file>`
Already have `--changed`. Adding a `--patch <file>` mode that takes
a unified-diff file (e.g. from `git format-patch`) would unblock
"check this patch in isolation" workflows. Small parser, reuses the
existing diff filter.

### 16. Future paradigms (AC, TX, SE)
`docs/PARADIGMS.md` reserves prefixes for Access Control (AC),
Transaction Boundary (TX), Side-Effect (SE). All depend on the
loader work in §10/11 — they consume `RequestContext`/`HotPath`/
`PersistenceWrite` facts that aren't widely emitted yet.

## Reading order for a fresh agent

1. `README.md` — what Locus is.
2. `docs/AGENT_GUARDRAILS.md` — non-negotiables.
3. `AGENTS.md` (== `CLAUDE.md`) — current state + roadmap.
4. `docs/PARADIGMS.md` — paradigm spec.
5. This file — open work.
