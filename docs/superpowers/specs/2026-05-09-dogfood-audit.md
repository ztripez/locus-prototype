# Dogfood audit — narrative

**Issue:** [#45](https://github.com/ztripez/locus/issues/45). **Design spec:** [`2026-05-09-dogfood-audit-design.md`](2026-05-09-dogfood-audit-design.md). **Companion JSON:** [`2026-05-09-dogfood-audit.json`](2026-05-09-dogfood-audit.json).

**Date:** 2026-05-09. **Baseline ref:** `c479ce3` (pre-#36). **Target ref:** `2144368` (origin/main, post-#46).

---

## Honest project status (snapshot 2026-05-09, updated 2026-05-11)

- **Active fatals (under `--agent-strict`):** 0
- **Remaining warning debt:** 103 (64 CX001 + 39 CX002, all advisory tier; not blocking). Down from 143 at audit time after umbrella #51's refactor-first sweep.
- **Accepted debt (with metadata):** 16 entries — 14 lockfile exceptions + 2 MO overrides; all carry `expires` + `reason`
- **Policy suppressions (no debt metadata):** 13 — `acknowledged_empty` ×12 silencing LOCUS002 + `CX.exempt_paths` ×1 covering CX007. Tracked as schema gaps in follow-up issues.
- **Severity-tier demotions:** 133 — CX001 ×106 + CX002 ×27 demoted by PR #36; blocking status disappeared but diagnostics remained as warnings (unchanged historical fact)
- **Post-audit refactor delta:** CX001 dropped 49 hits (113 → 64) via real code splits; CX002 rose 9 hits (30 → 39) from PR #57's test extraction producing new `rules_tests.rs` files. Net post-refactor change: -40. Net delta vs pre_36 baseline: -30 (refactor sweep reversed earlier source-drift accumulation).

The "exit 0 under strict" claim is structurally honest: zero Fatal under current policy. It is *not* a "zero diagnostics" claim — 103 warnings remain visible. Policy Guard (PR #46) prevents future widening.

---

## Methodology

The audit is defensible only if a future engineer can reproduce its numbers. Three layers cover the measurement, classification, and forensic accounting.

### Layer 1 — Ground-truth measurement at four git refs

Each ref was checked out in an isolated worktree, built with a per-worktree `--target-dir`, and run under both default and `--agent-strict` modes. Diagnostic counts were captured by rule from stdout.

| Ref | Commit | What it represents |
|---|---|---|
| `pre_36` | `c479ce3` | PR #35 merge head — before severity tier policy. CX001/CX002 were still strict-immediate Fatal. |
| `post_36` | `86732e2` | PR #36 merge — severity policy applied. CX001/CX002 demoted to Advisory. No lockfile yet. |
| `post_39` | `12085ea` | PR #39 merge — self-onboarding lockfile. The "agent-strict exits 0" claim's actual evidence. |
| `target` | `2144368` | `origin/main` HEAD at audit time (post-#46 Policy Guard). |

`target_ref` is the current `origin/main` HEAD. Policy Guard (#46) is referenced as related prospective protection; it does not retire any pre-existing diagnostic and does not appear in per-rule counters.

### Layer 2 — Classify dispositions between adjacent refs

For each diagnostic present at `pre_36` and absent at `target`, disposition was classified by deterministic inspection: lockfile diff, git log, source diff at the symbol's path. Each classification cites the responsible PR and the responsible lockfile field.

The CX001/CX002 cluster is bulk-classified as a single `suppressed_by_severity_tier` decision applied to all 133 pre-baseline diagnostics. The audit cites the mechanism once. Per-diagnostic enumeration is only required if a CX001/CX002 finding has a different disposition.

Smaller categories (14 lockfile exceptions, 2 MO overrides, 12 `acknowledged_empty` entries, 5 DG features, 3 OT converter_paths patterns, 2 CX `exempt_paths` patterns) are hand-classified per-mechanism.

### Layer 3 — PR #41 / #42 forensic accounting from PR text

These PRs were not merged; ground-truth measurement is unavailable. Each rule_delta in their records is annotated `"source": "PR description"` and `"verified": false`. The audit states this distinction explicitly: PR #36 / #39 / target numbers are measured; PR #41 / #42 numbers are quoted from closed PRs.

### Build isolation requirement

Cargo's user-level `~/.cargo/config.toml` may set a global `target-dir` that is shared across worktrees and projects. With sccache or other compile caches in the mix, a fresh `cargo build` in a different worktree can return the previously-compiled binary without rebuilding. This produced a measurement artifact during the first audit run: 136 false-positive OT004 hits at `post_39` were emitted by a cached pre_36 binary that lacked PR #39's OT matcher upgrade.

Mitigations:

- Each measurement task in this audit ran `RUSTC_WRAPPER= cargo build --workspace --target-dir <worktree>/target` to force fresh isolated builds, then invoked the binary directly via `<worktree>/target/debug/locus`.
- The locus repo now ships a project-local `.cargo/config.toml` setting `target-dir = "target"` (cargo default) so every worktree gets its own `target/` by default. This applies going forward; historical refs (`pre_36`, `post_36`, `post_39`) do not have the file and require explicit `--target-dir` if re-measured.

---

## Per-rule disposition table

Generated from [`2026-05-09-dogfood-audit.json`](2026-05-09-dogfood-audit.json). `Before diagnostics` counts all diagnostics at `pre_36` regardless of severity tier (fatals, warnings, advisories — the JSON formerly called this `before_fatal`, which leaked the same severity assumption the audit critiques; renamed). `After fatal` and `After warning` are measured at `target`. The `Primary class` column is the largest non-zero disposition bucket per rule.

| Rule | Before diagnostics | After fatal | After warning | Post-audit refactor delta | Primary class | Verdict |
|---|---:|---:|---:|---:|---|---|
| CX001 | 106 | 0 | 64 | -49 | `suppressed_by_severity_tier` | not_remediated_remaining_warning_debt |
| CX002 | 27 | 0 | 39 | +9 | `suppressed_by_severity_tier` | not_remediated_remaining_warning_debt |
| CX007 | 1 | 0 | 0 | 0 | `suppressed_by_exempt_paths` | suppressed_no_debt_metadata |
| ER007 | 11 | 0 | 0 | 0 | `accepted_by_exception` | accepted_with_expires_and_reason |
| DC002 | 3 | 0 | 0 | 0 | `accepted_by_exception` | accepted_with_expires_and_reason |
| MO001 | 2 | 0 | 0 | 0 | `suppressed_by_override` | suppressed_with_full_debt_metadata |
| LOCUS002 | 13 | 0 | 0 | 0 | `suppressed_by_acknowledged_empty` | suppressed_no_debt_metadata |
| OT009 | 0 | 0 | 0 | `—` | pre_emptive_exception |
| OT_CANONICALS | 0 | 0 | 0 | `—` | resolved_by_code_source_hints_predated_audit_window |

**OT_CANONICALS** is not a rule; it is an aggregate entry for the 47 OT canonical type declarations persisted to `locus.lock` by PR #39. Authority was present in source hints before the audited PR window; no OT001/OT002 diagnostics fired at `pre_36`.

**OT009** is listed for completeness: 2 pre-emptive lockfile exceptions were added in PR #39 to block future false-positive recurrence on adapter-internal parser naming. Zero hits at `pre_36`.

---

## Per-PR forensics

### PR #36 — severity tier policy

**Merged:** yes. **Primary mechanism:** `suppressed_by_severity_tier`.

PR #36 changed the severity tier for CX001 and CX002 from strict-immediate Fatal to Advisory. No code was changed. No lockfile was created.

| Rule | Before diagnostics | After fatal | After warning | Class |
|---|---:|---:|---:|---|
| CX001 | 106 | 0 | 106 | `suppressed_by_severity_tier` |
| CX002 | 27 | 0 | 27 | `suppressed_by_severity_tier` |

**Effect:** 133 diagnostics demoted Fatal → Warning under `--agent-strict`. Blocking status disappeared; diagnostics remained as warnings. `CheckMode::elevate_when_actionable` returns Warning when no narrowing config is present, so CX001 and CX002 are still visible at every measured ref through `target`.

**Verdict:** `blocking_status_changed_diagnostics_remained`. What "exit 0 under strict" actually meant at `post_36`: the rules were demoted, not the code fixed. The "0 fatals" claim is accurate for the severity tier that PR #36 established; it is not a remediation claim.

**Post-baseline drift note:** Source code growth on main added 7 more CX001 hits and 3 more CX002 hits between the `post_39` lockfile commit and the audit `target` ref, so the audit-time `after_warning` counts (113 CX001 + 30 CX002) exceeded the `post_36` post-demotion counts (106 + 27). After the umbrella #51 refactor sweep (see "Post-audit refactor sweep" section below), CX001 dropped from 113 → 64 and CX002 rose from 30 → 39, for a net delta of -30 vs the pre_36 baseline.

---

### PR #39 — self-onboarding lockfile

**Merged:** yes. **Primary mechanism:** multiple (see breakdown below).

PR #39 landed the self-onboarding `locus.lock`. This is where most of the suppression surface was established. The audit classifies each mechanism separately.

#### OT canonicals — 47 entries

**Class:** `resolved_by_code` (source hints predated audit window).

47 OT canonical type declarations persisted to `locus.lock`. The authority (`// locus: ot canonical`) was already present in source hints before the audited PR window; PR #39 ran `locus init` and persisted it into the lockfile. No OT001/OT002 diagnostics fired at `pre_36` because source hints suppressed them. The lockfile declarations are legitimate: they represent accepted architectural authority, not new suppression.

#### DG features — 5 declarations

**Class:** `suppressed_by_public_api` / `resolved_by_code`.

5 DG feature blocks declared with `public_api` patterns — genuine architectural boundary declarations. These also resolve the `LOCUS002` vacancy nudge for DG: because DG now has definitions, the nudge no longer fires.

#### OT converter_paths — 3 patterns

**Class:** `suppressed_by_converter_paths`. **Split verdict:**

- 1 legitimate — `locus_rust::*`: adapter authority per ADR; the Rust language adapter is the designated converter layer.
- 2 carve-outs — `*::tests::*`, `*::layer_detection_tests::*`: test modules that construct types outside the normal domain path. Legitimate carve-outs for test code.

#### MO overrides — 2 entries (full debt metadata)

**Class:** `suppressed_by_override`. **Verdict:** `suppressed_with_full_debt_metadata`.

Both MO001 overrides carry full debt metadata (`expires`, `reason`, `owner`, `debt_id`, `introduced_by`):

- `locus_air` — 43 public types against a budget of 5; override raises to 50. Debt ID: `MO001-locus-air-canonical-data-crate`.
- `locus_core::paradigms::one_truth::lockfile_schema` — 7 public types against a budget of 5; override raises to 10. Debt ID: `MO001-ot-lockfile-schema-grouped-shape`.

Both expire 2027-05-09. This is the correct shape for suppression with debt metadata — PG006 confirms these satisfy the justification requirement.

#### CX exempt_paths — 2 entries (schema gap)

**Class:** `suppressed_by_exempt_paths`. **Verdict:** `suppressed_no_debt_metadata`.

`paradigms.CX.exempt_paths` entries: `*::tests::*` and `locus_air::*`. The `locus_air::*` pattern covers the single CX007 hit (43 public items in `locus_air`, budget 30). The `*::tests::*` pattern is a blanket test carve-out.

**Schema gap (follow-up issue #48):** `CX.exempt_paths` is `Vec<String>` with no `expires`, `reason`, `owner`, or `debt_id` fields. There is no way to attach debt metadata. The 2 entries are currently active suppressions with no expiry or justification trail.

#### Lockfile exceptions — 14 entries (full debt metadata)

**Class:** `accepted_by_exception`. **Verdict:** `accepted_with_expires_and_reason`.

14 lockfile exceptions total: 9 covering 11 ER007 hits + 3 covering 3 DC002 hits + 2 pre-emptive for OT009.

All carry `expires=2027-05-09` and documented `reason` text. This is the correct accepted-debt shape. The ER007 exceptions cover paradigm-scoped `*EditError` types with structurally duplicate variant names — the architectural justification (each `*EditError` is paradigm-scoped, not cross-paradigm taxonomy drift) is documented in the reason field.

#### acknowledged_empty — 12 paradigm prefixes (schema gap)

**Class:** `suppressed_by_acknowledged_empty`. **Verdict:** `suppressed_no_debt_metadata`.

12 paradigm prefixes in `acknowledged_empty`: BO, CF, CR, DA, ER, FL, FO, PA, RM, RW, TA, UT. Each silences one LOCUS002 vacancy nudge.

**Schema gap (follow-up issue #49):** `acknowledged_empty` is `Vec<String>` with no per-prefix metadata (`expires`, `reason`, `owner`). There is no way to attach a rationale or expiry to any of these suppressions.

#### Overall verdict

**Verdict:** `mixed_legitimate_onboarding_plus_two_schema_gaps`.

Most entries are legitimate onboarding: ER007/DC002/OT009 exceptions carry `expires`+`reason`, MO001 overrides carry full debt metadata, DG features are genuine architectural declarations, OT canonicals predate the audit window. Two surfaces (`CX.exempt_paths`, `acknowledged_empty`) lack debt metadata because the schema does not carry it — named schema gaps for follow-up issues #48 and #49.

---

### PR #41 — CX002 cleanup (closed, not merged)

**Merged:** no. **`contributes_to_current_state`: false.** **Primary mechanism:** `proposed_but_not_landed`.

PR #41 targeted CX002 with two distinct halves:

**Half 1 — test extraction (legitimate refactor):** 19 paradigm `rules.rs` files would have been split into `rules.rs` + `rules_tests.rs`, moving inline `mod tests {}` blocks out. This would have been `resolved_by_code` for any CX002 hits in those test-heavy modules. This half is viable as a standalone refactor — see follow-up issue #50.

**Half 2 — policy calibration:** `CX.default_max_module_lines = 700` (from default 400) + 8 per-module overrides. This would have been `suppressed_by_budget_increase` + `suppressed_by_override` for the remaining CX002 hits. Under Policy Guard (PR #46), this shape now requires PG001 (budget raise) + PG002 (new overrides) + PG006 (debt metadata on overrides). The calibration half cannot land without that metadata.

**Rule deltas (from PR description; `verified: false`):**

| Rule | Before diagnostics | Would have: after fatal | Would have: after warning | Class |
|---|---:|---:|---:|---|
| CX002 | 27 | 0 | 0 | `proposed_but_not_landed` |

**Verdict:** `proposed_but_not_landed`. The bundling of a legitimate refactor with policy calibration in one PR was the structural problem. On any future re-attempt, the audit recommends splitting: land the test extraction first (clean `resolved_by_code`), then evaluate the budget changes separately with debt metadata.

---

### PR #42 — CX001 cleanup (closed, not merged)

**Merged:** no. **`contributes_to_current_state`: false.** **Primary mechanism:** `proposed_but_not_landed`.

PR #42 was pure policy calibration with no code changes: `CX.default_max_function_lines = 120` (from default 50) + 6 per-file overrides. No code was modified.

**Rule deltas (from PR description; `verified: false`):**

| Rule | Before diagnostics | Would have: after fatal | Would have: after warning | Class |
|---|---:|---:|---:|---|
| CX001 | 106 | 0 | 0 | `proposed_but_not_landed` |

**Verdict:** `proposed_but_not_landed`. The PR's "0 diagnostics" claim would have meant `suppressed_by_budget_increase` + `suppressed_by_override`, not remediation. The issue's framing that this is a policy decision rather than a fix is correct.

Under Policy Guard (PR #46), this same shape would now fire PG001 (budget raise from 50 → 120) + PG002 (6 new overrides) + PG006 (missing debt metadata on new overrides). The calibration is not ruled out as a future direction, but it requires explicit debt metadata (`expires`, `reason`, `debt_id`) on each override — and a defensible `--allow-policy-calibration` flag invocation to downgrade PG001/PG002 from Fatal. Re-evaluation is tracked in follow-up issue #51.

---

## Schema gaps (tracked as follow-up issues)

- **`paradigms.CX.exempt_paths`** is `Vec<String>` with no `expires`, `reason`, `owner`, or `debt_id` fields. Currently 2 entries (`*::tests::*`, `locus_air::*`) silencing 1 known CX007 hit. → tracked as follow-up issue #48.
- **`acknowledged_empty`** is `Vec<String>` with no per-prefix metadata. Currently 12 paradigm prefixes silencing 12 LOCUS002 vacancy nudges (one per prefix). → tracked as follow-up issue #49.

PG006 (Policy Guard) requires debt metadata on new MO overrides — confirming these two surfaces are real gaps to close, not invented requirements.

---

## Refactor candidates (named, non-blocking)

- **Split `locus_rust::visitor::scan_expr` per AST variant** (~298 lines). The single largest CX001 contributor and the structural reason PR #42 proposed a per-file budget override on the visitor module. → tracked as follow-up issue #52.
- **Per-rule splits in `failure_lineage::rules` and `one_truth::rules`** — the two largest paradigm rule files. Splitting per-rule would let CX002 fire honestly without per-file overrides. → tracked as follow-up issue #53.

---

## Post-audit refactor sweep (umbrella #51)

After the audit was published (PR #54, 2026-05-09), umbrella issue #51 drove a refactor-first response to the CX001 warning debt. The policy stance was explicit: **no `default_max_function_lines` raise; no `module_overrides` except temporary + metadata-backed + unavoidable; CX001 stands at 50 as designed.** PR #42's budget-raise approach was rejected permanently.

Sub-issues landed:

- **#59 / PR #64** — split `emit_fn` and `emit_impl` in `locus_rust::visitor` per AST variant.
- **#60 / PR #70** — split >50-line paradigm rule functions; 21 paradigms refactored; CX001 in paradigm files dropped from 60 to 36 (target <40 met).
- **#61 / PR #65** — split CLI command handlers; 5 functions split; CX001 in `locus_cli` dropped from 5 to 0.
- **#52 / PR #56** — split `scan_expr` per AST variant (already in Wave 1).
- **#53 / PR #66** — per-rule splits in FL/OT (already in Wave 2).

Out-of-scope follow-ups deferred (not blockers for closing #51):

- **#67 / PR #68** — MO005 (entrypoint declaration ownership) landed as an architectural reinforcement alongside the CLI split.
- **#69** — lib.rs entrypoint handling for MO005 (separate design pass).
- **PR #77** — DG003 lockfile fix for `AcknowledgedEmpty` re-export (one-line public_api addition).
- **#62** — this audit doc update (Wave 4).

**Result:** CX001 dropped from 113 (audit time) → 64 (current). CX002 rose from 30 → 39 due to PR #57's test extraction producing new `rules_tests.rs` files (accepted advisory debt per the audit's prior framing). No `module_overrides`, no `default_max_function_lines` raise, no `// locus: allow CX001` source hints introduced anywhere in the sweep.

Refactor-first policy worked. CX001 dropped 49 hits via real code changes; CX002 grew 9 hits from the test-extraction structural effect; net warning debt down from 143 to 103. The remaining 64 CX001 + 39 CX002 = 103 stay as accepted advisory debt under the same dogfood-honest framing.

---

## Going forward

Policy Guard (PR #46, merged into `origin/main` at audit time) closes the prospective trust boundary:

- **PG001** blocks new `default_max_*` raises — what PR #42 proposed, what PR #41's second half proposed.
- **PG002** blocks new `module_overrides` / `overrides` entries without prior approval — what both PRs proposed.
- **PG003** blocks new `exempt_paths` entries — would block widening of the CX `exempt_paths` surface (schema gap #48).
- **PG004** blocks new `acknowledged_empty` entries — would block adding further vacancy suppressions without review (schema gap #49).
- **PG006** requires debt metadata on new overrides — confirms the schema gaps are real requirements, not invented.

The two schema gaps (#48, #49) are now blocked from expanding silently, but existing entries are not retroactively required to carry metadata. The follow-up issues (#48 and #49) track the schema work needed to retrofit debt metadata onto these surfaces.

---

## Reproducibility

To re-run the Layer 1 measurement at any of the four refs, use isolated builds (per-worktree `--target-dir` + `RUSTC_WRAPPER=` to bypass any host-level cargo config):

```bash
# Add a worktree at the ref.
git worktree add /tmp/locus-measure-pre_36 c479ce3

# Force fresh build.
cd /tmp/locus-measure-pre_36
RUSTC_WRAPPER= cargo build --workspace --target-dir /tmp/locus-measure-pre_36/target

# Run check, capturing default and strict mode.
./target/debug/locus check --workspace . > /tmp/dogfood-audit/pre_36-default.txt 2>&1
./target/debug/locus check --workspace . --agent-strict > /tmp/dogfood-audit/pre_36-strict.txt 2>&1

# Extract per-rule counts (bracketed format like [CX001]).
grep -oE '\[[A-Z]+[0-9]+\]' /tmp/dogfood-audit/pre_36-default.txt | sort | uniq -c | sort -rn
```

The four refs:

- `pre_36` = `c479ce3` (PR #35 merge; before #36 severity policy)
- `post_36` = `86732e2` (PR #36 merge; severity policy applied; no lockfile)
- `post_39` = `12085ea` (PR #39 merge; lockfile + matcher upgrade)
- `target` = `origin/main` HEAD (currently `2144368`, post-#46 Policy Guard)

The current main checkout (or any branch from main) is equivalent to `target` for source code state.

---

## Audit metadata

- `audit_baseline_ref`: `c479ce3` (pre-#36)
- `audit_target_ref`: `2144368` (origin/main HEAD, post-#46)
- `audit_date`: 2026-05-09
- `measurement_mode`: bulk_cluster_for_uniform_cx_severity_tier; per-mechanism classification for lockfile suppressions

Source of truth: companion JSON ([`2026-05-09-dogfood-audit.json`](2026-05-09-dogfood-audit.json)) and design spec ([`2026-05-09-dogfood-audit-design.md`](2026-05-09-dogfood-audit-design.md)).
