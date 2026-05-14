# Locus

Locus is a deterministic architecture guardrail for codebases that use LLM coding agents.

LLMs are often strong at architecture planning but weak at architecture-preserving implementation. They can describe a sound system, then make local code changes that silently violate it: shadow models, hardcoded policy, boundary leaks, speculative abstractions, god modules, swallowed failures, and runtime-state shortcuts.

Locus turns architectural intent into enforceable local constraints.

Core question:

> Does this code have architectural authority to do what it is doing here?

If not, Locus should produce a precise diagnostic and point to the accepted owner, boundary, converter, config source, or runtime owner.

## What Locus is

Locus is intended to be:

- language-agnostic at the core,
- Rust-first in implementation,
- deterministic and CI-friendly,
- useful as an architectural oracle for coding agents,
- based on source facts plus accepted ownership metadata,
- strict for agent-generated patches,
- conservative about framework-specific rules until sub-paradigm loaders exist.

## What Locus is not

Locus is not:

- a style linter,
- a formatter,
- a generic clone detector,
- an LLM reviewer,
- a framework-specific rule bundle,
- a replacement for architecture planning,
- a hand-written architecture config that drifts,
- a macro system that projects must depend on.

Blocking findings must be derived from deterministic source facts and accepted ownership. Locus may later have advisory modes, but `locus check` must not rely on an LLM to find or classify violations.

## Core architecture

Locus separates source facts from architectural decisions.

Language adapters emit normalized source facts into AIR, the Architecture Intermediate Representation. Schema v8 carries:

- symbols (package-prefixed for cross-crate uniqueness),
- types (struct / enum / alias / union / trait) with fields, variants, derives, and doc text,
- functions with signature, line count, and doc text,
- inherent and trait `impl` blocks with their method names,
- imports (each `use` leaf flattened, `crate::` prefix normalized),
- call sites (function / method / macro) with rendered callee text and enclosing function,
- conversions (`From`, `TryFrom`, inherent methods, free functions),
- truth actions (construct, enum-match, string-compare, validate, normalize),
- source hints — paradigm-scoped (`// locus: ot canonical`, `// locus: ot boundary <concept> <boundary>`, `// locus: ot converter`, `// locus: ot protocol-translation reason="…"`, `// locus: ot generated-boundary`) and generic (`// locus: allow XX### reason="…" expires="…"`, `// locus: fact <fact_kind>`),
- normalized loader facts (`spawned_work`, `config_read`, `logging` today; `external_io`, `persistence_write`, `blocking_call`, `hot_path`, `request_context`, `boundary_entry`, `runtime_state_owner`, `background_worker` reserved for future loaders).

What's *not* yet in AIR (planned, tracked in `CLAUDE.md` roadmap): general literal capture beyond truth actions, full branch / arm-body inspection, discarded-binding (`let _ =`) tracking, retry-loop shape detection. These limit how deeply some rules can match the spec.

The core rule engine consumes AIR plus accepted ownership metadata in `.locus/lock.json` and emits diagnostics.

Framework or runtime-specific knowledge enters through deterministic sub-paradigm loaders. The current `std-rt` loader covers Rust language-level patterns (`tokio::spawn` / `std::thread::spawn` / `rayon::spawn` → `spawned_work`; `std::env::var` family → `config_read`; the print/dbg/log macro family → `logging`). Framework-specific loaders (reqwest, sqlx, tracing-spans, axum, …) are out of scope until the paradigm rule set is more complete — Locus rules operate on the normalized facts, not framework-specific opinions.

## Main documents

- [`docs/PARADIGMS.md`](docs/PARADIGMS.md) defines the architectural paradigms Locus is intended to guard.
- [`docs/AGENT_GUARDRAILS.md`](docs/AGENT_GUARDRAILS.md) is a compact primer for code assistants working on this repository.
- [`docs/AGENT_LOCUS_HARNESS.md`](docs/AGENT_LOCUS_HARNESS.md) describes the A/B harness for comparing agent changes with and without Locus guidance.

## Early implementation direction

Start small and deterministic:

1. Rust adapter emits AIR for types, functions, imports, comments, literals, conversions, and basic failure patterns.
2. Core builds an ownership graph from AIR.
3. Lockfile records accepted ownership decisions.
4. Rule engine checks changed code in human mode and agent strict mode.
5. Framework-specific rules are deferred until the loader system exists.

The first useful Locus does not need perfect semantic understanding. It needs to catch high-confidence architectural drift before it lands.
