# CLAUDE.md / AGENTS.md

This file provides guidance to Claude Code (and other agents — `CLAUDE.md` is a symlink to this file) when working with code in this repository.

## Project status

Phase 1 (Rust scanner emits paradigm-neutral AIR JSON) is implemented. Workspace layout:

```
crates/
  locus-air/       # AIR data + serde — schema_version = 1, paradigm-neutral
  locus-core/      # paradigm host: registry + paradigms/<name>/ modules (Phase 2)
  locus-rust/      # Rust adapter: cargo_metadata + walkdir + syn + ot: hints
  locus-cli/       # binary `locus` (only `emit-air` subcommand so far)
  locus-report/    # STUB; populated in Phase 3
tests/fixtures/sample-crate/   # standalone fixture; NOT a workspace member
```

Two source-of-truth docs:
- **`Paradigms.md`** — umbrella architectural spec; lists all 16 paradigm families (OT, CF, DG, BO, PA, CR, RM, UT, ER, FO, AB, AC, TX, SE, OB, TA). Read this first.
- **`project-jumpoff.md`** — the OT-paradigm-specific spec (rule set OT001–OT012, source hints, lockfile shape). Read this for OT work.

## What Locus is

Locus is a multi-paradigm architecture verifier. The core question every check answers:

> Does this code have architectural authority to do what it is doing here?

Each paradigm implements that question for one architectural concern. The first paradigm being implemented is OT (Canonical Domain Ownership / "one truth"). Others (DG, BO, CF, PA, …) follow once the OT module and lockfile workflow are stable.

## Naming

- **Locus** is the name of the tool (Cargo crate, CLI binary `locus`, lockfile `locus.lock`).
- **OT / "one truth"** is a *paradigm*, the first one Locus implements. It survives in the rule ID prefix (`OT001`–`OT012`), the source-hint syntax (`// ot: canonical`, `// ot: boundary`, …), and the module name `crates/locus-core/src/paradigms/one_truth/`.
- Future paradigms get analogous prefixes (`DG###`, `CF###`, …) and their own modules under `paradigms/`.

## Architecture

Two-layer separation, strictly enforced:

1. **AIR is paradigm-neutral source facts.** Language adapters (`locus-rust`, future `locus-ts`, …) emit AIR. Adapters know nothing about paradigms; they record what *is* in source, not what it *means*.
2. **Paradigm modules consume AIR.** Each paradigm under `crates/locus-core/src/paradigms/` interprets AIR through its own lens. OT looks at name/field overlap to find shadow types; DG looks at imports to find forbidden edges; CF looks at literal values to find hidden decision data. Paradigms share `locus-core`'s graph/lockfile/diagnostic infrastructure but never import each other.

If you find yourself reaching for `syn`/`cargo_metadata` from `locus-core`, stop — that belongs in the language adapter. If you find yourself adding paradigm-specific reasoning to `locus-rust`, also stop — that belongs in a paradigm module.

## Self-application (dogfooding)

Locus must be able to scan its own source. Annotate types in this codebase with the same `// ot:` (and future paradigm-specific) source hints used by user projects, so that once `locus check` exists in Phase 2, running it against this repo produces a clean report.

Add the hint at type-creation time, not retroactively. Rules of thumb for this codebase:
- `// ot: canonical` on `locus-air` types (`AirWorkspace`, `AirType`, `AirField`, …) — they are the canonical representation of "source facts in a workspace."
- `// ot: boundary <concept> <boundary>` on `clap`-derive arg structs in `locus-cli` (CLI input shape) and on the lockfile-on-disk types Phase 2 introduces (file format).
- `// ot: converter` on `From`/`TryFrom` impls or free functions that move data between those layers.

The dogfood test is the strongest possible regression check: if Locus can't keep its own source clean, the rules are wrong.

## Test corpus

The big-corpus integration test (`crates/locus-rust/tests/emit_air_corpus.rs`) is gated on the env var **`LOCUS_TEST_CORPUS`**. When unset, it silently passes; when set to a directory, it scans and asserts coarse invariants (≥1 package, >100 items).

Recorded path:
```
LOCUS_TEST_CORPUS=/mnt/code/projects/sides/lors
```

(A 17-crate Bevy/anatom/governance workspace, ~190 source files. Locus currently scans it in ~1.2s and reports 621 types / 1822 functions / 19 conversions / 3571 items.)

Run the corpus test explicitly:
```bash
LOCUS_TEST_CORPUS=/mnt/code/projects/sides/lors \
  cargo test -p locus-rust --test emit_air_corpus -- --nocapture
```

## Non-negotiable design constraints (apply to every paradigm)

- **No proc macros as the default authoring surface.** Source hints are compact `// ot:` (and future `// dg:`, `// cf:`, …) comments only.
- **No required runtime/compile-time dependency** in projects being checked.
- **No hand-authored semantic config.** The accepted ownership graph lives in a generated `locus.lock`. A small structural YAML (paths, generated code globs) is allowed; a giant rule DSL is not.
- **Blocking rules must be deterministic.** No LLM-in-the-loop for fail/pass decisions. LLM advisory mode may exist later but never gates CI.
- **Inference-first UX.** Verbose annotations are a UX failure. The tool infers role; the developer accepts ambiguous cases via CLI.
- **Make the canonical path shorter** than the shadow path — generators (`locus add adapter`) are part of the product, not a nice-to-have.
- **Source facts vs. accepted ownership** (`Paradigms.md` §"Source Facts, Accepted Ownership"): adapters emit facts, paradigms apply rules; never let one bleed into the other.

## Inference confidence thresholds

```
>= 0.90  strong inference
>= 0.70  warning / needs acceptance (fatal in --agent-strict for new code)
>= 0.50  advisory only
```

Every inferred fact carries `confidence` and `reasons`.

## AIR shape gotcha

`AirItem` is an externally tagged enum (`#[serde(tag = "kind")]`), so the discriminant occupies the JSON key `kind`. `AirType.kind` and `AirUsage.kind` are therefore renamed to `type_kind` / `usage_kind` in JSON to avoid duplicate keys. The Rust field names stay `kind`. If you add another `AirItem` variant whose payload struct has a `kind` field, do the same rename.

## Implementation phases

- ✅ Phase 1 — paradigm-neutral AIR emission for Rust
- 🔜 Phase 2 — first paradigm module: OT (`paradigms/one_truth/`), shared `lockfile`/`graph`/`diagnostics`, `locus init` / `accept` / `check` subcommands
- Phase 3 — second paradigm: DG (Dependency Graph) — same shape, reuses everything in `locus-core`
- Phase 4+ — BO, CF, PA, others; new language adapters

Do not jump ahead — paradigms after OT depend on the lockfile model and diagnostic format settling.

## Common commands

```bash
cargo build --workspace
cargo test --workspace
cargo test -p locus-rust hints::tests        # single test by path
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo run -p locus-cli -- emit-air --workspace tests/fixtures/sample-crate --pretty
```

No CI is configured.
