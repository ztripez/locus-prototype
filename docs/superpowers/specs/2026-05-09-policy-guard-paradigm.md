# PG — Policy Guard

Date: 2026-05-09
Issue: #44 (related: closed PRs #41 and #42 are the triggering example)

## Problem

PR #42 (closed) demonstrated that an agent can make
`locus check --workspace . --agent-strict` clean by **changing the
measurement surface** instead of fixing the architectural issue. In
that PR, CX001's 109 diagnostics were cleared by raising
`default_max_function_lines` from 50 → 120 and adding per-file
overrides. The output was technically green, but the underlying code
was not improved.

This is especially dangerous after issue #6's elevation gating made
broad rules advisory — warnings are allowed to sit, but agents must
not be able to **erase** them by rewriting policy.

The principle, captured by #44:

> Severity controls whether debt blocks normal work.
> Policy mutation controls whether agents are allowed to erase debt.

## Goal

A deterministic policy-mutation gate that distinguishes:

| Outcome | Acceptable to agents? |
|---|---|
| Diagnostic gone because **code improved** | yes |
| Diagnostic gone because **code/symbol was deleted or renamed** | yes |
| Diagnostic gone because **threshold was raised** | **no** without explicit calibration |
| Diagnostic gone because **override or exemption was added** | **no** without debt metadata |
| Diagnostic gone because **rule was disabled or paradigm acknowledged_empty** | **no** without explicit calibration |
| Diagnostic gone because **severity was lowered** | **no** (MVP defers — no severity-override schema yet) |

## Design

### Cross-paradigm check

PG fits the same shape as `LOCUS001` (expired exception) and
`LOCUS002` (vacant paradigm): a cross-paradigm advisory emitted
outside the per-paradigm `check()` flow. It runs **once per `locus
check`** and consumes:

- the **current** lockfile (the one being evaluated)
- a **baseline** lockfile (read via `git show <baseline>:locus.lock`)

When no baseline is available (no git, no prior `locus.lock`), PG
silently skips — first-time onboarding does not fire spurious PG.

### Baseline resolution

Reuses the resolution chain already wired for `--changed`:

```
origin/main → origin/master → main → master → HEAD~1
```

Override via `--baseline <ref>`. The flag works alongside `--changed`;
the same baseline is used by both.

### Rules

#### PG001 — policy budget raised

Fires when a numeric budget field in the lockfile increased between
baseline and current:

- `paradigms.CX.default_max_function_lines`
- `paradigms.CX.default_max_module_lines`
- `paradigms.CX.max_public_items`
- `paradigms.CX.max_fan_out`
- `paradigms.MO.default_max_public_types`
- `paradigms.MO.entropy_threshold`
- (any future `default_max_*` / `max_*` field)

A budget DECREASE is not flagged — that's tightening. Going from
unset (built-in fallback) to a higher explicit value counts as a raise
when the explicit value is greater than the built-in fallback.

#### PG002 — new override added

Fires when an entry exists in the current lockfile's override list
(`paradigms.CX.overrides`, `paradigms.CX.module_overrides`,
`paradigms.MO.overrides`, …) that has no matching `module` pattern in
the baseline list, AND the new entry lacks structured debt metadata.

Required debt metadata (all optional in schema for backward compat,
but PG002 fires when missing):

- `reason` — non-empty string
- `expires` — `YYYY-MM-DD` date
- `owner` — non-empty string

Optional but recommended:

- `debt_id` — stable identifier (e.g. `CX001-visitor-scan-expr`)
- `introduced_by` — PR/issue reference

#### PG003 — new exempt_paths entry

Fires when an entry exists in the current lockfile's `exempt_paths`
that wasn't in the baseline. `exempt_paths` is currently a
`Vec<String>` with no metadata shape; PG003 flags any addition. A
follow-up may extend the schema to structured exempt-path entries
with debt metadata.

#### PG004 — paradigm added to acknowledged_empty

Fires when a prefix is in current `acknowledged_empty` but not in
baseline. Acknowledging vacancy silences the entire paradigm; the
addition deserves visible debt tagging.

#### PG005 — severity lowered (deferred)

The lockfile has no severity-override schema today; rule severity is
hard-coded. PG005 is reserved for the day that schema lands.

### Severity tiers

- **Without `--allow-policy-calibration`**: PG001-PG004 fire as
  Warning by default. Under `--agent-strict`, they elevate to Fatal
  via the existing `mode.elevate` helper. They block CI for any PR
  that widens policy without explicit calibration.
- **With `--allow-policy-calibration`**: PG diagnostics fire as
  Advisory (informational only) and the CLI prints a structured
  "policy calibration" report alongside normal output:

  ```
  Policy calibration report:
    Budgets raised:
      paradigms.CX.default_max_function_lines: 50 → 120 (+70)
    New overrides:
      paradigms.CX.overrides[*]: locus_rust::visitor (max_function_lines = 300)
        reason: "AST dispatcher; per-variant split tracked as follow-up"
        expires: 2026-06-01
        owner: architecture
    New exempt_paths: (none)
    New acknowledged_empty: (none)
  ```

### Where it runs

`check_policy_mutation(current, baseline, mode, calibration)` is
called from the CLI's `check` flow after paradigm checks complete.
Its diagnostics merge into the normal output stream and are subject
to the existing `--changed` filter (PG runs lockfile-vs-lockfile, so
the filter is a no-op on it — PG is global by design).

## Schema additions

The override types in paradigm `lockfile_schema.rs` files gain
optional debt metadata:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CxOverride {
    pub module: String,
    pub max_function_lines: u32,
    /// Why this override exists. Populated when the override is added
    /// via calibration mode; required for PG002 to stay quiet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// `YYYY-MM-DD`. Past expiries surface via `LOCUS001`-shaped
    /// "expired-debt" diagnostics in a follow-up; today PG002 just
    /// requires the field be present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
    /// Who owns this debt — team name, individual, or role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// Optional stable identifier for cross-referencing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debt_id: Option<String>,
    /// Optional PR/issue reference describing the debt's origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced_by: Option<String>,
}
```

Same shape extends to `CxModuleOverride`, `MoOverride`, and any
future override type. The fields are `#[serde(default,
skip_serializing_if = "Option::is_none")]` so existing lockfiles
parse and round-trip unchanged.

## CLI

```bash
# Normal: PG diagnostics fatal under agent-strict.
locus check --workspace . --agent-strict

# Calibration: PG diagnostics advisory, structured report printed.
locus check --workspace . --agent-strict --allow-policy-calibration

# Custom baseline (e.g. release branch):
locus check --workspace . --baseline origin/release --agent-strict
```

## Acceptance criteria mapping (#44)

- [x] `--agent-strict` fails when a PR clears diagnostics by raising
      budgets, adding exemptions, or disabling rules.
- [x] Locus reports policy mutations separately from normal rule
      diagnostics (PG-prefixed rule ids).
- [x] Policy calibration is explicit and cannot be mistaken for
      remediation (`--allow-policy-calibration` plus structured
      report).
- [x] Overrides/exemptions carry structured debt metadata
      (`reason`/`expires`/`owner` schema fields, PG002 enforcement).
- [x] Self-check output distinguishes "0 active diagnostics" from
      "diagnostics suppressed by policy widening" (calibration report
      makes this visible).

## MVP scope

- PG001 (budget raised), PG002 (override added without metadata),
  PG003 (new exempt_paths), PG004 (new acknowledged_empty).
- Optional debt-metadata fields on `CxOverride`, `CxModuleOverride`,
  `MoOverride`. Future override types follow the same shape.
- Baseline reading via `git show <baseline>:locus.lock`. Silent skip
  when git or the prior lockfile isn't available.
- `--allow-policy-calibration` flag with structured report.
- Cross-paradigm advisory (PG prefix), wired into the CLI's check
  pipeline alongside `LOCUS001`/`LOCUS002`.

## Deferred to follow-up

- **PG005** (severity lowered) — depends on a severity-override
  schema landing in the lockfile.
- **Expired-debt diagnostic** (override past its `expires` date) —
  schema is in place; the rule body needs date comparison logic
  similar to `LOCUS001`.
- **Dead-debt detection** (override whose target no longer violates)
  — needs the rule engine to track which overrides actually applied
  during a check; non-trivial.
- **Diagnostic baseline snapshot** — full ratchet behavior (new vs
  unchanged vs improved vs suppressed). Bigger lift; this MVP is the
  foundation.

## Dogfood expectations

After this PR lands, Locus's own `locus.lock` (post #39) carries
existing overrides:

- `paradigms.OT.converter_paths` (3 entries — adapter authority)
- `paradigms.MO.overrides` (2 entries — kitchen-sink data crate +
  paradigm schema)
- `paradigms.CX.exempt_paths` (2 entries — tests, AIR data crate)
- `paradigms.CX.module_overrides` — none in main
- `acknowledged_empty` (12 entries — vacant-by-definition paradigms)

PG would fire on every one of those because they predate this PR.
Two paths:

1. **Treat the post-#39 lockfile as the new baseline.** PG diff is
   only meaningful from this commit forward. (Implemented via the
   "no baseline" silent-skip when `git show origin/main:locus.lock`
   doesn't exist before #39's merge — already true since #39 is the
   commit that introduced the file.)
2. **Retrofit debt metadata onto the existing overrides.** Each
   existing override gets `reason`/`expires`/`owner` populated from
   the rationale already documented in PR #39's commit message and
   the dogfood ledger.

Both paths land in this PR. (1) is structurally automatic; (2)
documents the existing decisions clearly. Once PG is in place, *any*
new override added without metadata fails the gate.

## Self-application invariant

After this PR:

- `locus check --workspace . --agent-strict` continues to exit 0
  (existing overrides have debt metadata; PG002 stays silent).
- A subsequent PR that adds a new untagged override fails
  `--agent-strict`.
- A subsequent PR that uses `--allow-policy-calibration` and adds a
  tagged override passes, with the calibration report visible in
  output.
