# Locus CLI Agent Interface

## Purpose

The Locus CLI is not only a human linter command. It is the local architectural oracle that coding agents should consult before they change a codebase.

The CLI has two jobs:

1. Tell the agent what architectural problem it is about to create or has already created.
2. Tell the agent the narrow, deterministic command path for recording accepted architectural intent in `locus.lock`.

The CLI must stay compact enough to fit into agent context. It should prefer short, actionable diagnostics over long explanations, and expose deeper explanation only when asked.

## Design constraints

- `locus check` remains deterministic. No LLM decides pass/fail.
- Agents should not hand-edit `locus.lock`.
- Every lockfile write goes through a typed CLI mutator.
- Diagnostic output should include a next action, not just a rule violation.
- The compact agent instruction block should fit in `AGENTS.md` without becoming project documentation.
- `init` bootstraps the agent handoff, but does not infer project architecture beyond deterministic source facts.

## `locus init`

`locus init` creates or refreshes the local Locus files for a repository:

```bash
locus init --workspace .
```

Required behaviour:

- scan the workspace and promote accepted source hints into `locus.lock`,
- create `locus.lock` when missing,
- preserve existing accepted ownership decisions,
- refuse to silently delete lockfile entries that still have source references,
- add a small Locus section to `AGENTS.md` when the file exists,
- create `AGENTS.md` with only the Locus section when no agent handoff file exists,
- never add a large rule catalogue to `AGENTS.md`.

Suggested flags:

```bash
locus init --workspace . --agent-instructions append   # default
locus init --workspace . --agent-instructions print    # stdout only
locus init --workspace . --agent-instructions none
locus init --workspace . --check                       # verify files without writing
```

The `AGENTS.md` block should be idempotent and bounded by markers:

```md
<!-- locus:start -->
## Locus

This repository uses Locus for deterministic architecture checks. Before changing architecture-sensitive code, run:

```bash
locus check --workspace . --changed --agent-strict
```

If Locus reports a violation, do not hand-edit `locus.lock`. Either change the code to use the accepted owner/boundary/converter, or use the matching `locus accept ...` / `locus <paradigm> ...` command when the architecture decision is intentional.

For more context, run:

```bash
locus explain <diagnostic-id>
locus query owner <symbol-or-path>
locus debt
```
<!-- locus:end -->
```

The generated block is intentionally small. The full rule semantics live in the project's docs, not in every checked repository's agent prompt.

## Compact agent check output

Agents need machine-stable output with human-readable summaries. Add an agent-oriented check mode:

```bash
locus check --workspace . --changed --agent-strict --format agent
```

Output shape:

```text
LOCUS FAIL 3 fatal, 2 warning

[OT004] fatal src/user/service.rs:44
Problem: User is constructed outside the accepted owner or converter.
Owner: identity::domain::User
Allowed path: add/use accepted converter identity::api::CreateUserRequest -> identity::domain::User
Next: change code to call the converter, or run `locus accept converter <symbol> --concept identity.user` if this converter is intentional.
Explain: locus explain OT004:src/user/service.rs:44

[DG001] fatal src/domain/mod.rs:8
Problem: domain imports api, but the lockfile forbids that direction.
Next: invert the dependency or update the architectural edge with `locus dg forbid-edge ... --force` only if the architecture changed.
Explain: locus explain DG001:src/domain/mod.rs:8
```

Rules for agent output:

- keep each diagnostic to one screen or less,
- include rule id, severity, file span, problem, accepted owner when known, and next action,
- include the exact command to ask for deeper explanation,
- include lockfile mutation commands only when they are valid for that rule family,
- never suggest editing `locus.lock` manually.

## `locus explain`

`explain` expands one diagnostic into enough context for an agent to make the right local change without loading the whole rule spec.

```bash
locus explain <diagnostic-id>
locus explain OT004:src/user/service.rs:44
locus explain --from-check .locus/last-check.json OT004:1
```

Expected content:

- what the rule protects,
- why this code location lacks authority,
- what accepted owner or boundary currently owns the concern,
- the preferred code-level fix,
- the valid lockfile mutation command when the architecture decision is intentional,
- examples of invalid fixes that merely hide the issue.

Example:

```text
OT004 — Direct Canonical Construction Outside Owner/Converter

This code constructs `identity::domain::User` in `identity::api::handler`.
`User` is canonical for `identity.user`; construction authority belongs to the owner module and accepted converters.

Preferred fix:
  Convert `CreateUserRequest` through an accepted converter before entering application/domain logic.

Valid acceptance path:
  locus accept converter identity::api::CreateUserRequest::try_into_user --concept identity.user --reason "api v1 inbound request"

Do not fix by:
  - adding a second User-like type,
  - moving the same construction into a generic helper,
  - adding `// locus: allow` without an expiry and reason.
```

## `locus query`

`query` is the low-context lookup surface for agents. It answers architectural authority questions without running the full rule set.

```bash
locus query owner <symbol-or-path>
locus query concept <concept-id>
locus query boundary <symbol-or-path>
locus query converter <from-symbol> <to-symbol>
locus query edge <from-path> <to-path>
locus query facts <symbol-or-path>
```

Examples:

```bash
locus query owner identity::domain::User
locus query converter identity::api::CreateUserRequest identity::domain::User
locus query edge lore::domain::service lore::api::dto
```

Query output should be terse:

```text
Owner: identity.user
Canonical: identity::domain::User
Boundaries:
  - identity::api::CreateUserRequest boundary=api.v1 inbound
Converters:
  - identity::api::CreateUserRequest -> identity::domain::User via TryFrom
```

## Lockfile mutation commands

The lockfile records accepted architectural facts. It is not a hand-written architecture manifesto.

Every mutation command should:

- validate symbols or patterns against current AIR where possible,
- require a reason for non-obvious architecture changes,
- reject duplicates unless `--force` updates an existing entry,
- preserve namespace ownership under `paradigms.<PREFIX>`,
- print the exact lockfile path it changed,
- keep generated ordering stable.

### OT ownership

```bash
locus accept canonical <symbol> [--concept <id>] [--reason <text>]
locus accept boundary <symbol> --concept <id> [--boundary <name>] [--direction inbound|outbound|bidirectional] [--reason <text>]
locus accept converter <symbol> --concept <id> [--from <symbol>] [--to <symbol>] [--reason <text>]
locus accept protocol-translation <symbol> --from-boundary <name> --to-boundary <name> --reason <text>
```

### Dependency direction

```bash
locus dg forbid-edge --from <pattern> --to <pattern> --reason <text>
locus dg define-feature --name <id> --module <pattern> [--public-api <pattern>...]
locus dg add-shared-path <pattern> --reason <text>
```

### Config/data ownership

```bash
locus cf accept-config-path <pattern> --reason <text>
locus cf forbid-id-pattern <regex> --reason <text>
locus cf accept-decision-owner <symbol-or-path> --concept <id> --reason <text>
```

### Runtime ownership

```bash
locus rw accept-runtime-owner <symbol-or-path> --reason <text>
locus rw accept-background-worker <symbol-or-path> --reason <text>
locus rw accept-hot-path <symbol-or-path> --reason <text>
```

### Failure lineage

```bash
locus fl accept-failure-sink <symbol-or-path> --reason <text>
locus fl accept-retry-policy <symbol-or-path> --reason <text>
locus fl allow-discard <callee-pattern> --reason <text>
```

### Exceptions

Exceptions are last-resort, narrow, and temporary.

```bash
locus allow <rule-id> <symbol-or-path> --reason <text> --expires <YYYY-MM-DD>
locus debt
locus debt --expired
locus prune
```

`allow` writes the same semantic structure as a source hint, but into the lockfile. It must require a reason and expiry. `debt` lists active and expired exceptions. `prune` removes stale lockfile entries only when current AIR proves the referenced source no longer exists.

## Diagnostic-to-command mapping

Each rule should expose a small command hint table. Examples:

| Rule family | Primary fix | Valid acceptance command |
|-------------|-------------|--------------------------|
| OT duplicate/shadow | use accepted canonical | `locus accept boundary ...` or `locus accept canonical ... --force` |
| OT missing converter | add converter | `locus accept converter ...` |
| DG forbidden import | invert dependency | `locus dg forbid-edge ... --force` only for architecture changes |
| CF hardcoded decision | move value to config owner | `locus cf accept-decision-owner ...` |
| RW spawn/blocking | route through runtime owner | `locus rw accept-runtime-owner ...` |
| FL silent failure | propagate/handle with context | `locus fl accept-failure-sink ...` or `locus allow ... --expires ...` |
| MO god module | split ownership | usually no lockfile command; fix structure |
| DC LLM residue | rewrite docs/comments | no lockfile command |

If a rule has no valid acceptance path, the diagnostic should say so. Not every architectural problem deserves a lockfile escape hatch.

## `locus agent instructions`

`init` should call the same renderer exposed directly as:

```bash
locus agent instructions
locus agent instructions --format agents-md
locus agent instructions --format claude-md
locus agent instructions --format plain
```

This makes the generated prompt block testable and lets humans refresh it manually.

The renderer should be versioned by content, not by timestamp, so repeated runs do not churn files.

## Non-goals

- Do not dump the entire paradigm catalogue into agent context.
- Do not ask agents to infer architectural intent from prose when the lockfile can answer it.
- Do not make `locus.lock` an editable DSL.
- Do not let an LLM decide whether a diagnostic is valid.
- Do not add framework-specific commands before the normalized loader facts exist.
