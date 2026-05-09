# Self-onboarding dogfood findings (2026-05-09)

A short dogfood pass against Locus's own repo on `main` (commit `30b2552`).
Goal: configure `locus.lock` and reach a clean `locus check`. Outcome:
the pass surfaced one real bug, two init UX gaps, and a substantial
real onboarding effort that's out of scope for a single sitting. The
work was reverted so `main` stays clean. This note records what the
pass found so future work can pick it up without re-discovering.

## Bug — `// locus: ot canonical` on singletons silently dropped

`crates/locus-core/src/paradigms/one_truth/infer.rs` `cluster_concepts`
skips name-stem buckets with fewer than two members. So a
`// locus: ot canonical` on a type with no shadow peers (e.g. `AirWorkspace`,
`AirHint`) ends up in no cluster, and `build_ot_section` never emits
a `ConceptEntry` for it. Before the fix, `locus init` against the
self-annotated repo reported `auto-applied: 0 source hints promoted`
despite ~47 such annotations. After a singleton pass in
`build_ot_section` (walks AIR for hint-tagged canonicals not yet
recorded, emits a `ConceptEntry` per type with empty boundaries),
that same run reports 47 promotions.

Reverted with the dogfood branch, but the fix is small and obvious:
- Add `promote_singleton_canonicals(air, &mut section)` after the
  cluster loop.
- Use `super::infer::stem_concept_id(&ty.name)` for the concept id.
- Guard against double-insert if the cluster path already covered
  the symbol.
- Test: `singleton_hinted_canonical_lands_in_section`.

Boundary singletons (`// locus: ot boundary cli.invocation cli` on a
clap-derive struct without a domain canonical) are a deeper modelling
question — `ConceptEntry` requires a canonical, and we'd have to
either synthesize a placeholder, loosen the schema, or skip
boundary-only declarations. Defer.

## UX gap — `vacancy_seeds` ignores populated lockfile sections

`crates/locus-core/src/init.rs` `vacancy_seeds` checks
`acknowledged_empty` and `already_covered` (paradigm has a specific
suggestion), but does not consult the lockfile section itself. After
running `locus dc enable`, the lockfile carries
`paradigms.DC.require_public_docs = true`, but the next `locus init`
still emits the DC vacancy nudge.

Fix: also skip prefixes whose lockfile section is populated (use a
recursive `json_is_empty` helper — same shape as the CLI's local
`section_is_empty`).

## UX gap — seed text uses commands that don't exist

`crates/locus-cli/src/main.rs` ~1735 (the `seeds` array passed to
`vacancy_seeds`) has:

```rust
("DA", "Demand-Driven", &["locus da toggle --enabled true"]),
("DC", "Documentation", &["locus dc toggle --require-public-docs true"]),
```

Neither `da toggle` nor `dc toggle` exist as subcommands. The actual
verbs are `locus da enable` / `locus dc enable`. Update the seed
text.

## UX gap — seed list misses 11 vacant-by-definition paradigms

`init`'s `seeds` array only seeds RW/OB/AB/DA/DC. The other
vacant-by-definition paradigms (BO/CF/CR/ER/FL/FO/MO/PA/RM/TA/UT)
have `vacant_paradigm_diagnostic` wired in their `check()` so they
emit `LOCUS002` advisories at *check* time, but `init` never offers
to onboard them. Result: a freshly-`init`-ed repo passes init with
`unresolved: 0`, then `check` prints 10+ `LOCUS002` advisories.

Fix: extend the seeds array to cover all 16 vacant-by-definition
paradigms with their actual mutator surfaces. Several paradigms
already have specific cross-paradigm suggestions (BO/ER/FL/RM share
the domain-layer suggestion) — those aren't seeds, but the rest
(CF/CR/FO/MO/PA/TA/UT) need entries.

## Real onboarding effort (deferred)

The real architectural work surfaced once the bugs above were
patched. Baseline `check` output on the configured repo:

| Code     | Count | Meaning                                                            |
|----------|-------|--------------------------------------------------------------------|
| DG003    | 528   | Cross-feature internals reach. Features defined without `public_api` — every cross-crate import is an internals violation. Fix: declare `public_api` per feature. |
| OT004    | 169   | Canonical built outside owner module. The 47 promoted singletons (`AirHint`, `AirWorkspace`, …) are constructed by design across crates — `locus-rust` builds them as part of the AIR adapter contract. Fix: declare `locus-rust` modules as converters. |
| DC001    | 129   | Missing public docs. Many crate-public types/fns currently undocumented. Fix: write the docs, or add `*::tests::*` and similar to `exempt_paths`. |
| CX001    | 101   | Function-line budget breaches (built-in 50-line default). Fix: raise the budget (the spec defaults are conservative for a young codebase) or refactor. |
| CX002    | 27    | Module-line budget breaches.                                       |
| ER007    | 11    | Untyped error in domain return position.                           |
| LOCUS002 | 10    | Vacancy nudges (see seed-list gap above).                          |
| OT009    | 2     | Concept-shadow reach.                                              |
| MO001    | 2     | Module public-types budget.                                        |
| DC002    | 2     | Forbidden doc phrase (residue).                                    |
| CX007    | 1     | Per-file public-API count.                                         |

Most of this is *real architectural assertion*, not noise. Two of
these — DG003 with no `public_api` and OT004 across the AIR adapter
boundary — are deep enough they probably want a design pass rather
than a config tweak:

- DG: what's the public API of `locus-air`? Of `locus-core`? Today
  they're effectively-public-everything (the CLI, rust adapter, and
  paradigm modules all reach freely). Declaring a real public API
  forces the question of whether type-level access should narrow.
- OT: is `locus-rust` a converter for every AIR type, or is the
  AIR-construction concept itself one converter (e.g. the `scan`
  function and below)? Today's `// locus: ot converter` annotation is
  per-impl-block / per-function; covering an entire crate may need
  a glob form.

DC, CX, MO, ER, LOCUS002 are local config / write-the-docs work.

## Recommended next steps

1. Land the OT singleton-canonical fix (small, isolated, has a test).
2. Land the `vacancy_seeds` non-empty-section check.
3. Land the DC/DA seed text fix.
4. Extend the seed list to cover all 16 vacant-by-definition
   paradigms.
5. *Then* attempt onboarding. The DG and OT design questions probably
   want their own ADRs before anyone tries to clean up the 528+169.

Follow-up execution plan: `docs/superpowers/plans/2026-05-09-issue-1-epic-execution.md`.
