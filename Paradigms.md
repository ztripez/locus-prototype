# Locus Architectural Paradigms

## Purpose

This document defines the architectural paradigms Locus is intended to guard.

Locus exists because LLM coding agents are often strong at architecture planning but weak at architecture-preserving implementation. They can describe a sound architecture, but when asked to modify code they tend to optimize for local completion: add the type, add the mapper, add the branch, add the helper, add the constant, make the test pass.

That behavior is useful at the junior implementation level, but dangerous at the senior architectural level.

Locus turns architectural intent into enforceable local constraints.

The core question is:

> Does this code have architectural authority to do what it is doing here?

If not, Locus should block it or warn with a precise explanation and an approved path.

---

## Core Thesis

LLMs are good at producing architecture plans.

LLMs are bad at preserving architecture while implementing changes.

They commonly:

* create parallel models instead of using canonical domain concepts,
* hardcode decision data instead of using config/data ownership,
* bypass ports and adapters,
* collapse boundaries,
* put logic in convenient nearby files,
* create generic helpers that become hidden domain owners,
* mix policy, orchestration, IO, validation, and mapping,
* introduce new abstractions without architectural need,
* scatter construction, configuration, transactions, and side effects.

Locus should act as the missing senior-engineering guardrail.

It should not judge code by style. It should judge whether code violates accepted ownership.

---

## Architectural Authority

Most Locus paradigms are variations of one principle:

> A code location may only define, decide, construct, convert, validate, persist, call, configure, or emit things it has authority to own.

Examples:

* A boundary adapter has authority to represent an external protocol shape.
* A canonical domain type has authority to represent a domain concept.
* A converter has authority to transform between a boundary shape and a canonical type.
* A config source has authority to own behavior-shaping values.
* A composition root has authority to wire concrete implementations.
* A repository has authority to persist state, but not to own domain policy.
* A handler has authority to translate transport concerns, but not to own business rules.
* A port has authority to define what the application needs from infrastructure.
* An adapter has authority to implement a port, but not to leak inward.

Locus should detect when code performs an action without the corresponding authority.

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

* parallel canonical types,
* shadow DTOs/models/entities,
* boundary types entering domain/application logic,
* direct canonical construction outside owner/converter,
* unregistered conversions,
* adapter-to-adapter conversion,
* domain logic on boundary adapters,
* scattered validation,
* shadow enums,
* shadow newtypes,
* primitive substitutes for known value objects.

### Example violation

```rust
pub struct UserModel {
    pub id: String,
    pub email: String,
}

fn map_user(model: UserModel) -> User {
    User {
        id: UserId::parse(model.id).unwrap(),
        email: EmailAddress::parse(model.email).unwrap(),
    }
}
```

### Preferred shape

```text
UserModel is either removed, or explicitly accepted as a boundary adapter.
The mapping is either removed, or explicitly accepted as the converter.
```

### Rule family

```text
OT — One Truth / Canonical Domain Ownership
```

---

## Paradigm 2: Config/Data Ownership

### Problem

LLM agents hardcode behavior-shaping values instead of making systems data-driven.

Examples:

* provider IDs,
* model names,
* retry counts,
* timeout values,
* role/permission logic,
* status transitions,
* tier limits,
* scoring weights,
* region-specific behavior,
* feature flag semantics,
* queue/topic names,
* route mappings,
* local lookup tables.

### Invariant

> Behavior-shaping decision data must have one accepted owner. Code may execute decisions, but must not secretly own decision data.

### Locus should reject or warn on

* magic decision constants,
* hardcoded provider/model/topic IDs,
* inline policy branches,
* inline lookup tables,
* environment-specific branching outside the config layer,
* scattered feature flag semantics,
* hardcoded state transitions,
* duplicate decision tables,
* unregistered config-like files,
* code-owned constants without accepted ownership.

### Example violation

```rust
fn select_provider(region: &str, tier: &str) -> &'static str {
    if region == "eu" && tier == "enterprise" {
        "anthropic"
    } else {
        "openai"
    }
}
```

### Preferred shape

```yaml
# config/provider_selection.yaml
rules:
  - region: eu
    tier: enterprise
    provider: anthropic
  - default:
    provider: openai
```

```rust
provider_selection.select(context)
```

### Important nuance

Locus must not ban all literals.

It should distinguish:

* harmless local algorithmic constants,
* stable code-owned constants,
* generated constants from accepted config,
* behavior-shaping policy/config values.

The target is hidden decision ownership, not literal usage itself.

### Rule family

```text
CF — Config/Data Ownership
```

---

## Paradigm 3: Dependency Direction Ownership

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

* forbidden imports,
* new dependency cycles,
* lower layers importing upper layers,
* shared/common modules importing feature-specific modules,
* feature internals being imported across feature boundaries,
* boundary modules becoming implicit shared modules.

### Example violation

```rust
// inside src/domain/user.rs
use crate::api::v1::UserDto;
```

### Preferred shape

```text
api.v1 converts UserDto into canonical User before crossing into domain/application.
```

### Rule family

```text
DG — Dependency Graph / Direction
```

---

## Paradigm 4: Boundary Ownership

### Problem

LLM agents collapse boundaries. Transport, persistence, serialization, generated protocol shapes, and external service types leak inward.

### Invariant

> Boundary concerns stay at the boundary. Domain/application logic consumes canonical concepts or ports, not protocol or infrastructure shapes.

### Locus should reject

* HTTP DTOs in domain/application signatures,
* database rows treated as domain objects,
* generated protobuf/GraphQL/OpenAPI types leaking inward,
* domain types depending on transport or serialization framework details,
* persistence concerns inside domain logic,
* transport status codes in domain errors.

### Example violation

```rust
pub fn create_user(req: CreateUserRequest) -> Result<User, Error> {
    ...
}
```

inside application/domain code.

### Preferred shape

```rust
let command = CreateUserCommand::try_from(req)?;
application.create_user(command).await?;
```

or direct conversion to the accepted canonical type if that is the project's pattern.

### Rule family

```text
BO — Boundary Ownership
```

---

## Paradigm 5: Port/Adapter Ownership

### Problem

LLM agents bypass ports and use concrete infrastructure directly.

Examples:

```rust
let repo = SqlUserRepository::new(pool);
repo.save(user).await?;
```

inside application logic.

Or:

```rust
let client = reqwest::Client::new();
client.post(...).send().await?;
```

inside domain/application code.

### Invariant

> Application code depends on ports. Infrastructure adapters implement ports. Concrete adapter construction belongs in the composition root.

### Locus should reject

* concrete infrastructure imports in application/domain,
* direct external service calls without a declared port,
* adapter construction outside composition root,
* adapter-to-adapter calls that bypass application orchestration,
* feature modules reaching through another feature's adapter.

### Preferred shape

```rust
trait UserRepository {
    async fn save(&self, user: User) -> Result<()>;
}
```

Application code uses the port. Infrastructure provides the implementation.

### Rule family

```text
PA — Port/Adapter Ownership
```

---

## Paradigm 6: Composition Root Ownership

### Problem

LLM agents scatter runtime wiring and object construction through the codebase.

Examples:

* config loaded in random modules,
* clients constructed inside handlers,
* repositories constructed inside services,
* service graphs assembled in tests or jobs,
* global singletons introduced locally,
* environment variables read outside config loading.

### Invariant

> Runtime wiring, concrete construction, config loading, and dependency assembly belong to accepted composition roots.

### Locus should reject

* infrastructure object construction outside composition root,
* service graph wiring outside composition root,
* config loading outside config layer,
* environment reads outside config layer,
* runtime singletons introduced outside owner,
* dependency injection bypasses.

### Example violation

```rust
let api_key = std::env::var("OPENAI_API_KEY")?;
let client = OpenAiClient::new(api_key);
```

inside a handler or application service.

### Preferred shape

```rust
let provider = ProviderClient::new(config.providers.openai);
```

inside the composition root, then injected through a port.

### Rule family

```text
CR — Composition Root Ownership
```

---

## Paradigm 7: Responsibility Ownership

### Problem

LLM agents often put many responsibilities into one convenient function.

Example:

```text
handler:
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

* handlers containing domain policy,
* converters performing side effects,
* repositories containing business rules,
* validators performing IO,
* domain types doing persistence,
* application services doing boundary serialization,
* functions mixing mapping, policy, persistence, and external IO.

### Example violation

```rust
impl TryFrom<CreateUserRequest> for User {
    fn try_from(req: CreateUserRequest) -> Result<User> {
        audit_log.write("creating user")?;
        ...
    }
}
```

### Preferred shape

Converters convert and validate. Side effects happen in application orchestration or dedicated side-effect owners.

### Rule family

```text
RM — Responsibility Mixing
```

---

## Paradigm 8: Utility/Shared Module Discipline

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

* new generic utility modules without acceptance,
* domain concept logic inside utility modules,
* validation inside utility modules,
* mapping/conversion inside utility modules,
* utility modules importing feature-specific concepts,
* helpers that know about roles, status, users, providers, policies, or tiers.

### Example violation

```rust
pub fn is_admin(role: &str) -> bool {
    role == "admin" || role == "owner"
}
```

inside `utils.rs`.

### Preferred shape

Role policy belongs to the accepted role/permission policy owner.

### Rule family

```text
UT — Utility / Shared Module Discipline
```

---

## Paradigm 9: Error Taxonomy Ownership

### Problem

LLM agents invent local error types, string errors, catch-all errors, or transport-aware domain errors.

Examples:

```text
UserError
CreateUserError
UserServiceError
ApiUserError
ValidationError
AppError
DomainError
```

without a coherent taxonomy.

### Invariant

> Error types and error conversions must follow the accepted error taxonomy and boundary mapping path.

### Locus should reject

* new overlapping error types,
* boundary errors in domain,
* HTTP status codes in domain errors,
* string errors where typed errors exist,
* catch-all errors hiding domain errors,
* unregistered error conversions,
* duplicated validation error variants.

### Example violation

```rust
pub enum UserError {
    NotFound(StatusCode),
}
```

inside domain.

### Preferred shape

```rust
pub enum UserError {
    NotFound,
}
```

Boundary maps `UserError::NotFound` to HTTP 404.

### Rule family

```text
ER — Error Taxonomy Ownership
```

---

## Paradigm 10: Feature Ownership

### Problem

LLM agents add code to the nearest feature or import another feature's internals because it solves the local task.

### Invariant

> Feature modules own their internals. Cross-feature interaction must go through accepted APIs, ports, events, or application services.

### Locus should reject

* feature internals imported by another feature,
* one feature writing another feature's state directly,
* cross-feature calls bypassing declared APIs/ports,
* shared types introduced to avoid proper feature boundaries,
* a feature defining a concept owned by another feature.

### Example violation

```rust
// billing module
use crate::identity::internal::UserRecord;
```

### Preferred shape

```rust
identity_port.get_user_identity(user_id).await?;
```

or an accepted domain event/query interface.

### Rule family

```text
FO — Feature Ownership
```

---

## Paradigm 11: Abstraction Discipline

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

* new traits/interfaces with one implementation and no boundary role,
* manager/handler/processor abstractions without accepted responsibility,
* abstractions duplicating existing ports,
* generic service layers hiding domain concepts,
* base/common types shared across unrelated concepts.

### Example violation

```rust
trait UserManager {
    fn create_user(...);
}
```

when an accepted `CreateUser` application service or `UserRepository` port already exists.

### Preferred shape

Use the existing architectural role, or explicitly accept the new abstraction as a port, service, policy, or adapter.

### Rule family

```text
AB — Abstraction Discipline
```

---

## Paradigm 12: Runtime Boundary Ownership

### Problem

LLM agents are weak at runtime architecture. They add tasks, locks, channels, transactions, retries, and side effects locally.

### Invariant

> Runtime mechanisms that affect concurrency, transactions, side effects, retries, or event flow must be owned by accepted runtime boundaries.

### Locus should reject or warn on

* tasks spawned outside accepted runtime/orchestration owners,
* blocking calls inside async paths,
* shared mutable state introduced without ownership,
* mutexes/locks added around domain state without a concurrency policy,
* channels/topics introduced without ownership,
* transactions started in the wrong layer,
* external IO inside transactions,
* event publishing outside event owner,
* DB write plus message publish without accepted outbox/transaction policy,
* side effects inside converters or validators.

### Example violation

```rust
tokio::spawn(async move {
    send_email(user).await;
});
```

inside a random handler.

### Preferred shape

Use an accepted job/event/outbox/side-effect mechanism.

### Rule families

```text
AC — Async / Concurrency Ownership
TX — Transaction Boundary Ownership
SE — Side Effect / Event Ownership
```

---

## Paradigm 13: Observability Ownership

### Problem

LLM agents add ad hoc logs, metrics, and events while patching.

Examples:

```rust
println!("created user");
dbg!(&value);
tracing::info!("payment done");
metrics::counter!("stuff_count").increment(1);
```

This creates noisy, inconsistent, or unsafe observability.

### Invariant

> Logs, metrics, events, and audit records that represent system behavior must use accepted names, fields, and redaction paths.

### Locus should reject or warn on

* raw print/debug statements in non-test code,
* unregistered metric names,
* unregistered event names,
* missing required correlation/context fields,
* logging sensitive concepts outside approved redaction paths,
* duplicate events emitted from multiple owners,
* audit events emitted outside audit owner.

### Rule family

```text
OB — Observability Ownership
```

---

## Paradigm 14: Test Architecture Ownership

### Problem

LLM agents often hide architecture violations inside tests.

Examples:

* local test-only DTOs,
* duplicated fixtures,
* fake implementations outside accepted test adapter locations,
* inline JSON blobs that become shadow schemas,
* tests asserting unstable implementation details,
* bypassing canonical builders.

### Invariant

> Tests may use test-specific adapters and fixtures, but they must not create new domain truth.

### Locus should reject or warn on

* test shadow models,
* test fixtures duplicating domain concepts,
* test fakes implementing ports outside accepted test adapter modules,
* inline fixture data that should be fixture-owned,
* tests bypassing canonical builders,
* tests asserting private implementation details when public behavior should be tested.

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
DG — Dependency Graph / Direction
BO — Boundary Ownership
PA — Port/Adapter Ownership
CR — Composition Root Ownership
RM — Responsibility Mixing
UT — Utility / Shared Module Discipline
ER — Error Taxonomy Ownership
FO — Feature Ownership
AB — Abstraction Discipline
AC — Async / Concurrency Ownership
TX — Transaction Boundary Ownership
SE — Side Effect / Event Ownership
OB — Observability Ownership
TA — Test Architecture Ownership
```

Not all families need to be implemented immediately. They define the long-term architecture of Locus as an architectural guardrail system.

---

## Recommended Implementation Order

### Phase 1: Canonical Domain Ownership

Implement first because it is the original Locus problem and has strong static signals.

Prioritize:

* shadow models,
* boundary leaks,
* converter bypasses,
* direct canonical construction,
* scattered validation.

### Phase 2: Dependency Direction and Boundary Ownership

High value and relatively deterministic.

Prioritize:

* forbidden imports,
* wrong layer dependencies,
* infrastructure imports in domain/application,
* boundary types crossing inward.

### Phase 3: Config/Data Ownership

Catch LLM hardcoding drift.

Prioritize:

* hardcoded provider/model/topic strings,
* environment reads outside config layer,
* inline lookup tables,
* magic threshold constants,
* role/status/tier policy branches.

### Phase 4: Port/Adapter and Composition Root Ownership

Prevent infrastructure bypass.

Prioritize:

* concrete adapter imports in application/domain,
* construction outside composition root,
* direct external service calls,
* config loading outside config owner.

### Phase 5: Responsibility Mixing

Use heuristic detection, especially in agent strict mode.

Prioritize:

* side effects in converters,
* policy in handlers,
* business rules in repositories,
* IO in validators.

### Phase 6: Runtime, Observability, and Test Architecture

Add once the core ownership graph is stable.

---

## Agent Strict Mode

LLM agents should be held to stricter architectural rules than humans.

Reason:

* agents are more likely to create plausible architectural rot,
* agents optimize for local task completion,
* agents do not reliably preserve implicit project constraints,
* agents often invent new shapes instead of discovering existing owners.

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
```

---

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
```

The goal is not only to reject bad patches, but to tell the agent where the correct implementation belongs.

---

## Design Principle: Do Not Encode Taste

Locus should avoid subjective style rules.

It should not care whether the project uses:

* clean architecture,
* hexagonal architecture,
* vertical slices,
* DDD,
* layered architecture,
* functional core / imperative shell,
* ECS-style architecture,
* plugin architecture,
* generated clients,
* serde directly on domain types,
* explicit DTOs.

Locus cares about accepted ownership.

A project may choose different architectural patterns, but once chosen, code must not silently violate them.

---

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
```

The core interprets facts against accepted ownership:

```text
this type is canonical
this type is a boundary adapter
this path is a boundary
this module owns provider selection
this file is a config source
this function is an accepted converter
```

This keeps language adapters simple and the rule engine coherent.

---

## Design Principle: Inference First, Acceptance Second

Locus should infer likely architectural roles, but accepted ownership must be explicit in the lockfile or source hints.

Inference produces candidates.

Acceptance creates authority.

This prevents both extremes:

* no giant hand-written architecture config,
* no purely heuristic architecture enforcement.

---

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

---

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
new side effect inside converter
new transaction boundary in wrong layer
new feature-internal import across boundary
```

Locus does not replace architecture planning.

It converts architecture plans into enforceable implementation guardrails.

That is the point of the tool.
