# PG — Policy Guard

Date: 2026-05-09 (revised after PR #46 review — see "Review fixes" below)
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

#### PG000 — Policy Guard baseline missing

Fires when no baseline lockfile could be resolved (workspace is not a
git repo, no baseline ref exists, the baseline ref doesn't carry a
`locus.lock`, or the file fails to parse). Without this, an agent
could silently disable the audit by manipulating the baseline (shallow
clone, wrong `--baseline` ref, etc.).

Severity: Warning by default; **Fatal under `--agent-strict` regardless
of `--allow-policy-calibration`**. Calibration acknowledges intentional
widening, not a missing audit. Use `--allow-missing-policy-baseline`
when the gap is genuinely expected (first-time onboarding, intentional
shallow clone for local prototyping).

#### PG001 — policy budget raised

Fires when a numeric budget increased between baseline and current.
Two surfaces:

**Workspace-wide defaults**:

- `paradigms.CX.default_max_function_lines`
- `paradigms.CX.default_max_module_lines`
- `paradigms.CX.max_public_items`
- `paradigms.CX.max_fan_out`
- `paradigms.MO.default_max_public_types`
- `paradigms.MO.entropy_threshold`
- (any future `default_max_*` / `max_*` field)

**Existing-override budgets** (keyed by `module`, identifies the
"slipperier cheat" of bumping an already-tagged override):

- `paradigms.CX.overrides[*].max_function_lines`
- `paradigms.CX.module_overrides[*].max_module_lines`
- `paradigms.MO.overrides[*].max_public_types`

A budget DECREASE is not flagged — that's tightening. Going from
unset (built-in fallback) to a higher explicit value counts as a raise.

#### PG002 — new override added

Fires whenever an override appears in the current lockfile under a
`module` pattern that wasn't present in the baseline list.
**Independent of debt metadata** — the addition itself is policy
widening that should be visible. PG006 separately fires when metadata
is missing.

Calibration mode (`--allow-policy-calibration`) renders PG002 as
Advisory: the user has explicitly acknowledged the widening.

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

#### PG006 — override lacks debt metadata

Fires alongside PG002 when a new override is missing any of `reason`,
`expires`, `owner`. **Always Fatal under `--agent-strict`, even with
`--allow-policy-calibration`** — calibration legitimizes the act of
adding the override (PG002 → Advisory under calibration), but it does
NOT waive the requirement to record why, when to revisit, and who
owns it.

This is the rule that makes calibration mode meaningful: a calibration
run without metadata fails strict; with metadata, calibration passes
strict because the addition is acknowledged AND justified.

### Severity tiers

- **PG000 (baseline missing)**: Warning by default; Fatal under
  `--agent-strict` regardless of calibration. `--allow-missing-policy-baseline`
  silences it.
- **PG001-PG004 (widening signals)**: Warning by default; Fatal under
  `--agent-strict`. Calibration mode (`--allow-policy-calibration`)
  downgrades to Advisory.
- **PG006 (metadata gap)**: Warning by default; **Fatal under `--agent-strict`
  even with calibration**. Metadata is not negotiable.

The CLI prints a structured "policy calibration" report alongside
normal output when calibration mode is active:

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

`check_policy_mutation(current, baseline, mode, calibration, allow_missing_baseline)`
is called from the CLI's `check` flow.

**Pipeline order matters.** PG runs **after** `apply_exceptions`, not
before. Without that ordering, a `Lockfile.exceptions[]` entry with
`{rule: "*"}` (or `{rule: "PG"}`) would silence PG using the same
lockfile it audits. PG is meta-policy; it must not be suppressible by
normal exceptions. The ordering in `crates/locus-cli/src/main.rs::check`
is:

1. Run paradigm checks → `paradigm_diags`.
2. `apply_exceptions(paradigm_diags, ...)` filters `LOCUS001`-aware
   suppressions.
3. Append PG diagnostics to the post-filter set.

PG diagnostics are subject to the existing `--changed` filter (PG runs
lockfile-vs-lockfile, so the filter is a no-op on it — PG is global by
design).

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

# Calibration: PG001/PG002/PG003/PG004 → Advisory, structured report
# printed. PG006 stays Fatal under strict (metadata required).
locus check --workspace . --agent-strict --allow-policy-calibration

# Custom baseline (e.g. release branch):
locus check --workspace . --baseline origin/release --agent-strict

# Missing baseline accepted (first-time onboarding, prototyping).
# PG000 silenced; PG001-PG006 silently skip with no audit.
locus check --workspace . --agent-strict --allow-missing-policy-baseline
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

## Review fixes (post-PR-#46-review)

The first PR #46 cut had four bypasses caught in review:

1. **PG was suppressible by lockfile exceptions** — `apply_exceptions`
   ran AFTER PG was added, so a `{rule: "*"}` exception erased PG.
   Fix: reordered pipeline so `apply_exceptions` runs first; PG is
   appended after. PG is meta-policy and not subject to normal
   exception suppression.
2. **Existing-override budget raises were silent** — PG002 only
   compared `module` strings, so bumping `max_function_lines: 120 →
   10000` on a pre-existing override silently passed. Fix: PG001 now
   walks the (current ∩ baseline) intersection and flags any override
   whose budget value increased.
3. **Tagged overrides bypassed PG002** — old PG002 only fired when
   metadata was missing, so an agent who learned the new shape could
   add tagged overrides freely. Fix: PG002 now fires on every new
   override regardless of metadata; PG006 is the separate
   missing-metadata sub-rule. Calibration downgrades PG002 but PG006
   stays Fatal.
4. **Missing baseline silently disabled the guard** — first-onboarding
   runs returned no diagnostics. Fix: PG000 fires when the baseline
   can't be resolved, Fatal under strict unless
   `--allow-missing-policy-baseline` is set explicitly.

## MVP scope

- PG000 (baseline missing), PG001 (budget raised, both workspace
  defaults AND existing overrides), PG002 (new override added
  regardless of metadata), PG003 (new exempt_paths), PG004 (new
  acknowledged_empty), PG006 (new override missing metadata).
- Optional debt-metadata fields on `CxOverride`, `CxModuleOverride`,
  `MoOverride`. Future override types follow the same shape.
- Baseline reading via `git show <baseline>:locus.lock`. PG000 fires
  when missing; `--allow-missing-policy-baseline` silences.
- `--allow-policy-calibration` flag with structured report;
  downgrades PG001-PG004 to Advisory but **not** PG000 or PG006.
- Cross-paradigm advisory (PG prefix), wired into the CLI's check
  pipeline alongside `LOCUS001`/`LOCUS002`. Crucially, PG runs
  **after** `apply_exceptions` so lockfile exceptions cannot silence
  the guard.

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
