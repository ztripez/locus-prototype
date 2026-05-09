# DG `public_api` policy for Locus self-onboarding

Date: 2026-05-09
Issue: #30 (parent: #37 — Locus self-onboarding completion)
Related: #3 (DG public-API onboarding hardening, closed via PRs #22 / #35)

## Context

Locus's own workspace had no DG features declared, so the rule engine gave
us no opinion at all about whose code is allowed to import what across
crates. Self-dogfood with naive feature declarations produced hundreds of
`DG003` findings (cross-feature internals reach) — the bulk rooted in the
question "what *is* this crate's public API?" rather than real
architectural drift.

This ADR fixes the policy.

## Constraint discovered

`crates/locus-core/src/paradigms/dependency_graph/lockfile_schema.rs::matches_pattern`
is a prefix-match-with-descendants matcher. `foo::*` matches `foo::bar`
and `foo::bar::baz`. There is no single-segment wildcard. Practical
consequence: `<crate>::*` as a `public_api` value declares every path
inside the crate public, which silences DG003 entirely for that crate.
The matcher does not let us write "everything re-exported at the crate
root, but not deeper sub-paths" as a single pattern.

The policy below lives with that constraint by enumerating re-exports
explicitly where discrimination matters, and by accepting `<crate>::*`
where every path *is* canonical surface by design.

## Decision

For each Locus crate, the DG `public_api` patterns are:

- **`locus-air`** — `["locus_air::*"]`. The crate is pure paradigm-neutral
  data. Every `pub` type is one of the canonical AIR concepts (`AirWorkspace`,
  `AirType`, `AirItem`, `AirFact`, …) — that is the contract the crate
  exists to publish. No discrimination is needed because there is no
  "internals" — every type is meant to be reachable by name.
- **`locus-core`** — explicit list mirroring `pub use` at lib.rs root, plus
  the per-paradigm prefix constants used cross-crate. See "Concrete
  patterns" below.
- **`locus-rust`** — explicit list mirroring `pub use` at lib.rs root, plus
  the `loaders` sub-module which is `pub mod`-exposed by design.
- **`locus-cli`** — `[]`. Binary crate with no library surface.
- **`locus-report`** — `[]`. Stub awaiting Phase 2 / 3 work.

For sub-paths used cross-crate today that aren't re-exported at the lib
root (e.g., `locus_core::paradigms::dependency_graph::DG_PREFIX` used by
the integration test in `crates/locus-core/tests/dg_basic.rs`), the
**default treatment is to add the sub-path to `public_api`**, capturing
reality. Refactoring those into clean lib-root re-exports is left as
follow-up — explicit `public_api` entries make the cross-crate dependency
visible, which is the ADR's main intent.

## Concrete patterns

The patterns below are what `paradigms.DG.features` will contain in
`locus.lock`.

```jsonc
{
  "name": "locus-air",
  "module": "locus_air::*",
  "public_api": ["locus_air::*"]
},
{
  "name": "locus-core",
  "module": "locus_core::*",
  "public_api": [
    // re-exports at lib.rs root
    "locus_core::Diagnostic",
    "locus_core::Severity",
    "locus_core::CheckMode",
    "locus_core::VACANT_PARADIGM_RULE",
    "locus_core::vacant_paradigm_diagnostic",
    "locus_core::EXPIRED_EXCEPTION_RULE",
    "locus_core::apply_exceptions",
    "locus_core::today_utc",
    "locus_core::CommandOption",
    "locus_core::Suggestion",
    "locus_core::SuggestionCategory",
    "locus_core::Loader",
    "locus_core::apply_loaders",
    "locus_core::Lockfile",
    "locus_core::LockfileError",
    "locus_core::Paradigm",
    "locus_core::registry",
    // sub-module surface used cross-crate today
    "locus_core::paradigms::dependency_graph::DG_PREFIX"
  ]
},
{
  "name": "locus-rust",
  "module": "locus_rust::*",
  "public_api": [
    "locus_rust::scan",
    "locus_rust::scan_raw",
    "locus_rust::ScanError",
    "locus_rust::scan_hints",
    "locus_rust::MarkersLoader",
    "locus_rust::StdRtLoader",
    "locus_rust::derive_module_path",
    "locus_rust::package_to_crate_name",
    "locus_rust::render_path",
    "locus_rust::render_type",
    "locus_rust::collect_items",
    "locus_rust::loaders::*"
  ]
},
{
  "name": "locus-cli",
  "module": "locus_cli::*",
  "public_api": []
},
{
  "name": "locus-report",
  "module": "locus_report::*",
  "public_api": []
}
```

## Rationale

- **Explicit beats permissive.** The matcher's prefix-with-descendants
  semantics means a single `<crate>::*` is a binary "everything is
  public" switch. We want narrowing decisions — that's the whole point
  of declaring a public API.
- **`pub use` at lib.rs root is the contract surface.** A crate's stable
  surface is what it re-exports at its top level. Sub-module access is
  always at risk of breaking on internal refactors. DG003's job is to
  make that risk visible.
- **`locus-air` is the exception by intent.** The crate's whole reason
  to exist is to publish canonical types. Every path *is* part of the
  contract.
- **Cross-crate sub-path access is opt-in, not implicit.** When a test
  or sibling crate reaches into `paradigms::dependency_graph::DG_PREFIX`,
  the dependency becomes visible in the lockfile. Future cleanup can
  promote those to re-exports; until then, they're declared.

## Acceptance criteria (from #30)

- [x] ADR in `docs/superpowers/specs/`.
- [x] Decision recorded for each of the five crates.
- [x] Sub-module / `pub`-but-internal handling spelled out.
- [x] Test/dev-only modules covered (DG features carve crates by `module`
      pattern, not file globs — `*::tests::*` carve-outs are a
      per-other-paradigm concern, e.g., DC `exempt_paths`).
- [x] DG onboarding expectation updated: each crate gets explicit
      patterns; we don't treat the legacy DG003 count as architecture
      debt.

## Implementation

This ADR ships together with the lockfile config that puts the patterns
into `locus.lock`. See the parent PR for the actual `paradigms.DG.features`
entries and the integration check that `locus check --workspace . --agent-strict`
no longer emits DG003 floods.
