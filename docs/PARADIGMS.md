# Locus Architectural Paradigms

## Purpose

This document defines the architectural paradigms Locus is intended to guard.

Locus exists because LLM coding agents are often strong at architecture planning but weak at architecture-preserving implementation. They can describe a sound architecture, but when asked to modify code they tend to optimize for local completion: add the type, add the mapper, add the branch, add the helper, add the constant, make the test pass.

That behavior is useful at the junior implementation level, but dangerous at the senior architectural level.

Locus turns architectural intent into enforceable local constraints.

The core question is:

> Does this code have architectural authority to do what it is doing here?

If not, Locus should block it or warn with a precise explanation and an approved path.

## Determinism Requirement

Locus must never rely on an LLM agent to find or classify blocking issues.

Blocking findings must come from deterministic source facts and accepted ownership metadata:

- AST facts,
- symbol facts,
- import graphs,
- call graphs,
- literal and branch analysis,
- module path classification,
- lockfile ownership,
- source hints,
- framework loader mappings,
- complexity metrics,
- error-handling patterns.

An LLM may consume Locus diagnostics. Locus must not depend on LLM guesses.

Optional advisory modes may exist later, but `locus check` must be deterministic.

## Architectural Authority

Most Locus paradigms are variations of one principle:

> A code location may only define, decide, construct, convert, validate, persist, call, configure, spawn, document, or emit things it has authority to own.

Examples:

- A boundary adapter has authority to represent an external protocol shape.
- A canonical domain type has authority to represent a domain concept.
- A converter has authority to transform between a boundary shape and a canonical type.
- A config source has authority to own behavior-shaping values.
- A composition root has authority to wire concrete implementations.
- A repository has authority to persist state, but not to own domain policy.
- A handler has authority to translate transport concerns, but not to own business rules.
- A port has authority to define what the application needs from infrastructure.
- An adapter has authority to implement a port, but not to leak inward.
- A runtime owner has authority to spawn work, block, schedule, mutate shared state, or own concurrency policy.

Locus should detect when code performs an action without the corresponding authority.

## Framework Knowledge and Sub-Paradigm Loaders

Core Locus should not know framework-specific opinions.

Framework-specific knowledge must enter through deterministic sub-paradigm loaders that emit normalized architectural facts.

Examples of normalized facts:

```text
hot_path
request_context
background_worker
blocking_call
spawned_work
persistence_write
external_io
boundary_entry
config_read
runtime_state_owner
```

A future Bevy loader may determine that a function registered in `Update` is a hot path. A web framework loader may determine that a function is a request handler. A Tokio loader may classify `tokio::spawn` as spawned work and `std::fs::read_to_string` as blocking IO.

The core rule should not be `Bevy Update systems must not do blocking work`.

The core rule should be:

```text
blocking work is forbidden in hot or non-blocking runtime contexts unless routed through an accepted owner
```

This keeps Locus deterministic, extensible, and architecture-focused.

---

## Paradigm 1: Canonical Domain Ownership

### Problem

LLM agents frequently create parallel representations of the same domain concept.

Examples:

```text
User
UserDto
UserModel
UserRecord
UserEntity
UserResponse
InternalUser
ValidatedUser
```

They also create local mapping, validation, and enum/state logic rather than reusing the canonical domain path.

### Invariant

> Every domain concept has one accepted canonical representation. Every other shape is boundary-only and must convert through accepted converters.

### Locus should reject

- parallel canonical types,
- shadow DTOs/models/entities,
- boundary types entering domain/application logic,
- direct canonical construction outside owner/converter,
- unregistered conversions,
- adapter-to-adapter conversion,
- domain logic on boundary adapters,
- scattered validation,
- shadow enums,
- shadow newtypes,
- primitive substitutes for known value objects.

### Rule family

```text
OT — One Truth / Canonical Domain Ownership
```

---

## Paradigm 2: Config/Data Ownership

### Problem

LLM agents hardcode behavior-shaping values instead of making systems data-driven.

Examples:

- provider IDs,
- model names,
- retry counts,
- timeout values,
- role/permission logic,
- status transitions,
- tier limits,
- scoring weights,
- region-specific behavior,
- feature flag semantics,
- queue/topic names,
- route mappings,
- local lookup tables.

### Invariant

> Behavior-shaping decision data must have one accepted owner. Code may execute decisions, but must not secretly own decision data.

### Locus should reject or warn on

- magic decision constants,
- hardcoded provider/model/topic IDs,
- inline policy branches,
- inline lookup tables,
- environment-specific branching outside the config layer,
- scattered feature flag semantics,
- hardcoded state transitions,
- duplicate decision tables,
- unregistered config-like files,
- code-owned constants without accepted ownership.

### Important nuance

Locus must not ban all literals.

It should distinguish:

- harmless local algorithmic constants,
- stable code-owned constants,
- generated constants from accepted config,
- behavior-shaping policy/config values.

The target is hidden decision ownership, not literal usage itself.

### Rule family

```text
CF — Config/Data Ownership
```

---

## Paradigm 3: Demand-Driven Architecture

### Problem

LLM agents frequently implement hypothetical future flexibility instead of the smallest architecture demanded by the current system. They create traits, factories, registries, hooks, strategies, managers, generic layers, and config knobs before there is real variation.

This is the enforceable form of YAGNI.

### Invariant

> Architectural surface area must be justified by present demand or explicitly accepted future variation.

A new abstraction represents variation. If there is no accepted variation owner, second implementation, external boundary, current consumer, or architectural role, the abstraction is speculative.

### Locus should reject or warn on

- traits/interfaces with one implementation and no accepted port role,
- factories that construct one concrete type,
- registries with one entry,
- builders for trivial structs,
- generic abstractions with one concrete instantiation,
- config options with one valid value,
- extension hooks with no consumers,
- event buses with one local subscriber,
- pass-through service/manager/processor layers,
- single-variant strategy enums,
- abstractions duplicating existing ownership paths.

### Valid abstraction rent

A new abstraction may be justified when it:

- crosses a real boundary,
- has multiple implementations,
- owns policy,
- centralizes construction,
- hides external infrastructure,
- isolates volatility,
- encodes an invariant,
- supports test substitution through an accepted port,
- represents an accepted extension point,
- is generated from an external protocol.

### Rule family

```text
DA — Demand-Driven Architecture
```

---

## Paradigm 4: Dependency Direction Ownership

### Problem

LLM agents often solve the local task by importing whatever is convenient, even when it points against the architecture.

Examples:

```text
domain -> api
domain -> infrastructure
core -> feature module
shared -> domain-specific module
billing -> identity internals
identity -> billing internals
```

### Invariant

> Dependencies must follow the accepted architecture direction graph.

### Locus should reject

- forbidden imports,
- new dependency cycles,
- lower layers importing upper layers,
- shared/common modules importing feature-specific modules,
- feature internals being imported across feature boundaries,
- boundary modules becoming implicit shared modules.

### Rule family

```text
DG — Dependency Graph / Direction
```

---

## Paradigm 5: Boundary Ownership

### Problem

LLM agents collapse boundaries. Transport, persistence, serialization, generated protocol shapes, and external service types leak inward.

### Invariant

> Boundary concerns stay at the boundary. Domain/application logic consumes canonical concepts or ports, not protocol or infrastructure shapes.

### Locus should reject

- HTTP DTOs in domain/application signatures,
- database rows treated as domain objects,
- generated protocol types leaking inward,
- domain types depending on transport or serialization framework details,
- persistence concerns inside domain logic,
- transport status codes in domain errors.

### Rule family

```text
BO — Boundary Ownership
```

---

## Paradigm 6: Port/Adapter Ownership

### Problem

LLM agents bypass ports and use concrete infrastructure directly.

### Invariant

> Application code depends on ports. Infrastructure adapters implement ports. Concrete adapter construction belongs in the composition root.

### Locus should reject

- concrete infrastructure imports in application/domain,
- direct external service calls without a declared port,
- adapter construction outside composition root,
- adapter-to-adapter calls that bypass application orchestration,
- feature modules reaching through another feature's adapter.

### Rule family

```text
PA — Port/Adapter Ownership
```

---

## Paradigm 7: Composition Root Ownership

### Problem

LLM agents scatter runtime wiring and object construction through the codebase.

Examples:

- config loaded in random modules,
- clients constructed inside handlers,
- repositories constructed inside services,
- service graphs assembled in tests or jobs,
- global singletons introduced locally,
- environment variables read outside config loading.

### Invariant

> Runtime wiring, concrete construction, config loading, and dependency assembly belong to accepted composition roots.

### Locus should reject

- infrastructure object construction outside composition root,
- service graph wiring outside composition root,
- config loading outside config layer,
- environment reads outside config layer,
- runtime singletons introduced outside owner,
- dependency injection bypasses.

### Rule family

```text
CR — Composition Root Ownership
```

---

## Paradigm 8: Responsibility Ownership

### Problem

LLM agents often put many responsibilities into one convenient function.

Example responsibilities mixed in one handler:

```text
parse request
validate domain rules
map DTO
check permissions
write database
call external service
send email
emit event
build response
```

The result works locally but destroys architectural separation.

### Invariant

> Code should perform only the responsibilities its layer owns.

### Locus should reject or warn on

- handlers containing domain policy,
- converters performing side effects,
- repositories containing business rules,
- validators performing IO,
- domain types doing persistence,
- application services doing boundary serialization,
- functions mixing mapping, policy, persistence, and external IO.

### Rule family

```text
RM — Responsibility Mixing
```

---

## Paradigm 9: Module / File Ownership

### Problem

LLM agents keep adding code to existing large files because they optimize for local completion. Over time, files become god modules that own multiple concepts, boundaries, policies, adapters, constants, and orchestration paths.

A god module is a missing ownership split.

### Invariant

> A module/file should have one coherent architectural responsibility. When it accumulates unrelated ownership roles, it must split into accepted submodules.

### Locus should reject or warn on

- files with many unrelated architectural roles,
- canonical domain types co-located with boundary/persistence adapters,
- handlers co-located with domain logic,
- repositories co-located with policy,
- config data co-located with execution logic,
- new code added to already overloaded modules,
- modules containing many independent concepts with behavior,
- large files that become ownership sinks.

### Detection focus

Line count alone is weak. Locus should prefer responsibility entropy:

```text
domain type + boundary adapter + converter + repository + handler + config data + policy branch + side effect
```

A large generated table may be fine. A smaller file containing six architectural roles is not.

### Rule family

```text
MO — Module / File Ownership
```

---

## Paradigm 10: Complexity Budget Ownership

### Problem

LLM agents often implement locally correct code by increasing complexity in the nearest file or function. They add branches, parameters, helper layers, imports, side effects, and responsibilities without regard for the complexity budget of that architectural role.

This is the enforceable form of KISS.

### Invariant

> Complexity must be owned by the right abstraction. A module, function, or symbol may only carry the complexity appropriate to its accepted role.

### Locus should reject or warn on

- functions exceeding complexity budget for their role,
- modules with high responsibility entropy,
- handlers with policy/orchestration complexity,
- converters with side effects or excessive branching,
- repositories with business decision complexity,
- utility modules with high fan-in and domain knowledge,
- symbols touching too many concepts without being accepted orchestrators,
- excessive public surface area,
- high fan-out outside composition/orchestration owners,
- changed code that increases complexity in already overloaded modules.

### Important nuance

Complexity is not inherently bad. A parser, solver, query planner, state machine, or protocol implementation may be complex for good reasons. A DTO mapper, handler, config loader, or converter usually should not be.

The budget depends on role.

### Rule family

```text
CX — Complexity Budget Ownership
```

---

## Paradigm 11: Utility / Shared Module Discipline

### Problem

LLM agents create generic helpers and shared modules as dumping grounds.

Examples:

```text
utils.rs
helpers.rs
common.rs
shared.rs
misc.rs
```

These often become hidden owners of domain behavior.

### Invariant

> Shared utility modules may only own domain-free technical helpers. Domain-aware behavior belongs to the relevant concept, feature, policy, or adapter owner.

### Locus should reject

- new generic utility modules without acceptance,
- domain concept logic inside utility modules,
- validation inside utility modules,
- mapping/conversion inside utility modules,
- utility modules importing feature-specific concepts,
- helpers that know about roles, status, users, providers, policies, or tiers.

### Rule family

```text
UT — Utility / Shared Module Discipline
```

---

## Paradigm 12: Failure Lineage Ownership

### Problem

LLM agents often make Rust code compile by discarding, collapsing, defaulting, logging, or stringifying failures. This creates silent errors, masked failures, partial state commits, and sinks of invalid or unwanted state.

### Invariant

> Every failure must be handled, propagated with context, converted through an accepted error boundary, routed to an accepted failure sink, or explicitly acknowledged as intentionally ignored.

### Locus should reject or warn on

- discarded `Result`s,
- `.ok()` conversions that erase failure,
- `unwrap_or_default` masking failed config/parse/load operations,
- `map_err(|_| ...)` losing source context,
- catch-all `Err(_)` branches,
- spawned task failures with no sink,
- logging an error and continuing as success for required operations,
- panics/unwraps outside invariant owners or tests,
- failed initialization leaving registered state,
- invalid input converted into valid default state,
- unknown/default enum variants acting as failure sinks,
- retry loops without accepted retry policy,
- lossy error stringification outside presentation boundaries.

### Important nuance

The rule is not `always propagate every error`.

The rule is:

> Every failure must have an owner.

Failure owners can be callers, domain errors, boundary mappers, retry policies, best-effort sinks, supervisors, outboxes, transactions, compensation handlers, observability paths, or explicit discard policies.

### Rule family

```text
FL — Failure Lineage Ownership
```

---

## Paradigm 13: Error Taxonomy Ownership

### Problem

LLM agents invent local error types, string errors, catch-all errors, or transport-aware domain errors.

### Invariant

> Error types and error conversions must follow the accepted error taxonomy and boundary mapping path.

### Locus should reject

- new overlapping error types,
- boundary errors in domain,
- HTTP status codes in domain errors,
- string errors where typed errors exist,
- catch-all errors hiding domain errors,
- unregistered error conversions,
- duplicated validation error variants.

### Rule family

```text
ER — Error Taxonomy Ownership
```

---

## Paradigm 14: Runtime Work Ownership

### Problem

LLM agents are weak at runtime architecture. They add tasks, locks, channels, blocking calls, retries, and shared state locally. They often ignore async boundaries, scheduling, request contexts, hot paths, frame budgets, cancellation, and concurrency ownership.

This should remain framework-neutral in core Locus. Framework-specific runtime models belong in deterministic sub-paradigm loaders.

### Invariant

> Runtime work, runtime state, runtime failure, and runtime blocking must have accepted owners and budgets.

### Locus should reject or warn on

- blocking operations in async, request, or hot contexts,
- untracked background tasks,
- locks held across suspension points,
- shared mutable state without ownership,
- global mutable singleton state,
- unbounded work in request/hot contexts,
- runtime object construction in hot paths,
- unbounded task spawning,
- CPU-heavy work in non-blocking contexts,
- clone-to-avoid-lifetime state forks,
- runtime failure with no failure sink.

### Deterministic design

Core Locus should enforce normalized runtime facts such as:

```text
hot_context
non_blocking_context
spawned_work
blocking_call
shared_mutable_state
lock_across_await
unbounded_work
```

Framework loaders may map framework-specific syntax into these facts. Core rules must not depend on framework-specific opinions.

### Rule family

```text
RW — Runtime Work Ownership
```

---

## Paradigm 15: Feature Ownership

### Problem

LLM agents add code to the nearest feature or import another feature's internals because it solves the local task.

### Invariant

> Feature modules own their internals. Cross-feature interaction must go through accepted APIs, ports, events, or application services.

### Locus should reject

- feature internals imported by another feature,
- one feature writing another feature's state directly,
- cross-feature calls bypassing declared APIs/ports,
- shared types introduced to avoid proper feature boundaries,
- a feature defining a concept owned by another feature.

### Rule family

```text
FO — Feature Ownership
```

---

## Paradigm 16: Abstraction Discipline

### Problem

LLM agents often create abstractions to sound architectural rather than because the architecture needs them.

Examples:

```text
UserManager
ProviderService
DataHandler
ConfigProvider
AbstractProcessor
GenericMapper
```

Often these have one implementation or duplicate an existing port.

### Invariant

> New abstractions must have an accepted architectural role.

### Locus should reject or warn on

- new traits/interfaces with one implementation and no boundary role,
- manager/handler/processor abstractions without accepted responsibility,
- abstractions duplicating existing ports,
- generic service layers hiding domain concepts,
- base/common types shared across unrelated concepts.

### Rule family

```text
AB — Abstraction Discipline
```

---

## Paradigm 17: Documentation / Comment Ownership

### Problem

LLM agents often write comments for the conversation, not for the codebase.

These comments are context-locked: they only make sense if the reader saw the prompt, the previous bug, or the agent's reasoning. They look like documentation, but they preserve transient chat context instead of durable project context.

### Invariant

> Comments must be understandable from repository context alone.

A good comment should explain one of:

- why this code exists,
- what invariant it preserves,
- what external constraint forced it,
- what non-obvious tradeoff was chosen,
- what future removal condition exists.

A comment should not depend on:

- chat history,
- prompt history,
- agent reasoning,
- what was just changed,
- what was discussed earlier,
- unstated bug context.

### Locus should reject or warn on

- context-locked comments,
- conversation residue,
- vague temporal comments,
- unowned TODO/FIXME notes,
- comments that explain patch history instead of invariants,
- obvious comment noise,
- generated documentation slop.

### Detection signals

High-signal phrases:

```text
as discussed
mentioned earlier
previously
the user wanted
the prompt
this should fix
new approach
old approach
for now
temporary
later
clean this up
edge case above
because of the issue
from the previous version
```

Human mode may warn. Agent strict mode should fail high-confidence new comments matching these patterns unless they include a durable reference or explicit accepted exception.

### Rule family

```text
DC — Documentation / Comment Ownership
```

---

## Paradigm 18: Observability Ownership

### Problem

LLM agents add ad hoc logs, metrics, and events while patching.

### Invariant

> Logs, metrics, events, and audit records that represent system behavior must use accepted names, fields, and redaction paths.

### Locus should reject or warn on

- raw print/debug statements in non-test code,
- unregistered metric names,
- unregistered event names,
- missing required correlation/context fields,
- logging sensitive concepts outside approved redaction paths,
- duplicate events emitted from multiple owners,
- audit events emitted outside audit owner.

### Rule family

```text
OB — Observability Ownership
```

---

## Paradigm 19: Test Architecture Ownership

### Problem

LLM agents often hide architecture violations inside tests.

Examples:

- local test-only DTOs,
- duplicated fixtures,
- fake implementations outside accepted test adapter locations,
- inline JSON blobs that become shadow schemas,
- tests asserting unstable implementation details,
- bypassing canonical builders.

### Invariant

> Tests may use test-specific adapters and fixtures, but they must not create new domain truth.

### Locus should reject or warn on

- test shadow models,
- test fixtures duplicating domain concepts,
- test fakes implementing ports outside accepted test adapter modules,
- inline fixture data that should be fixture-owned,
- tests bypassing canonical builders,
- tests asserting private implementation details when public behavior should be tested.

### Rule family

```text
TA — Test Architecture Ownership
```

---

## Rule Family Prefixes

Suggested rule family taxonomy:

```text
OT — One Truth / Canonical Domain Ownership
CF — Config/Data Ownership
DA — Demand-Driven Architecture
DG — Dependency Graph / Direction
BO — Boundary Ownership
PA — Port/Adapter Ownership
CR — Composition Root Ownership
RM — Responsibility Mixing
MO — Module / File Ownership
CX — Complexity Budget Ownership
UT — Utility / Shared Module Discipline
FL — Failure Lineage Ownership
ER — Error Taxonomy Ownership
RW — Runtime Work Ownership
FO — Feature Ownership
AB — Abstraction Discipline
DC — Documentation / Comment Ownership
OB — Observability Ownership
TA — Test Architecture Ownership
```

Not all families need to be implemented immediately. They define the long-term shape of Locus as an architectural guardrail system.

## Recommended Implementation Order

### Phase 1: Canonical Domain Ownership

Implement first because it was the original Locus problem and has strong static signals.

Prioritize:

- shadow models,
- boundary leaks,
- converter bypasses,
- direct canonical construction,
- scattered validation.

### Phase 2: Dependency Direction and Boundary Ownership

High value and relatively deterministic.

Prioritize:

- forbidden imports,
- wrong layer dependencies,
- infrastructure imports in domain/application,
- boundary types crossing inward.

### Phase 3: Config/Data Ownership

Catch LLM hardcoding drift.

Prioritize:

- hardcoded provider/model/topic strings,
- environment reads outside config layer,
- inline lookup tables,
- magic threshold constants,
- role/status/tier policy branches.

### Phase 4: Failure Lineage Ownership

Rust-first value is high.

Prioritize:

- discarded `Result`,
- `.ok()` failure erasure,
- `unwrap_or_default` on config/parse/load,
- lossy `map_err(|_| ...)`,
- untracked spawned task failures,
- panic/unwrap outside tests or invariant owners.

### Phase 5: Demand-Driven Architecture and Complexity Budgets

Prevent speculative abstraction and code growth in agent patches.

Prioritize:

- one-impl traits,
- pass-through layers,
- one-entry registries,
- god modules,
- functions exceeding role budget,
- changed code increasing complexity in overloaded modules.

### Phase 6: Port/Adapter and Composition Root Ownership

Prevent infrastructure bypass.

Prioritize:

- concrete adapter imports in application/domain,
- construction outside composition root,
- direct external service calls,
- config loading outside config owner.

### Phase 7: Runtime Work Ownership

Keep core framework-neutral until deterministic sub-paradigm loaders exist.

Prioritize generic runtime facts:

- blocking call in async/non-blocking context,
- untracked spawned work,
- lock across await,
- global mutable state,
- unbounded work in known hot/request contexts.

## Agent Strict Mode

LLM agents should be held to stricter architectural rules than humans.

Reason:

- agents are more likely to create plausible architectural rot,
- agents optimize for local task completion,
- agents do not reliably preserve implicit project constraints,
- agents often invent new shapes instead of discovering existing owners.

Agent strict mode should fail on high-confidence new violations in changed code, even when human mode would only warn.

Examples:

```bash
locus check --changed --agent-strict
locus check --patch /tmp/agent.patch --agent-strict
```

Agent-facing diagnostics should be directive:

```text
Do not create UserModel.
Use crate::domain::identity::User.
Convert with UserDtoToUser.
Put validation in EmailAddress.
Do not import db::UserRow here.
Provider selection is owned by config/providers.yaml.
This spawned task has no owner or failure sink.
This trait has one implementation and no accepted port role.
```

## Locus as an Architectural Oracle

For humans, Locus is a CI guardrail.

For agents, Locus should be an architectural oracle.

Useful query commands:

```bash
locus explain-symbol <symbol>
locus query owner <concept>
locus query allowed-import <from> <to>
locus query can-construct <type> <location>
locus query can-convert <from> <to>
locus query config-owner <decision-area>
locus query where-to-put validation <concept>
locus query where-to-put side-effect <effect>
locus query runtime-owner <symbol>
```

The goal is not only to reject bad patches, but to tell the agent where the correct implementation belongs.

## Design Principle: Do Not Encode Taste

Locus should avoid subjective style rules.

It should not care whether the project uses:

- clean architecture,
- hexagonal architecture,
- vertical slices,
- DDD,
- layered architecture,
- functional core / imperative shell,
- ECS-style architecture,
- plugin architecture,
- generated clients,
- serde directly on domain types,
- explicit DTOs.

Locus cares about accepted ownership.

A project may choose different architectural patterns, but once chosen, code must not silently violate them.

## Design Principle: Source Facts, Accepted Ownership

Locus should separate facts from decisions.

Language adapters emit facts:

```text
this type exists
this function constructs that type
this module imports that symbol
this branch compares role to "admin"
this function reads an environment variable
this table maps tiers to limits
this result is discarded
this task is spawned without an owner
this comment references previous discussion
```

The core interprets facts against accepted ownership:

```text
this type is canonical
this type is a boundary adapter
this path is a boundary
this module owns provider selection
this file is a config source
this function is an accepted converter
this module is a runtime owner
this abstraction is an accepted port
```

This keeps language adapters simple and the rule engine coherent.

## Design Principle: Inference First, Acceptance Second

Locus should infer likely architectural roles, but accepted ownership must be explicit in the lockfile or source hints.

Inference produces candidates.

Acceptance creates authority.

This prevents both extremes:

- no giant hand-written architecture config,
- no purely heuristic architecture enforcement.

## Design Principle: No Broad Ignores

Exceptions must be specific, reasoned, and expiring.

Bad:

```text
ignore OT002
ignore src/api/**
```

Good:

```text
allow OT002 at src/api/legacy_user.rs
reason: legacy migration adapter
expires: 2026-07-01
```

Architectural debt should be visible.

## North Star

Locus should make the following classes of agent-created architectural drift hard to sneak in:

```text
new shadow model
new local mapper
new boundary leak
new hardcoded provider choice
new role policy branch
new concrete adapter call
new env read in random code
new helper in utils owning domain behavior
new error taxonomy fork
new swallowed Result
new unwrap_or_default masking config failure
new side effect inside converter
new transaction boundary in wrong layer
new unowned spawned task
new global mutable state sink
new speculative trait/factory/registry
new god module patch
new context-locked comment
```

Locus does not replace architecture planning.

It converts architecture plans into enforceable implementation guardrails.
