# Dogfood Audit — design spec

**Issue:** [#45 — Audit dogfood results for suppressed diagnostics versus real fixes](https://github.com/ztripez/locus/issues/45)

**Status:** approved design; pending implementation plan.

**Date:** 2026-05-09

## Problem

The current dogfood/self-onboarding state is no longer trustworthy as proof
that Locus fixed its own architectural findings. Diagnostics can disappear
from `locus check` for at least a dozen distinct reasons — code refactor,
deletion, severity reclassification, lockfile onboarding, public-API
declarations, converter-path authority grants, exemptions, acknowledged-empty
paradigms, exceptions, budget/default increases, per-module overrides — and
the dogfood evidence collapses all of them into the same "exit 0 under
strict" claim.

`locus check --workspace . --agent-strict` exiting 0 is therefore
insufficient evidence. The audit replaces the single binary claim with a
classified breakdown.

## Goal

Produce a one-time audit that reclassifies each diagnostic's disposition
across the dogfood-relevant PR sequence and lands an honest project status
in `CLAUDE.md`. Open follow-up issues for schema gaps, un-landed work, and
named refactor candidates. Do not invent lockfile schema or refactor code in
this PR.

## Non-goals (scope guardrails)

- No lockfile schema changes. `CX.exempt_paths` and `acknowledged_empty`
  debt metadata is named as a gap; not implemented here.
- No code refactors. `scan_expr`, paradigm rule file splits, and the
  test-extraction half of PR #41 are tracked via follow-up issues.
- No re-landing of PR #41 or PR #42.
- No new Policy Guard test cases.
- No `locus dogfood-audit` command or audit script. Methodology gives
  reproducible commands; Policy Guard (#46) blocks future drift.
- No verdict-taxonomy expansion. 15 classes locked; no 16th invented during
  execution.
- No `README.md` change (verified to not overclaim).
- No `paradigms/` module or rule additions.
- No invented baseline numbers — PR #41/#42 deltas come from PR descriptions
  with `verified: false`.

## Deliverable

Three files plus six issues plus one paragraph edit:

| Path | Action |
|---|---|
| `docs/superpowers/specs/2026-05-09-dogfood-audit-design.md` | NEW — this design spec |
| `docs/superpowers/specs/2026-05-09-dogfood-audit.md` | NEW — narrative audit |
| `docs/superpowers/specs/2026-05-09-dogfood-audit.json` | NEW — structured records |
| `CLAUDE.md` | Replace one paragraph in "Project status" |
| `README.md` | NO CHANGE |
| `locus.lock` | NO CHANGE |
| GitHub issues | 6 new follow-up issues |

## Audited PR sequence

| PR | Title | Merged | Role |
|---|---|---|---|
| #36 | Severity tier policy + CX001/CX002 advisory elevation | yes | Severity tier reclassification |
| #39 | Locus self-onboarding completion (closes #30/#31/#32, epic #37) | yes | Lockfile onboarding (47 OT canonicals, 5 DG features, MO overrides, CX exempt_paths, 14 exceptions, 12 acknowledged_empty) |
| #41 | CX002 cleanup: extract tests + calibrate module budgets | **no (closed)** | Mixed: real test-extraction refactor + policy calibration |
| #42 | CX001 cleanup: calibrate per-function budget | **no (closed)** | Pure policy calibration |

PR #46 (Policy Guard) is on `origin/main` (`2144368`) at audit time but is
acknowledged as future protection only. It does not retire any existing
diagnostic; it does not appear in the per-rule disposition counters. It is
referenced in the audit's "going forward" framing only.

## Verdict taxonomy (locked, 15 classes)

| Class | Semantics |
|---|---|
| `resolved_by_code` | Code change altered the call site/shape so the rule no longer matches. Diagnostic gone, no policy added. |
| `resolved_by_deletion` | Symbol or call site deleted. Diagnostic gone because the surface is gone. |
| `resolved_by_symbol_move` | Symbol renamed/moved into a location matching the rule's accepted shape (e.g., into a converter module). Behavior didn't change. |
| `accepted_by_exception` | `Lockfile.exceptions[]` entry with `expires` + `reason` covering this finding. Diagnostic still emits, suppressed by exception, becomes `LOCUS001` after expiry. |
| `suppressed_by_public_api` | DG `features[].public_api` declaration covers the symbol, so DG003/DG004 stop classifying it as internal-reach. |
| `suppressed_by_converter_paths` | OT `converter_paths` pattern matches the call site, granting adapter authority for OT004 / construction rules. |
| `suppressed_by_exempt_paths` | CX `exempt_paths` pattern matches the file/module, so CX rules don't evaluate it. **No debt-metadata schema today.** |
| `suppressed_by_acknowledged_empty` | Paradigm prefix in `acknowledged_empty[]` silences `LOCUS002` vacancy nudge. **No debt-metadata schema today.** |
| `suppressed_by_budget_increase` | Workspace-wide `default_max_*` raised so threshold no longer fires. |
| `suppressed_by_override` | Per-module override (`paradigms.<P>.module_overrides` / `paradigms.MO.overrides`) raised the threshold for one module. |
| `suppressed_by_severity_tier` | Severity policy moved the rule from strict-immediate or strict-after-onboarding to advisory. **Diagnostics still emit; they no longer block CI under `--agent-strict`.** Phrasing: *blocking status disappeared; diagnostics remained as warnings*. |
| `remaining_warning_debt` | Diagnostic still emits as Warning (and stays Warning under strict because rule is Advisory and not narrowed). Not suppressed; explicitly carrying the debt. |
| `remaining_fatal` | Diagnostic still emits as Fatal. Audit should be near-zero here for the dogfood claim to mean anything. |
| `proposed_but_not_landed` | Mechanism was proposed in a PR that did not merge (PR #41 / PR #42). Cited for forensic completeness; not contributing to current state. |
| `unknown` | Audit could not classify with confidence. Every entry in this class needs a follow-up issue. |

PR #39's persistence of `// locus: ot canonical` source hints into
`locus.lock` does **not** introduce a 16th class. Each such finding is
counted as `resolved_by_code` with the JSON note:

```json
{
  "class": "resolved_by_code",
  "note": "authority was already present in source hints before the audited PR window; PR #39 persisted it into locus.lock"
}
```

## JSON shape

### Top level

```jsonc
{
  "audit_baseline_ref": "c479ce3",        // pre-#36 (PR #35 merge head)
  "audit_target_ref":   "2144368",        // origin/main HEAD at audit time (post-#46)
  "audit_date":         "2026-05-09",
  "methodology": "see audit doc §Methodology",
  "measurement_mode": "bulk_cluster_for_uniform_cx_severity_tier; per-mechanism classification for lockfile suppressions",
  "verdict_taxonomy": [ /* 15 locked classes */ ],
  "totals": {
    "active_fatals":           0,
    "active_warnings":         "<measured>",
    "accepted_debt_entries":   "<measured: 14 lockfile.exceptions + 2 MO.overrides>",
    "policy_suppressions":     "<measured>",
    "severity_tier_demotions": "<measured>",
    "remaining_warning_debt":  "<measured>"
  },
  "rules": [ /* per-rule records */ ],
  "prs":   [ /* per-PR forensic records */ ]
}
```

### Per-rule record

One per rule that has any non-zero count in any class. Counters are
mutually-exclusive disposition buckets — every diagnostic from `before_fatal`
lands in exactly one class, and `before_fatal == sum(all classes)`. This
makes the JSON arithmetically auditable.

`LOCUS002` (vacancy nudge) is included as its own per-rule record when
counting `suppressed_by_acknowledged_empty`. It is a Locus-internal
advisory, not a paradigm rule, but it carries diagnostic counts that the
audit must classify the same way.

> Numbers in the example below are illustrative shape, not verified
> measurements. Per-rule numbers in the committed audit come from Layer 1
> measurement at each ref. PR #36's own baseline shows CX001 ×106; PR #39's
> baseline shows CX001 ×107 — the count drift between adjacent refs is
> itself signal the audit surfaces.

```jsonc
{
  "rule": "CX001",
  "before_fatal":   107,
  "after_fatal":    0,
  "after_warning":  107,
  "resolved_by_code":              0,
  "resolved_by_deletion":          0,
  "resolved_by_symbol_move":       0,
  "accepted_by_exception":         0,
  "suppressed_by_public_api":      0,
  "suppressed_by_converter_paths": 0,
  "suppressed_by_exempt_paths":    0,
  "suppressed_by_acknowledged_empty": 0,
  "suppressed_by_budget_increase": 0,
  "suppressed_by_override":        0,
  "suppressed_by_severity_tier":   107,
  "remaining_warning_debt":        107,
  "remaining_fatal":               0,
  "proposed_but_not_landed":       0,
  "unknown":                       0,
  "verdict": "not_remediated_remaining_warning_debt",
  "responsible_policy": [
    { "field": "rule_severity_tier", "source": "PR #36", "ref": "docs/PARADIGMS.md §Severity tiers" }
  ],
  "findings": []   // present only for semantic rules where per-symbol detail matters
}
```

### Per-PR record

```jsonc
{
  "pr": 36,
  "title": "Severity tier policy + CX001/CX002 advisory elevation",
  "merged": true,
  "merged_at": "2026-05-09T14:47:31Z",
  "primary_mechanism": "suppressed_by_severity_tier",
  "rule_deltas": [
    { "rule": "CX001", "before_fatal": 107, "after_fatal": 0, "after_warning": 107, "class": "suppressed_by_severity_tier" },
    { "rule": "CX002", "before_fatal": 27,  "after_fatal": 0, "after_warning": 27,  "class": "suppressed_by_severity_tier" }
  ],
  "verdict": "blocking_status_changed_diagnostics_remained",
  "notes": "PR #36 changed severity tier; diagnostics did not disappear."
}
```

PR #41 / #42 records use `"merged": false`,
`"primary_mechanism": "proposed_but_not_landed"`,
`"contributes_to_current_state": false`, with each rule_delta annotated
`"source": "PR description"` and `"verified": false`.

## Methodology

The audit is defensible only if a future agent can reproduce its numbers.
Methodology has three layers.

### Layer 1 — Ground-truth measurement at four git refs

Check out each ref, build, run `cargo run -p locus-cli -- check --workspace .`
and `… --agent-strict`, capture diagnostic count by rule.

| Ref | Commit | What it represents |
|---|---|---|
| `pre_36` | `c479ce3` (PR #35 merge) | Before severity tier policy. CX001/CX002 still strict-immediate. |
| `post_36` | `86732e2` (PR #36 merge) | After severity policy. CX001/CX002 demoted to Advisory. |
| `post_39` | `12085ea` (PR #39 merge) | After self-onboarding lockfile. The "agent-strict exits 0" claim's actual evidence. |
| `target` | `origin/main` HEAD at audit time (`2144368`, post-#46) | Where dogfood evidence currently stands. Includes PR #40 (CL paradigm) and PR #46 (Policy Guard). |

`target_ref` is the current `origin/main` HEAD used by the audit branch.
Policy Guard (#46) is referenced as related future protection; if it had
not been merged at audit time, it would not have been included in measured
dogfood state. It is currently merged, but does not retire any pre-existing
diagnostic, so it does not appear in per-rule counters.

For each ref, capture: total diagnostics, fatals, warnings, breakdown by
rule, breakdown by file. This is what backs every per-rule and per-PR
number in the audit.

### Layer 2 — Classify dispositions between adjacent refs

For each diagnostic present at `pre_36` and absent at `target`, classify by
deterministic inspection (lockfile diff + git log + source diff at the
symbol's path):

- Did the rule's tier change? → `suppressed_by_severity_tier`
- Did `Lockfile.exceptions[]` gain a covering entry? → `accepted_by_exception`
- Did `OT.converter_paths` / `DG.features[].public_api` / `CX.exempt_paths` / `acknowledged_empty` gain a matching pattern? → respective `suppressed_by_*`
- Did `default_max_*` rise? → `suppressed_by_budget_increase`
- Did `module_overrides` gain an entry covering the symbol? → `suppressed_by_override`
- Did the source code change in a way that drops the match? → `resolved_by_code` / `_deletion` / `_symbol_move`
- Couldn't determine? → `unknown` (becomes its own follow-up issue)

The classifier is a deterministic inspection, not LLM judgment. Every
classification cites the responsible commit/PR and the responsible lockfile
field.

#### Bulk-classification rule for CX001/CX002

The CX001/CX002 cluster is bulk-classified as a single
`suppressed_by_severity_tier` decision applied to all 134 diagnostics. The
audit cites the mechanism once and counts its contribution; it does not
list 134 individual function/module entries. Per-diagnostic enumeration is
only required if a CX001/CX002 finding has a *different* disposition (e.g.,
covered by `CX.exempt_paths`).

Smaller categories (the 14 exceptions, 2 MO overrides, 12 acknowledged_empty
entries, 5 DG features, 3 OT.converter_paths patterns, 2 CX.exempt_paths
patterns, and any `unknown`s) are hand-classified per-mechanism.

The top-level JSON `measurement_mode` field records this:

```
"measurement_mode": "bulk_cluster_for_uniform_cx_severity_tier; per-mechanism classification for lockfile suppressions"
```

### Layer 3 — PR #41 / #42 forensic accounting from PR text

Since these didn't land, ground-truth measurement isn't available. Each
rule_delta in the PR #41/#42 records is annotated:

```json
{
  "source": "PR description",
  "verified": false,
  "merged": false,
  "contributes_to_current_state": false
}
```

The audit explicitly states this distinction: PR #36/#39/target numbers
are measured; PR #41/#42 numbers are quoted from rejected PRs.

### Reproducibility

The audit doc closes with the exact `git checkout` + `cargo run` commands
for each ref. No new tooling is added. Policy Guard (#46) blocks future
drift, so a recurring tool isn't required.

## Per-PR forensic structure (audit Markdown spine)

Each PR gets a subsection in `2026-05-09-dogfood-audit.md`.

### PR #36 — severity tier policy

- Mechanism: `suppressed_by_severity_tier`
- Effect: 134 diagnostics demoted Fatal → Warning under `--agent-strict`
  (107 CX001 + 27 CX002, to be verified at `pre_36` and `post_36`)
- Phrasing: blocking status disappeared; diagnostics remained as warnings
- Verdict: `not_remediated_remaining_warning_debt`
- What "exit 0 under strict" actually meant: the rules were demoted, not
  the code fixed. CX001 and CX002 are still visible warnings;
  `CheckMode::elevate_when_actionable` returns Warning when no narrowing
  config is present.

### PR #39 — self-onboarding lockfile

- Primary mechanisms: `accepted_by_exception`, `suppressed_by_public_api`,
  `suppressed_by_converter_paths`, `suppressed_by_exempt_paths`,
  `suppressed_by_acknowledged_empty`, `suppressed_by_override`
- 47 OT canonicals → counted as `resolved_by_code` with the note
  *"authority was already present in source hints before the audited PR
  window; PR #39 persisted it into locus.lock"*. Source hints predate the
  audit window.
- 5 DG features with public_api → `suppressed_by_public_api` for any
  DG003/DG004 hits inside those API surfaces; legitimate declaration.
- 3 OT.converter_paths → 1 legitimate (`locus_rust::*`, adapter authority
  per ADR), 2 carve-outs (`*::tests::*`, `*::layer_detection_tests::*`).
- 2 MO.overrides → both with full debt metadata; flagged as
  `accepted_debt_with_metadata`.
- 2 CX.exempt_paths → no debt metadata; flagged as schema gap (issue #1).
- 14 lockfile.exceptions → all with `expires` + `reason`;
  `accepted_by_exception`.
- 12 acknowledged_empty paradigms → no debt metadata; flagged as schema
  gap (issue #2).
- Verdict: mixed. Most entries are legitimate onboarding; two surfaces
  (`CX.exempt_paths`, `acknowledged_empty`) lack debt metadata because
  the schema doesn't carry it.

### PR #41 — CX002 cleanup (CLOSED, NOT MERGED)

- `contributes_to_current_state: false`
- Two halves with different verdicts:
  - **Test extraction** (19 paradigm `rules.rs` → `rules_tests.rs`):
    legitimate refactor; would have been `resolved_by_code` for any CX002
    hit in those modules. Not landed; remains a viable refactor candidate
    (issue #3).
  - **CX.default_max_module_lines = 700 + 8 module_overrides**: would
    have been `suppressed_by_budget_increase` + `suppressed_by_override`.
- Verdict: `proposed_but_not_landed`. Real refactor and policy
  calibration were bundled in one PR; on any future re-attempt the audit
  recommends splitting.

### PR #42 — CX001 cleanup (CLOSED, NOT MERGED)

- `contributes_to_current_state: false`
- Pure calibration: `CX.default_max_function_lines = 120` + 6 per-file
  overrides. No code change.
- Would have been `suppressed_by_budget_increase` +
  `suppressed_by_override` for all 109 CX001 hits.
- Verdict: `proposed_but_not_landed`. The issue's framing is correct:
  this PR's "0 diagnostics" claim would have meant policy suppression,
  not remediation. Re-evaluation under Policy Guard (#46) would now fire
  PG001 + PG002 on this same shape.

## CLAUDE.md update

Replace the existing paragraph in "Project status":

> *Locus's own source is annotated. `locus check --workspace .` against the
> unconfigured repo (no `locus.lock` at the root) intentionally emits a
> mix of warnings (CX/MO/DC/ER) and `LOCUS002` advisories — those are the
> "noisy default" working as designed. Self-application clean-status now
> means **zero unexpected fatals**, not zero warnings.*

with:

> Self-application status is not "zero findings."
>
> Current dogfood status means: zero unexpected fatals under the current
> lockfile and severity policy. Known remaining surfaces include CX001/CX002
> warning debt, accepted lockfile exceptions, acknowledged-empty paradigms,
> declared public API / converter authority, and policy suppressions
> tracked in the dogfood audit.
>
> Snapshot numbers live in
> [`docs/superpowers/specs/2026-05-09-dogfood-audit.md`](docs/superpowers/specs/2026-05-09-dogfood-audit.md).
> Update that audit when changing policy or dogfood claims.
>
> Snapshot as of 2026-05-09: \<computed at implementation time —
> measured numbers only; no "likely" placeholder in the committed
> form\>.

The committed snapshot line uses measured numbers only. If a Layer 1
measurement at any ref fails (older refs may not build cleanly under
current toolchain — `cargo build` against `c479ce3` may surface
dependency-resolution drift), the audit doc records the partial result
with `"verified": false` on the failing ref, and the CLAUDE.md snapshot
line is omitted with the audit doc cited as the sole source of truth.

## Follow-up issues (all 6 opened via `gh issue create`)

Issues 1–4: high-priority. Labels: `dogfood`, `architecture`,
`high-priority`.

| # | Title |
|---|---|
| 1 | Design debt-metadata schema for `paradigms.CX.exempt_paths` |
| 2 | Design debt-metadata schema for `acknowledged_empty` |
| 3 | Re-land PR #41 test extraction without budget calibration |
| 4 | Re-evaluate PR #42 calibration with Policy Guard debt metadata |

Issues 5–6: non-blocking. Labels: `refactor`, `complexity-budget`,
`dogfood-debt`. Body includes:

> *This is a named refactor candidate from the dogfood audit, not a release
> blocker by itself.*

| # | Title |
|---|---|
| 5 | Refactor candidate: split `locus_rust::visitor::scan_expr` per AST variant |
| 6 | Refactor candidates from CX001 cluster: per-rule splits in FL/OT |

## Acceptance-criteria mapping (#45)

| AC | Where satisfied |
|---|---|
| AC1 — Dogfood status no longer reports only `0 fatal` or `0 diagnostics` | Audit doc § honest-status + CLAUDE.md update |
| AC2 — Audit separates real code fixes from policy/config suppressions | 15-class verdict taxonomy + per-rule counters |
| AC3 — PR #42 explicitly classified as policy suppression, not remediation | §PR-42 forensic, `proposed_but_not_landed` verdict |
| AC4 — PR #41 split/accounted as real test extraction plus policy calibration | §PR-41 forensic, two-halves treatment |
| AC5 — PR #39 lockfile/config changes classified by suppression type and reviewed for legitimacy | §PR-39 forensic, per-mechanism breakdown |
| AC6 — Report identifies which suppressions need debt metadata or follow-up refactor issues | Six follow-up issues (1–2 schema, 3–4 un-landed work, 5–6 refactor candidates) |
| AC7 — Final project status uses honest wording | CLAUDE.md update with measured snapshot |

## Relationship to #44 / #46

#45 is retrospective; #46 (Policy Guard, merged) is prospective. PG001–PG004
prevent the future appearance of:

- new `default_max_*` raises (PG001) → would now block what PR #42
  proposed
- new `module_overrides` (PG002) → would block what PR #41 and PR #42
  proposed
- new `exempt_paths` (PG003) → would block widening of CX exempt_paths
- new `acknowledged_empty` entries (PG004) → would block silent
  paradigm-level vacancy admissions

PG006 requires debt metadata on new overrides — confirming the schema-gap
issues #1 and #2 are real gaps to close, not invented requirements.

This audit closes the trust boundary on existing dogfood evidence; PG (#46)
keeps it closed going forward.
