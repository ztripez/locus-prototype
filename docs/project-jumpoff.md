# Locus — Project Primer and Specification

## Purpose

Locus is a language-agnostic architecture verifier for enforcing canonical domain ownership.

It is not a normal linter, style checker, duplicate-code detector, or API design tool. Its job is to prevent domain truth fragmentation: parallel models, shadow DTOs, scattered conversions, duplicate validation, duplicated enum/state logic, and boundary shapes leaking into domain code.

The core invariant:

> For every domain concept, there is one accepted canonical representation. Every other representation is boundary-only, and all transformations must pass through accepted converters.

This project starts Rust-first, but the architecture must remain language-agnostic. Rust is the first language adapter, not the core design.

---

## Problem Statement

LLM coding agents repeatedly violate DRY at the architecture level. They often avoid touching existing domain code and instead create nearby alternatives:

* `User`, `UserDto`, `UserModel`, `UserRecord`, `UserResponse`, `UserEntity`
* local mapper functions instead of existing converters
* duplicate enum/state representations
* duplicate validation logic
* primitive `String`/`Uuid`/`i64` substitutes for existing value objects
* DTOs passed into domain or application services
* database rows treated as domain objects
* API version objects converted directly into other API version objects

This is not primarily textual duplication. It is authority fragmentation.

The tool must answer:

> Is this code defining, carrying, constructing, converting, validating, normalizing, or interpreting a domain concept outside the accepted canonical path?

If yes, it should fail with a precise diagnostic and a suggested canonical path.

---

## Non-Goals

Locus is not:

* a formatting tool
* a general code quality linter
* a clone detector
* a dependency graph visualizer only
* a package manager
* a schema registry
* an API versioning strategy
* a serialization format policy tool
* an LLM semantic reviewer
* a replacement for type systems
* a framework that requires runtime dependencies
* a Rust macro framework

The project should not start by implementing:

* a full Rust compiler plugin
* proc macros as the primary source of truth
* a giant rule DSL
* a hand-authored semantic architecture config
* cross-language support before the AIR model is stable
* LLM-based blocking decisions

---

## Core Concept

Locus builds a **Concept Ownership Graph** from source facts.

The graph contains:

* domain concepts
* canonical types
* boundary adapters
* converters
* owner modules
* boundaries
* functions
* usage edges
* truth actions

It then validates that all concept-related operations happen through accepted ownership paths.

Important framing:

> The tool should not say “you duplicated code.”
>
> It should say “this code lacks authority to define, interpret, validate, or convert this concept here.”

---

## Developer Experience Principles

### 1. Do not require long annotations everywhere

Bad default UX:

```rust
#[one_truth::boundary_adapter(
    concept = "identity.user",
    boundary = "http.api.v1",
    direction = "inbound"
)]
pub struct CreateUserRequest { ... }
```

This is too verbose and will create resistance.

Preferred UX:

```rust
pub struct CreateUserRequest { ... }
```

Then the tool infers the likely role and, if needed, the developer accepts it:

```bash
locus accept CreateUserRequest --as boundary --concept identity.user --boundary api.v1
```

Or, in ambiguous source code, a compact optional hint may be used:

```rust
// locus: ot boundary
pub struct CreateUserRequest { ... }
```

### 2. Do not require runtime or compile-time dependencies

Rust proc macros must not be the default approach.

Source hints should be comments, sidecar declarations, or lockfile decisions. The codebase should not need a new crate dependency merely to be checked by Locus.

### 3. Infer first, ask only when needed

The tool should infer architecture from:

* paths
* names
* type shapes
* field overlap
* conversion edges
* usage locations
* derives/attributes/tags
* existing project conventions

Developers should only intervene for ambiguous or intentional cases.

### 4. Generated lockfile, not hand-authored concept config

The accepted concept graph should live in a generated lockfile:

```text
.locus/lock.json
```

This is reviewable, diffable, and updated by commands. Developers should not maintain concept declarations by hand in a large semantic config file.

A small config file may exist for structural terrain:

```yaml
paths:
  domain:
    - src/domain/**
  application:
    - src/application/**
  boundaries:
    api.v1:
      - src/api/v1/**
    persistence:
      - src/db/**
      - src/persistence/**
generated:
  - target/**
  - src/generated/**
```

This config defines neighborhoods, not domain truth.

### 5. Make the correct path shorter

If canonical compliance is more annoying than shadow implementation, developers and agents will bypass it.

The tool should provide generators and precise fixes:

```bash
locus add adapter identity.user --boundary api.v1 --name UserDto
```

This can generate a boundary type and converter stub.

---

## Key Vocabulary

### Domain Concept

A stable semantic idea in the domain.

Examples:

* `identity.user`
* `identity.user_id`
* `identity.email_address`
* `lore.universe`
* `lore.manifest`
* `inventory.item`
* `policy.permission`

Concept IDs should be stable, but they should not need to be repeated constantly in source.

### Canonical Type

The accepted source-of-truth representation for a domain concept within a scope/runtime.

Example:

```rust
pub struct User {
    id: UserId,
    email: EmailAddress,
    display_name: DisplayName,
}
```

### Boundary Adapter

A non-canonical shape used at a boundary.

Examples:

* HTTP request/response DTO
* persistence row
* protobuf message
* CLI input shape
* CSV import row
* external service contract
* generated API model

Boundary adapters may exist, but they are quarantined. They must convert to/from canonical concepts before entering domain or application logic.

### Converter

A named, accepted transformation between canonical and boundary representations.

The default legal topology is:

```text
boundary adapter -> canonical -> boundary adapter
```

Direct adapter-to-adapter conversion is forbidden unless explicitly accepted as protocol translation.

### Owner Module

The module/package/file area with authority to define, construct, validate, normalize, and interpret a concept.

### Truth Action

A concept-related action performed by code.

Examples:

* define
* represent
* construct
* convert
* validate
* normalize
* interpret
* serialize
* persist
* compare
* format

Locus should detect these actions and check whether the location has authority to perform them.

---

## Architecture Overview

The system has two major parts:

```text
+-------------------------------+
|         locus core         |
| concept graph + rule engine    |
+-------------------------------+
       ^        ^        ^
       |        |        |
   Rust AIR  TS AIR   C# AIR
   adapter   adapter  adapter
```

The core is language-agnostic.

Language adapters scan source code and emit AIR: Architecture Intermediate Representation.

The core consumes AIR, builds the Concept Ownership Graph, compares it against the lockfile, and emits diagnostics.

---

## Architecture Intermediate Representation, AIR

Language adapters emit normalized source facts.

AIR should represent at least:

* packages/workspaces
* files
* modules/namespaces
* types
* fields
* enums and variants
* functions/methods
* constructors
* conversions
* imports/usages
* annotations/comments/source hints
* visibility/export state
* truth actions
* confidence/provenance

Example AIR type fact:

```json
{
  "kind": "type",
  "language": "rust",
  "symbol": "crate::api::v1::UserDto",
  "name": "UserDto",
  "module": "crate::api::v1",
  "file": "src/api/v1/user.rs",
  "visibility": "public",
  "fields": [
    { "name": "id", "type": "String" },
    { "name": "email", "type": "String" },
    { "name": "display_name", "type": "String" }
  ],
  "traits_or_derives": ["Serialize", "Deserialize"],
  "hints": []
}
```

Example conversion fact:

```json
{
  "kind": "conversion",
  "language": "rust",
  "from": "crate::api::v1::UserDto",
  "to": "crate::domain::identity::User",
  "mechanism": "TryFrom",
  "symbol": "impl TryFrom<UserDto> for User",
  "file": "src/api/v1/user.rs"
}
```

Example truth action:

```json
{
  "kind": "truth_action",
  "action": "construct",
  "target": "crate::domain::identity::User",
  "file": "src/api/v1/user.rs",
  "function": "create_user",
  "confidence": 0.94,
  "reasons": ["struct literal", "field mapping from UserDto"]
}
```

Every inferred fact should include confidence and reasons where practical.

---

## Lockfile

`.locus/lock.json` records accepted architecture decisions.

It is generated and updated by CLI commands. It should be reviewed in pull requests, but not edited by hand in normal use.

Example:

```json
{
  "version": 1,
  "concepts": {
    "identity.user": {
      "canonical": {
        "symbol": "crate::domain::identity::User",
        "source": "accepted"
      },
      "owner": {
        "module": "crate::domain::identity",
        "source": "inferred"
      },
      "adapters": [
        {
          "symbol": "crate::api::v1::UserDto",
          "boundary": "api.v1",
          "source": "accepted"
        },
        {
          "symbol": "crate::persistence::UserRow",
          "boundary": "persistence",
          "source": "accepted"
        }
      ],
      "converters": [
        {
          "from": "crate::api::v1::UserDto",
          "to": "crate::domain::identity::User",
          "symbol": "impl TryFrom<UserDto> for User",
          "source": "accepted"
        }
      ]
    }
  },
  "exceptions": []
}
```

The lockfile should support:

* accepted canonical types
* accepted boundary adapters
* accepted converters
* accepted protocol translations
* active exceptions
* stale symbol detection
* rename/prune workflows

---

## Source Hints

Source hints are optional.

They exist for ambiguity, legacy code, generated code, or cases where inference is not enough.

Preferred form: compact comments.

Rust:

```rust
// locus: ot canonical
pub struct User { ... }

// locus: ot boundary
pub struct UserDto { ... }

// locus: ot converter
impl TryFrom<UserDto> for User { ... }
```

Long form only when needed:

```rust
// locus: ot boundary identity.user api.v1
pub struct PrincipalResponse { ... }
```

Avoid proc macros as the default.

Allowed source hint categories:

```text
ot: canonical
ot: boundary
ot: converter
ot: protocol-translation
ot: generated-boundary
ot: allow <RULE> reason="..." expires="YYYY-MM-DD"
```

---

## Rule Set

### OT001 — Duplicate Canonical Concept

A concept may not have multiple canonical types within the same scope/runtime.

Fail when two accepted or strongly inferred canonical types represent the same concept.

### OT002 — Undeclared Concept-Shaped Type

A new type overlaps strongly with an existing concept but is neither canonical nor an accepted boundary adapter.

Signals:

* name overlap
* field overlap
* enum variant overlap
* primitive equivalents of canonical fields
* location in a boundary path
* conversion-like usage
* DTO/model/entity suffix

### OT003 — Boundary Adapter Leak

Boundary adapters must not appear in domain or application service signatures, state, or core logic.

Bad:

```rust
pub fn create_user(req: CreateUserRequest) -> Result<User, Error> { ... }
```

inside domain/application.

Boundary types must be converted before crossing inward.

### OT004 — Direct Canonical Construction Outside Owner/Converter

Canonical types may only be constructed in the owner module or accepted converters.

Bad:

```rust
let user = User {
    id: UserId::parse(dto.id)?,
    email: EmailAddress::parse(dto.email)?,
};
```

outside the owner or converter.

### OT005 — Missing Converter

An accepted boundary adapter must have an accepted converter for each required direction.

Direction may be inferred:

* inbound request: adapter -> canonical
* outbound response: canonical -> adapter
* persistence row: usually bidirectional

### OT006 — Unregistered Conversion

Any conversion between a boundary adapter and canonical type must be accepted.

Examples:

* Rust `From` / `TryFrom`
* TypeScript `toUser(dto): User`
* C# `ToDomain()` / `FromDto()`
* Go `UserFromDTO(dto)`

### OT007 — Adapter-to-Adapter Conversion

Boundary adapter to boundary adapter conversion is forbidden by default.

Preferred path:

```text
adapter -> canonical -> adapter
```

Direct protocol translation must be explicitly accepted with a reason.

### OT008 — Domain Logic on Boundary Adapter

Boundary adapters must not contain domain behavior.

Bad:

```rust
impl UserDto {
    pub fn is_active(&self) -> bool {
        self.status == "active"
    }
}
```

This belongs on the canonical domain concept.

### OT009 — Scattered Validation

Validation or normalization of a known concept outside the concept owner or converter is suspicious.

Examples:

* email format checks outside `EmailAddress`
* status string interpretation outside `UserStatus`
* range checks for value objects outside the value object
* duplicated regexes
* repeated lowercase/trim normalization

Start as warning, fatal in agent strict mode for high-confidence cases.

### OT010 — Shadow Enum

An enum overlaps with a canonical enum concept but is not accepted as a boundary adapter.

### OT011 — Shadow Newtype / Value Object

A newtype or primitive alias overlaps with an existing value object concept.

### OT012 — Primitive Obsession Around Known Concept

A primitive field appears where a canonical value object is expected outside a boundary adapter.

Example:

```rust
pub struct UserCommand {
    pub user_id: String,
}
```

inside application/domain when `UserId` is canonical.

---

## Severity Modes

### Human Default Mode

* fatal: boundary leaks
* fatal: direct canonical construction outside owner/converter
* fatal: duplicate accepted canonical type
* fatal: missing converter for accepted adapter
* warning: inferred shadow types
* warning: scattered validation

### Agent Strict Mode

Designed for LLM code assistants.

Additional fatal checks:

* new public concept-shaped type without acceptance
* new mapper/converter without acceptance
* validation-like code around known concepts
* primitive substitutes for known concepts
* adapter-to-adapter conversions
* new domain-ish methods on boundary types

Command:

```bash
locus check --agent-strict
```

Patch mode should be supported:

```bash
locus check --patch /tmp/agent.patch --agent-strict
```

---

## Diagnostics

Diagnostics must be precise, non-moralizing, and actionable.

Bad:

```text
Architecture violation. You duplicated code.
```

Good:

```text
error[OT002]: undeclared concept-shaped type

src/api/v1/user.rs:12
  pub struct UserModel { ... }

`UserModel` overlaps 91% with accepted canonical concept `identity.user`.

Canonical:
  crate::domain::identity::User

Overlapping fields:
  id
  email
  display_name

This type must either:
  - be removed and use the canonical type,
  - be accepted as a boundary adapter,
  - or be explicitly allowed with a reason.

Suggested command:
  locus accept crate::api::v1::UserModel --as boundary --concept identity.user --boundary api.v1
```

Every diagnostic should include:

* rule ID
* file and location
* detected concept
* accepted canonical type if known
* why it matched
* what authority is missing
* suggested fix or command

---

## CLI Commands

Minimum useful CLI:

```bash
locus init
locus check
locus check --changed
locus check --agent-strict
locus check --patch <file>
locus explain <concept>
locus explain-symbol <symbol>
locus accept <symbol> --as canonical --concept <concept>
locus accept <symbol> --as boundary --concept <concept> --boundary <boundary>
locus accept-converter <from> <to>
locus reject <symbol>
locus debt
locus graph <concept>
locus emit-air
locus prune
```

Nice-to-have later:

```bash
locus classify
locus add adapter <concept> --boundary <boundary> --name <name>
locus query canonical-for <symbol>
locus query owner <concept>
locus query can-convert <from> <to>
```

---

## Expected First Implementation

Build the first version as Rust-first, language-agnostic core.

Suggested workspace:

```text
locus/
  crates/
    locus-air/
    locus-core/
    locus-rust/
    locus-cli/
    locus-report/
```

### `locus-air`

Defines language-neutral data structures:

* `AirWorkspace`
* `AirPackage`
* `AirFile`
* `AirType`
* `AirField`
* `AirFunction`
* `AirConversion`
* `AirUsage`
* `AirTruthAction`
* `AirHint`
* `Confidence`
* `Provenance`

### `locus-core`

Consumes AIR and implements:

* concept inference
* boundary inference
* concept graph construction
* lockfile model
* rule engine
* diagnostics
* accept/reject operations

### `locus-rust`

Rust source adapter.

Initial implementation may use:

* `cargo metadata`
* `syn`
* `walkdir`
* raw source scanning for `// locus:` hints

Detect initially:

* structs
* enums
* type aliases
* field names/types
* derives/attributes
* module path from file path
* visibility
* `impl From` / `impl TryFrom`
* obvious constructor functions
* struct literals
* field-by-field mappings where practical

Do not start with full rustc integration.

### `locus-cli`

Provides commands listed above.

### `locus-report`

Formats diagnostics:

* human text
* JSON
* SARIF later

---

## Rust Adapter MVP Detection

The Rust adapter should first detect:

### Type facts

```rust
pub struct User { ... }
pub enum UserStatus { ... }
type UserId = String;
```

### Boundary signals

* `Serialize` / `Deserialize`
* filename/path under `api`, `routes`, `dto`, `contract`, `db`, `persistence`, `proto`, `generated`
* suffixes: `Dto`, `Request`, `Response`, `Row`, `Record`, `Entity`, `Model`, `Schema`, `Message`

### Canonical signals

* path under `domain`, `core`, `model`
* no boundary suffix
* used by application/domain services
* has methods/smart constructors
* uses value objects rather than primitive-only fields

### Conversion signals

* `impl From<A> for B`
* `impl TryFrom<A> for B`
* functions named `to_*`, `from_*`, `map_*`, `convert_*`, `into_*`
* functions taking one concept-shaped type and returning another
* struct literal construction from another object’s fields

### Truth action signals

* struct literal construction of canonical-like type
* enum matching
* string comparisons against status/kind/type fields
* validation-like checks: `contains`, `starts_with`, `ends_with`, regex, length/range checks
* normalization-like calls: `trim`, `to_lowercase`, `to_uppercase`, parsing

---

## Inference Strategy

Use confidence-based inference.

Example candidate relation:

```json
{
  "candidate": "crate::api::v1::UserDto",
  "role": "boundary_adapter",
  "concept": "identity.user",
  "confidence": 0.93,
  "reasons": [
    "name suffix Dto",
    "path src/api/v1",
    "field overlap with User: 87%",
    "TryFrom<UserDto> for User exists"
  ]
}
```

Suggested thresholds:

```text
>= 0.90: strong inference
>= 0.70: warning / needs acceptance
>= 0.50: advisory only
```

Agent strict mode can treat `>= 0.70` as fatal for new code.

---

## Exceptions

Exceptions must be local, explicit, and reviewable.

No broad ignores.

Required fields:

* rule
* target/location
* reason
* expiry date

Example source hint:

```rust
// locus: allow OT009 reason="legacy migration import" expires="2026-07-01"
```

Example lockfile exception:

```json
{
  "rule": "OT009",
  "target": "src/api/v1/import.rs:42",
  "reason": "legacy migration import",
  "expires": "2026-07-01"
}
```

Command:

```bash
locus debt
```

Should list all active and expired exceptions.

Expired exceptions should fail CI.

---

## Generated Code

Generated code must be supported without fighting it.

Generated paths are configured once:

```yaml
generated:
  - target/**
  - src/generated/**
  - gen/**
```

Generated types may be accepted as boundary adapters automatically or semi-automatically.

But generated types must still not leak into domain/application logic.

Generated code is allowed to exist. It is not allowed to become domain truth.

---

## Protocol Translation

Default rule:

```text
adapter -> canonical -> adapter
```

Direct adapter-to-adapter conversion is forbidden.

Sometimes direct protocol translation is intentional. It must be accepted explicitly:

```bash
locus accept-protocol-translation ApiV1UserDto ApiV2UserDto \
  --reason "compatibility endpoint"
```

or source hint:

```rust
// locus: ot protocol-translation reason="compatibility endpoint"
fn translate_v1_to_v2(value: UserV1Dto) -> UserV2Dto { ... }
```

---

## Assistant / Agent Instructions

When using this document to prime a coding agent, include this section.

### Agent rules

1. Do not create new domain-shaped structs, enums, DTOs, rows, records, models, schemas, or aliases without checking the accepted concept graph.
2. Do not create local mapper functions if an accepted converter exists.
3. Do not pass boundary types into domain or application logic.
4. Do not add validation, normalization, or status interpretation outside the concept owner or accepted converter.
5. Do not convert boundary adapter to boundary adapter directly unless explicitly accepted as protocol translation.
6. Prefer canonical types inside domain/application code.
7. Boundary adapters are only for external protocols, persistence, generated code, or import/export surfaces.
8. If unsure, query the tool rather than inventing a new shape.

Suggested agent commands:

```bash
locus explain <concept>
locus explain-symbol <symbol>
locus query canonical-for <symbol>
locus query can-convert <from> <to>
locus check --changed --agent-strict
```

If the project does not yet implement these commands, preserve this behavior in the design and do not replace it with ad hoc architecture comments.

---

## Example Happy Path

Canonical type:

```rust
pub struct User {
    id: UserId,
    email: EmailAddress,
    display_name: DisplayName,
}
```

Boundary type:

```rust
pub struct UserDto {
    pub id: String,
    pub email: String,
    pub display_name: String,
}
```

Accepted converter:

```rust
impl TryFrom<UserDto> for User {
    type Error = UserConversionError;

    fn try_from(value: UserDto) -> Result<Self, Self::Error> {
        Ok(User::create(
            UserId::parse(value.id)?,
            EmailAddress::parse(value.email)?,
            DisplayName::new(value.display_name)?,
        )?)
    }
}
```

The tool accepts:

```text
UserDto -> User
```

The tool rejects:

```rust
fn create_user(dto: UserDto) -> Result<User, Error> {
    Ok(User {
        id: UserId::parse(dto.id)?,
        email: EmailAddress::parse(dto.email)?,
        display_name: DisplayName::new(dto.display_name)?,
    })
}
```

unless this function is the accepted converter or inside the owner module.

---

## Example Diagnostic

```text
error[OT004]: direct canonical construction outside owner/converter

src/api/v1/user.rs:63:15
  let user = User { ... }
             ^^^^

Concept:
  identity.user

Canonical:
  crate::domain::identity::User

This file is in boundary:
  api.v1

Only the owner module or an accepted converter may construct this concept.

Suggested fix:
  use the accepted converter:
    User::try_from(dto)?

Or accept this function as a converter:
  locus accept-converter crate::api::v1::UserDto crate::domain::identity::User
```

---

## Implementation Phases

### Phase 1 — Rust scanner and AIR

Goal: emit useful AIR from a Rust workspace.

Deliverables:

* parse workspace with `cargo metadata`
* collect Rust files
* parse structs/enums/type aliases with `syn`
* collect fields, derives, visibility, module paths
* parse compact `// locus:` hints
* emit AIR JSON
* CLI command: `locus emit-air`

### Phase 2 — Concept graph and lockfile

Goal: infer concepts and create an accepted lockfile.

Deliverables:

* concept candidates
* boundary candidates
* converter candidates
* confidence/reason output
* `locus init`
* `locus explain-symbol`
* `locus accept`
* generated `.locus/lock.json`

### Phase 3 — First fatal rules

Goal: become useful in CI.

Implement:

* OT001 duplicate canonical
* OT002 undeclared concept-shaped type, at least in agent strict mode
* OT003 boundary leak
* OT004 direct canonical construction
* OT005 missing converter
* OT006 unregistered conversion
* OT007 adapter-to-adapter conversion

### Phase 4 — Agent strict patch mode

Goal: block LLM shadow implementations before they land.

Deliverables:

* changed-file mode
* patch mode if practical
* stricter thresholds
* precise suggested fixes

### Phase 5 — Generators

Goal: make the canonical path shorter than the shadow path.

Deliverables:

* `locus add adapter`
* converter stub generation
* lockfile update

### Phase 6 — Additional language adapters

Candidates:

* TypeScript
* C# / Roslyn
* Go
* Python

Each adapter emits AIR. The core should not become language-specific.

---

## Design Constraints

* No default Rust proc macros.
* No required runtime dependency in checked projects.
* No large semantic hand-authored config.
* Concepts should be inferred or accepted into a generated lockfile.
* Source hints must be compact and optional.
* Diagnostics must explain why the tool thinks something is a concept violation.
* Every exception requires a reason and expiry.
* Core must stay language-agnostic.
* Blocking rules must be deterministic, not LLM-based.
* LLM/semantic advisory mode may exist later, but must not be required for CI.

---

## First Milestone Definition of Done

A useful first milestone is complete when the tool can run on a Rust project and:

1. emit AIR JSON,
2. infer likely canonical and boundary types,
3. generate `.locus/lock.json`,
4. accept or reject candidate symbols,
5. detect a newly added `UserModel`-style shadow type,
6. detect direct construction of a canonical type outside owner/converter,
7. detect boundary DTO usage in domain/application code,
8. explain each finding with confidence and reasons.

The first milestone does not need perfect Rust name resolution. It only needs to catch common architectural drift with low enough false positives to be useful.

---

## North Star

The successful version of Locus makes this impossible to sneak into a codebase:

```rust
pub struct UserModel {
    pub id: String,
    pub email: String,
    pub display_name: String,
}

fn map_user(model: UserModel) -> User {
    User {
        id: UserId::parse(model.id).unwrap(),
        email: EmailAddress::parse(model.email).unwrap(),
        display_name: DisplayName::new(model.display_name).unwrap(),
    }
}
```

unless the project explicitly accepts `UserModel` as a boundary adapter and `map_user` as the named converter.

The tool does not prevent boundary shapes. It prevents silent parallel truth.

That is the project.
