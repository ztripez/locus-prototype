# Agent Guardrails for Locus

This document is for code assistants working on Locus.

Locus is a deterministic architecture guardrail. Do not implement it as another vague linter, LLM reviewer, or framework-specific opinion bundle.

## Core rule

Before adding code, ask:

> Does this code have architectural authority to do what it is doing here?

If the answer is unclear, prefer emitting source facts and deferring ownership decisions to the core/lockfile model rather than hardcoding assumptions.

## Non-negotiables

1. Blocking checks must be deterministic.
2. Do not rely on an LLM to find or classify violations.
3. Keep the core language-agnostic.
4. Keep framework-specific knowledge out of core rules.
5. Frameworks belong behind deterministic loaders that emit normalized facts.
6. Do not require projects to depend on Rust proc macros.
7. Prefer inference plus accepted lockfile decisions over large hand-written architecture config.
8. Source hints should be compact and optional.
9. Do not add broad ignores; exceptions must be specific, reasoned, and expiring.
10. Agent strict mode should be stricter than human mode.

## Correct architecture shape

Language adapters emit AIR facts.

Loaders enrich AIR with deterministic normalized facts.

Core rules evaluate facts against accepted ownership.

```text
source code
  -> language adapter
  -> AIR facts
  -> optional deterministic loaders
  -> normalized architectural facts
  -> ownership graph
  -> rule engine
  -> diagnostics
```

Do not skip this by putting framework-specific checks directly into core.

## AIR facts should be boring

Prefer facts like:

```text
type exists
function exists
module imports symbol
function constructs type
function calls symbol
literal appears in branch
Result is discarded
task is spawned
comment says "as discussed"
trait has one implementation
file has many public symbols
```

Avoid facts that require subjective judgment.

## Core rules should be ownership checks

Good rule shape:

```text
Boundary type is used inside domain code.
This location lacks authority to use boundary representations.
```

Bad rule shape:

```text
This code looks messy.
```

Good rule shape:

```text
This trait has one implementation and no accepted port role.
```

Bad rule shape:

```text
This abstraction feels unnecessary.
```

## Do not hardcode framework rules in core

Bad:

```text
Bevy Update systems must not load assets.
```

Good:

```text
Blocking or unbounded work is forbidden in hot runtime contexts unless accepted.
```

A future Bevy loader may map Bevy `Update` systems to `hot_context`. The core rule should only know about `hot_context`.

## Main rule families

The paradigms document defines the long-term rule families. The most important early ones are:

```text
OT — Canonical Domain Ownership
CF — Config/Data Ownership
DA — Demand-Driven Architecture
DG — Dependency Graph / Direction
BO — Boundary Ownership
FL — Failure Lineage Ownership
MO — Module / File Ownership
CX — Complexity Budget Ownership
DC — Documentation / Comment Ownership
RW — Runtime Work Ownership
```

Do not try to implement every family at once.

## Early implementation priority

Start with high-confidence, deterministic checks:

1. Rust adapter emits AIR for types, fields, functions, imports, literals, comments, conversions, and failure patterns.
2. Detect shadow model candidates from field/name/path overlap.
3. Detect boundary leaks from imports/signatures/path roles.
4. Detect unregistered conversions and direct canonical construction.
5. Detect hardcoded decision data candidates.
6. Detect obvious failure erasure: discarded `Result`, `.ok()`, `unwrap_or_default`, lossy `map_err(|_| ...)`.
7. Detect context-locked comments.
8. Detect speculative abstractions: one-impl trait, one-entry registry, pass-through layer.

## Rust-first but not Rust-only

Rust is the first adapter. Do not let Rust-specific implementation leak into the core model.

Core should not know about `syn`, `cargo metadata`, `Result`, `Option`, or `tokio::spawn` directly.

The Rust adapter may emit facts such as:

```text
fallible_result_discarded
result_collapsed_to_option
spawned_work
trait_impl_count
struct_literal_constructs_type
```

Core rules consume the normalized facts.

## Comments and docs

Do not add context-locked comments.

Bad:

```rust
// Handle the edge case mentioned earlier
```

Good:

```rust
// Empty manifests are valid during plugin discovery.
// They are rejected later when the manifest kind is resolved.
```

Comments must make sense from repository context alone.

## Failure handling

Do not silence errors to make code compile.

Avoid adding:

```rust
let _ = operation();
operation().ok();
unwrap_or_default();
.map_err(|_| Error::Failed)?;
```

unless the failure has an accepted owner or explicit reason.

Locus itself should model failure lineage carefully. A tool that detects hidden failures must not hide its own.

## YAGNI / demand-driven architecture

Do not add speculative systems before there is demand.

Avoid:

- trait with one implementation and no accepted port role,
- factory for one product,
- registry with one entry,
- builder for trivial type,
- config option with one possible value,
- extension hook with no consumer,
- pass-through manager/service layer.

If an abstraction is needed, document the architectural rent it pays.

## Complexity and god modules

Do not keep adding to the nearest large file.

If a module accumulates multiple roles, split or emit facts that allow Locus to detect it.

Locus should measure responsibility entropy, not just line count.

## Diagnostics

Diagnostics should be precise and directive.

Good diagnostic structure:

```text
error[OT002]: undeclared concept-shaped type

Symbol:
  crate::api::v1::UserModel

Looks like:
  identity.user boundary adapter

Accepted canonical:
  crate::domain::identity::User

Why:
  field overlap 91%
  suffix Model
  api path

Fix:
  remove it, use User, or accept it as a boundary adapter.
```

Do not emit vague diagnostics.

## Final instruction

When changing Locus, preserve this architecture:

> deterministic facts first, accepted ownership second, rule diagnostics third.

Do not turn Locus into the kind of architecture drift it is meant to prevent.
