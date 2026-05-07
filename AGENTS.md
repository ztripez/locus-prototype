# CLAUDE.md / AGENTS.md

This file is the per-repo dev-handoff for Claude Code (and other agents — `CLAUDE.md` symlinks to this file). It describes *current state* and how to keep working on it. For the underlying architectural rules, see `docs/`.

## Reading order

1. **[`README.md`](README.md)** — what Locus is and isn't, in two screens.
2. **[`docs/AGENT_GUARDRAILS.md`](docs/AGENT_GUARDRAILS.md)** — non-negotiables for agents working on Locus itself (determinism, no LLM in `check`, no broad ignores, etc.). Read before adding anything to the rule engine.
3. **[`docs/PARADIGMS.md`](docs/PARADIGMS.md)** — full umbrella spec; every paradigm Locus is meant to guard. Use as the source of truth for paradigm semantics, source-fact taxonomy, and the architectural-authority framing. Paradigm 1 carries summary rule entries (OT001–OT012), source-hint forms, and severity tiers.
4. **[`docs/project-jumpoff.md`](docs/project-jumpoff.md)** — the original OT-paradigm deep dive. Read for full spec content (CLI command surface, lockfile examples, generator design, exception format). Pre-dates the multi-paradigm reframing, so treat its top-level "Locus is …" framing as historical; the rule definitions and AIR examples remain authoritative.

## Project status

OT (Canonical Domain Ownership) is wired end-to-end: AIR emission, paradigm host, OT002 rule, lockfile, `locus init / accept / check` CLI. Locus's own source is annotated; `locus check --workspace .` is clean.

Workspace layout:

```
crates/
  locus-air/       # paradigm-neutral data + serde, schema v3
  locus-core/      # paradigm host + OT module, shared diagnostics + lockfile
  locus-rust/      # cargo_metadata + walkdir + syn + ot: hints + clean type renderer
  locus-cli/       # binary `locus`: emit-air | init | accept canonical|boundary | check
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

# Run all enabled paradigms; exit non-zero on Fatal.
locus check --workspace .                # human mode (warnings)
locus check --workspace . --agent-strict # warnings → fatal
```

## Implementation roadmap

- ✅ AIR emission (Rust adapter, package-prefixed symbols, clean type rendering)
- ✅ OT paradigm: `init` / `accept canonical|boundary` / `check`, OT002 with `--agent-strict` elevation
- 🔜 OT001 (duplicate canonical) and OT006 (unregistered conversion) — both nearly trivial now that the lockfile records canonical symbols
- 🔜 OT003 (boundary leak), OT004 (direct canonical construction), OT007 (adapter-to-adapter conversion)
- Then: second paradigm — most likely DG (Dependency Graph), since the facts (`AirUsage`) are already in AIR
- Then: deterministic loaders (`docs/PARADIGMS.md` covers the loader system) for framework-specific normalized facts

Don't jump ahead. Paradigms after OT depend on the lockfile/diagnostic conventions settling.

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
