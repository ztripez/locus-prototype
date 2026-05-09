# OT cross-crate construction authority for adapters

Date: 2026-05-09
Issue: #31 (parent: #37 — Locus self-onboarding completion)
Related: #2 (OT construction authority hardening, closed via PR #22)

## Context

`locus-rust` builds `locus-air` types as part of the AIR adapter
contract — that is the entire reason the crate exists. Without
declaration, `OT004` ("canonical type constructed outside its accepted
converter or owner") fires on every one of those constructions during
self-dogfood.

PR #22 (`#2`) added `OtSection.converter_paths: Vec<String>` to the
lockfile and an `OT004` skip when a constructor's symbol or file path
matches one of those patterns. That fix was scoped narrowly: it could be
used to suppress single converter functions or modules, but the deeper
question remained: **does an entire adapter crate have construction
authority for the canonical types it adapts to, without per-function
annotation?**

This ADR answers yes — and shows the existing primitive already supports
the answer.

## Discovery

`OT004`'s `converter_paths` matcher applies the same `matches_pattern`
helper as the rest of the rule engine, with one extension: it checks
both the constructor function's symbol *and* the file path against each
pattern (see `crates/locus-core/src/paradigms/one_truth/rules.rs::ot004`,
the loop around line 404). Because `matches_pattern` does
prefix-with-descendants matching, a pattern like `locus_rust::*` matches
every constructor symbol that lives inside `locus-rust`.

That means `converter_paths` already supports crate-level adapter
authority. The "design change" #31 asks for is therefore a documentation
+ usage change, not a schema change.

## Decision

**Use `converter_paths` at crate granularity for adapter authority.**
For Locus's own workspace:

```jsonc
"OT": {
  "converter_paths": [
    "locus_rust::*"          // adapter for locus-air
  ]
}
```

That single pattern silences every AIR construction inside `locus-rust`,
without needing to annotate each visitor / loader function. The same
shape applies to any future adapter crate (`locus-ts`, `locus-py`, …).

## Why not a new `adapter_crates` schema field

Two options were considered:

| Approach | Pros | Cons |
|---|---|---|
| **Use existing `converter_paths` at crate scope** *(decided)* | Zero schema change; already test-covered in PR #22; one line of config silences the entire adapter surface. | Less self-documenting than a dedicated field name. |
| Add `adapter_crates: [{ adapter, canonical_for }]` schema | Self-documenting intent; could enforce that constructions outside `canonical_for`'s package still fire `OT004` (a tighter contract). | Schema bloat. The tighter contract is desirable in principle, but no actual project has surfaced a case where an adapter crate constructs canonical types from an *unrelated* crate; until that case appears, the tighter contract is speculation. |

The decision is to accept the slightly less semantic name (`converter_paths`)
in exchange for not adding schema we don't have evidence we need.
The deeper authority concept #31 raises is **already structurally
expressible** — we just hadn't documented or used it that way.

If a future case proves the tighter contract worth having (e.g., an
adapter crate accidentally constructs unrelated canonicals because
`converter_paths` is too coarse), we can layer `adapter_crates` on top
of `converter_paths` without breaking existing config. This ADR doesn't
preclude that; it just doesn't pre-build it.

## Test coverage

A new regression test (`ot004_quiet_for_crate_level_converter_path`)
asserts the documented behaviour: a `converter_paths` pattern of
`adapter_crate::*` silences `OT004` on every construction whose
constructor symbol starts with `adapter_crate::`. The existing
`ot004_quiet_for_converter_path_authority` test already covers
narrower-prefix cases.

## Acceptance criteria (from #31)

- [x] Design doc / spec update explaining how cross-crate construction
      authority works (this file).
- [x] `OT004` distinguishes illicit shadow construction from legitimate
      adapter construction (via `converter_paths` matching the adapter
      crate's symbol prefix; tested today and via the new
      `ot004_quiet_for_crate_level_converter_path` test).
- [x] The design does not require annotating every individual
      constructor function in an adapter crate (one `converter_paths`
      entry per adapter crate suffices).
- [x] Self-dogfood `OT004` findings for AIR construction are explainable
      by authority config — `paradigms.OT.converter_paths` carries
      `"locus_rust::*"`.
- [x] Tests cover allowed adapter construction (new regression test) and
      disallowed construction elsewhere (existing `ot004_fires_on_canonical_construction_outside_owner_and_converter`).

## Implementation

This ADR ships with:

1. The new regression test in `crates/locus-core/src/paradigms/one_truth/rules.rs`.
2. `paradigms.OT.converter_paths = ["locus_rust::*"]` in Locus's own
   `locus.lock`.
3. Verification via `locus check --workspace . --agent-strict` that
   `OT004` no longer fires on AIR adapter constructions.
