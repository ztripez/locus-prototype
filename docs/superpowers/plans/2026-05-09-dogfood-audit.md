# Dogfood Audit Implementation Plan

> **Status: historical / executed.** This plan was executed during the dogfood-audit work and is retained for audit traceability. The unchecked `- [ ]` boxes below are the original task list; they are **not** active project tasks. Don't re-execute. The audit deliverables this plan produced live alongside it: [`2026-05-09-dogfood-audit-design.md`](../specs/2026-05-09-dogfood-audit-design.md), [`2026-05-09-dogfood-audit.md`](../specs/2026-05-09-dogfood-audit.md), [`2026-05-09-dogfood-audit.json`](../specs/2026-05-09-dogfood-audit.json).
>
> *(Original plan header — for tooling that needs the agentic-workers cue:)* For agentic workers: REQUIRED SUB-SKILL: superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the dogfood audit defined in [`docs/superpowers/specs/2026-05-09-dogfood-audit-design.md`](../specs/2026-05-09-dogfood-audit-design.md) — a per-rule, per-PR forensic accounting that distinguishes real fixes from policy suppression, plus the CLAUDE.md honest-status update and six follow-up issues. Closes [#45](https://github.com/ztripez/locus/issues/45).

**Architecture:** Documentation-only PR. Three-layer methodology — (1) measure diagnostics at four git refs (`pre_36`, `post_36`, `post_39`, `target`) using throwaway worktrees, (2) classify dispositions by deterministic inspection (lockfile diff + git log + source diff), (3) accept PR-text numbers for the unmerged PR #41/#42 with `verified: false`. CX001/CX002 cluster bulk-classified as `suppressed_by_severity_tier`; other categories hand-classified per-mechanism. Six GitHub issues opened via `gh issue create`. No code changes, no schema changes, no rule-engine modifications.

**Tech Stack:** Markdown + JSON for the audit doc; bash + `cargo run -p locus-cli` for measurement; `git worktree` for ref isolation; `gh issue create` / `gh pr create` for follow-up tracking.

**Branch:** `dogfood-audit` (already created from `origin/main` at `2144368`; design spec already committed at `bd34ecf`).

**Guardrails (from `CLAUDE.md`):**
- Each git mutation in its own Bash call. No chaining mutating git steps with network calls.
- No destructive git operations (`reset --hard`, `branch -D`, force-push) without explicit confirmation.
- No commit `--no-verify` or hook-skip flags.
- No mention of Claude/AI assistants in commit messages, PR titles, or PR bodies.

---

## File structure

| Path | Action | Responsibility |
|---|---|---|
| `docs/superpowers/specs/2026-05-09-dogfood-audit.json` | NEW | Structured per-rule + per-PR records; arithmetically auditable counters. |
| `docs/superpowers/specs/2026-05-09-dogfood-audit.md` | NEW | Narrative audit: summary, per-rule table, per-PR forensics, honest-status, reproducibility. |
| `CLAUDE.md` | MODIFY | Replace one paragraph in "Project status" with dictated wording + measured snapshot. |
| `README.md` | NO CHANGE | Verified during brainstorming; does not overclaim. |
| `locus.lock` | NO CHANGE | No schema work in this PR. |
| GitHub issues | 6 NEW | Tracked via `gh issue create`. Issues 1–4 high-priority; issues 5–6 non-blocking. |

Measurement artifacts live in `/tmp/dogfood-audit/` (not committed). Worktrees live in `/tmp/locus-measure-{pre_36,post_36,post_39}/` (cleaned up in Task 16).

---

## Task 1: Prepare measurement scratch directory and worktrees

**Files:**
- Create (temp): `/tmp/dogfood-audit/`
- Create (temp worktrees): `/tmp/locus-measure-pre_36/`, `/tmp/locus-measure-post_36/`, `/tmp/locus-measure-post_39/`

- [ ] **Step 1: Create scratch directory.**

```bash
mkdir -p /tmp/dogfood-audit
ls /tmp/dogfood-audit
```

Expected: empty directory listing.

- [ ] **Step 2: Verify the four target refs exist locally.**

```bash
git rev-parse c479ce3 86732e2 12085ea origin/main
```

Expected: 4 SHAs printed, one per line. If `c479ce3` (or any other) is missing, run `git fetch origin` and retry. The 4th line is `2144368...` (origin/main HEAD).

- [ ] **Step 3: Add the three historical worktrees.**

Each worktree gets its own Bash call (no chaining of mutating git ops):

```bash
git worktree add /tmp/locus-measure-pre_36 c479ce3
```

```bash
git worktree add /tmp/locus-measure-post_36 86732e2
```

```bash
git worktree add /tmp/locus-measure-post_39 12085ea
```

Expected: each command prints `Preparing worktree (detached HEAD <sha>)`.

- [ ] **Step 4: Verify worktree state.**

```bash
git worktree list
```

Expected: 4+ entries — original repo, the audit branch, and the three measurement worktrees at the listed SHAs. Confirm the `dogfood-audit` working directory is undisturbed.

---

## Task 2: Measure diagnostic counts at `pre_36` (c479ce3)

**Files:**
- Read (in worktree): `/tmp/locus-measure-pre_36/`
- Write: `/tmp/dogfood-audit/pre_36-default.txt`, `/tmp/dogfood-audit/pre_36-strict.txt`

This ref is **before PR #36 severity tier policy** and **before PR #39 lockfile**. Expect Fatal-immediate severity for CX001/CX002. PR #36's description claims CX001 ×106 + CX002 ×27 = 133 at this baseline.

- [ ] **Step 1: Build at `pre_36`.**

```bash
cd /tmp/locus-measure-pre_36 && cargo build --workspace 2>&1 | tail -10
```

Expected: build succeeds with no errors. If build fails (toolchain/dependency drift), record the failure in `/tmp/dogfood-audit/pre_36-build-failure.txt` and **skip steps 2–5**; this ref will use PR-text fallback numbers in Task 7. Continue to Task 3.

- [ ] **Step 2: Run `locus check` in default mode.**

```bash
cd /tmp/locus-measure-pre_36 && cargo run -p locus-cli -- check --workspace . > /tmp/dogfood-audit/pre_36-default.txt 2>&1; echo "exit=$?"
```

Expected: `exit=0` or `exit=1`. Either is fine; the output file is what matters.

- [ ] **Step 3: Run `locus check` under `--agent-strict`.**

```bash
cd /tmp/locus-measure-pre_36 && cargo run -p locus-cli -- check --workspace . --agent-strict > /tmp/dogfood-audit/pre_36-strict.txt 2>&1; echo "exit=$?"
```

Expected: probably `exit=1` (CX001/CX002 fatals would block).

- [ ] **Step 4: Extract per-rule counts.**

```bash
grep -oE '\b(CX001|CX002|MO001|MO002|ER007|OT009|DC002|LOCUS002)\b' /tmp/dogfood-audit/pre_36-default.txt | sort | uniq -c | sort -rn
```

Expected: counts per rule. Save the output:

```bash
grep -oE '\b(CX001|CX002|MO001|MO002|ER007|OT009|DC002|LOCUS002)\b' /tmp/dogfood-audit/pre_36-default.txt | sort | uniq -c | sort -rn > /tmp/dogfood-audit/pre_36-counts.txt
cat /tmp/dogfood-audit/pre_36-counts.txt
```

If the rule pattern in the output differs (e.g., bracketed `[CX001]` or `CX001:` or different separator), adjust the regex. Verify by grepping a few sample lines:

```bash
head -20 /tmp/dogfood-audit/pre_36-default.txt
```

- [ ] **Step 5: Sanity-check against PR #36's stated baseline.**

PR #36's body claims "CX001 (×106) | Fatal" and "CX002 (×27) | Fatal" at this ref. Confirm the measured counts match (±1 is acceptable due to count drift between sub-commits). Record the comparison in `/tmp/dogfood-audit/pre_36-summary.txt`:

```bash
echo "pre_36 (c479ce3):" > /tmp/dogfood-audit/pre_36-summary.txt
echo "  PR #36 claim: CX001 ×106, CX002 ×27 (133 total)" >> /tmp/dogfood-audit/pre_36-summary.txt
echo "  measured:" >> /tmp/dogfood-audit/pre_36-summary.txt
cat /tmp/dogfood-audit/pre_36-counts.txt >> /tmp/dogfood-audit/pre_36-summary.txt
cat /tmp/dogfood-audit/pre_36-summary.txt
```

If measured counts diverge significantly from PR #36's claim (>5 difference), flag it as audit signal — the count drift is itself worth noting in Task 8's narrative.

---

## Task 3: Measure diagnostic counts at `post_36` (86732e2)

**Files:**
- Read (in worktree): `/tmp/locus-measure-post_36/`
- Write: `/tmp/dogfood-audit/post_36-default.txt`, `/tmp/dogfood-audit/post_36-strict.txt`, `/tmp/dogfood-audit/post_36-counts.txt`, `/tmp/dogfood-audit/post_36-summary.txt`

This ref is **after PR #36** but **before PR #39**. Severity tier policy active; no lockfile yet. Expect CX001/CX002 to be Warning (not Fatal) under both modes; `LOCUS002` not yet emitted (vacancy nudge added later).

- [ ] **Step 1: Build at `post_36`.**

```bash
cd /tmp/locus-measure-post_36 && cargo build --workspace 2>&1 | tail -10
```

Expected: build succeeds. On failure, record in `/tmp/dogfood-audit/post_36-build-failure.txt` and skip to Task 4 with PR-text fallback.

- [ ] **Step 2: Run `locus check` in default mode.**

```bash
cd /tmp/locus-measure-post_36 && cargo run -p locus-cli -- check --workspace . > /tmp/dogfood-audit/post_36-default.txt 2>&1; echo "exit=$?"
```

- [ ] **Step 3: Run `locus check` under `--agent-strict`.**

```bash
cd /tmp/locus-measure-post_36 && cargo run -p locus-cli -- check --workspace . --agent-strict > /tmp/dogfood-audit/post_36-strict.txt 2>&1; echo "exit=$?"
```

Expected: `exit=0` (CX001/CX002 demoted; should not block).

- [ ] **Step 4: Extract per-rule counts.**

```bash
grep -oE '\b(CX001|CX002|MO001|MO002|ER007|OT009|DC002|LOCUS002)\b' /tmp/dogfood-audit/post_36-default.txt | sort | uniq -c | sort -rn > /tmp/dogfood-audit/post_36-counts.txt
cat /tmp/dogfood-audit/post_36-counts.txt
```

- [ ] **Step 5: Sanity-check against PR #36's stated post-state.**

```bash
echo "post_36 (86732e2):" > /tmp/dogfood-audit/post_36-summary.txt
echo "  PR #36 claim: CX001 ×106 Warning, CX002 ×27 Warning" >> /tmp/dogfood-audit/post_36-summary.txt
echo "  measured:" >> /tmp/dogfood-audit/post_36-summary.txt
cat /tmp/dogfood-audit/post_36-counts.txt >> /tmp/dogfood-audit/post_36-summary.txt
cat /tmp/dogfood-audit/post_36-summary.txt
```

The CX001/CX002 *count* should be unchanged from `pre_36`; only the *severity* changed. If the count differs, that's drift between commits, not a #36 effect — note for Task 8.

---

## Task 4: Measure diagnostic counts at `post_39` (12085ea)

**Files:**
- Read (in worktree): `/tmp/locus-measure-post_39/`
- Write: `/tmp/dogfood-audit/post_39-default.txt`, `/tmp/dogfood-audit/post_39-strict.txt`, `/tmp/dogfood-audit/post_39-counts.txt`, `/tmp/dogfood-audit/post_39-summary.txt`

This ref is **after PR #39 self-onboarding lockfile**. The "agent-strict exits 0" claim's actual evidence. Expect 134 diagnostics (107 CX001 + 27 CX002 per PR #39's body), all Warning under strict, exit 0.

- [ ] **Step 1: Build at `post_39`.**

```bash
cd /tmp/locus-measure-post_39 && cargo build --workspace 2>&1 | tail -10
```

- [ ] **Step 2: Run default mode.**

```bash
cd /tmp/locus-measure-post_39 && cargo run -p locus-cli -- check --workspace . > /tmp/dogfood-audit/post_39-default.txt 2>&1; echo "exit=$?"
```

- [ ] **Step 3: Run strict mode.**

```bash
cd /tmp/locus-measure-post_39 && cargo run -p locus-cli -- check --workspace . --agent-strict > /tmp/dogfood-audit/post_39-strict.txt 2>&1; echo "exit=$?"
```

Expected: `exit=0` per PR #39's claim.

- [ ] **Step 4: Extract per-rule counts.**

```bash
grep -oE '\b(CX001|CX002|MO001|MO002|ER007|OT009|DC002|LOCUS002)\b' /tmp/dogfood-audit/post_39-default.txt | sort | uniq -c | sort -rn > /tmp/dogfood-audit/post_39-counts.txt
cat /tmp/dogfood-audit/post_39-counts.txt
```

- [ ] **Step 5: Sanity-check.**

```bash
echo "post_39 (12085ea):" > /tmp/dogfood-audit/post_39-summary.txt
echo "  PR #39 claim: CX001 ×107, CX002 ×27 (134 total Warning, exit 0 strict)" >> /tmp/dogfood-audit/post_39-summary.txt
echo "  measured:" >> /tmp/dogfood-audit/post_39-summary.txt
cat /tmp/dogfood-audit/post_39-counts.txt >> /tmp/dogfood-audit/post_39-summary.txt
cat /tmp/dogfood-audit/post_39-summary.txt
```

Note the count drift from `post_36` to `post_39` (CX001 went from ~106 to ~107). That's source code added between merges, not a #39 effect.

---

## Task 5: Measure diagnostic counts at `target` (origin/main 2144368)

**Files:**
- Read (in current working tree on `dogfood-audit` branch — same commit as `origin/main`)
- Write: `/tmp/dogfood-audit/target-default.txt`, `/tmp/dogfood-audit/target-strict.txt`, `/tmp/dogfood-audit/target-counts.txt`, `/tmp/dogfood-audit/target-summary.txt`

The current dogfood state. The `dogfood-audit` branch was created from `origin/main`, so the working tree IS this ref. No worktree needed.

- [ ] **Step 1: Confirm branch state.**

```bash
git rev-parse HEAD
git status
```

Expected: HEAD is `bd34ecf` (the design spec commit). The audit branch is `bd34ecf` = `origin/main` (`2144368`) + the design spec commit. The design spec doesn't touch source code, so measurements at this ref reflect post-#46 dogfood state. Confirm `git status` is clean (or only shows the in-progress audit work).

- [ ] **Step 2: Build at HEAD.**

```bash
cargo build --workspace 2>&1 | tail -10
```

Expected: build succeeds.

- [ ] **Step 3: Run default mode.**

```bash
cargo run -p locus-cli -- check --workspace . > /tmp/dogfood-audit/target-default.txt 2>&1; echo "exit=$?"
```

- [ ] **Step 4: Run strict mode.**

```bash
cargo run -p locus-cli -- check --workspace . --agent-strict > /tmp/dogfood-audit/target-strict.txt 2>&1; echo "exit=$?"
```

Expected: `exit=0` under strict (claimed dogfood evidence).

- [ ] **Step 5: Extract per-rule counts.**

```bash
grep -oE '\b(CX001|CX002|MO001|MO002|ER007|OT009|DC002|LOCUS002|CL001|PG[0-9]{3})\b' /tmp/dogfood-audit/target-default.txt | sort | uniq -c | sort -rn > /tmp/dogfood-audit/target-counts.txt
cat /tmp/dogfood-audit/target-counts.txt
```

The regex now includes `CL001` (PR #40 added Claim Ownership paradigm) and `PG###` (PR #46 Policy Guard rules) since both merged after #39.

- [ ] **Step 6: Sanity-check.**

```bash
echo "target (origin/main 2144368, post-#46):" > /tmp/dogfood-audit/target-summary.txt
echo "  measured:" >> /tmp/dogfood-audit/target-summary.txt
cat /tmp/dogfood-audit/target-counts.txt >> /tmp/dogfood-audit/target-summary.txt
echo "" >> /tmp/dogfood-audit/target-summary.txt
echo "  exit codes: default=$(grep -c 'exit=' /tmp/dogfood-audit/target-default.txt), strict=$(tail -1 /tmp/dogfood-audit/target-strict.txt)" >> /tmp/dogfood-audit/target-summary.txt
cat /tmp/dogfood-audit/target-summary.txt
```

If `target` strict mode does *not* exit 0, that's a finding the audit must explain (the dogfood claim is presently broken). Note for Task 8.

---

## Task 6: Capture lockfile diffs between adjacent refs

**Files:**
- Write: `/tmp/dogfood-audit/lockfile-pre_36.lock`, `/tmp/dogfood-audit/lockfile-post_36.lock`, `/tmp/dogfood-audit/lockfile-post_39.lock`, `/tmp/dogfood-audit/lockfile-target.lock`, `/tmp/dogfood-audit/lockfile-diff-39-vs-target.txt`

The lockfile diffs are the substrate for classifying lockfile-based suppressions in Task 7.

- [ ] **Step 1: Capture lockfile state at each ref.**

```bash
git show c479ce3:locus.lock > /tmp/dogfood-audit/lockfile-pre_36.lock 2>/dev/null || echo "(no locus.lock at pre_36)" > /tmp/dogfood-audit/lockfile-pre_36.lock
git show 86732e2:locus.lock > /tmp/dogfood-audit/lockfile-post_36.lock 2>/dev/null || echo "(no locus.lock at post_36)" > /tmp/dogfood-audit/lockfile-post_36.lock
git show 12085ea:locus.lock > /tmp/dogfood-audit/lockfile-post_39.lock
git show origin/main:locus.lock > /tmp/dogfood-audit/lockfile-target.lock
```

Expected: pre_36 and post_36 captures should print the "(no locus.lock)" placeholder (PR #39 introduced the lockfile). post_39 and target captures should be ~590 lines each.

- [ ] **Step 2: Verify the lockfile capture.**

```bash
wc -l /tmp/dogfood-audit/lockfile-*.lock
```

Expected: pre_36 and post_36 show 1 line (the placeholder); post_39 and target show ~590 lines.

- [ ] **Step 3: Diff post_39 vs target lockfile.**

```bash
diff /tmp/dogfood-audit/lockfile-post_39.lock /tmp/dogfood-audit/lockfile-target.lock > /tmp/dogfood-audit/lockfile-diff-39-vs-target.txt
wc -l /tmp/dogfood-audit/lockfile-diff-39-vs-target.txt
cat /tmp/dogfood-audit/lockfile-diff-39-vs-target.txt
```

Expected: small diff (or empty) — between #39 merge and current main, the only merged work is PR #40 (Claim Ownership; may or may not touch lockfile) and PR #46 (Policy Guard; may add `paradigms.PG` config). If the diff is small, PR #39's lockfile is essentially the current state; if large, the audit must account for additional suppressions.

- [ ] **Step 4: Note the count of lockfile policy fields at target.**

Capture the discrete decisions in the current lockfile for cross-reference with audit counts:

```bash
echo "lockfile policy at target:" > /tmp/dogfood-audit/lockfile-policy-summary.txt
echo "  OT canonicals: $(grep -c '"source": "hint"' /tmp/dogfood-audit/lockfile-target.lock)" >> /tmp/dogfood-audit/lockfile-policy-summary.txt
echo "  DG features: $(grep -c '"name":' /tmp/dogfood-audit/lockfile-target.lock)" >> /tmp/dogfood-audit/lockfile-policy-summary.txt
echo "  OT converter_paths entries: $(grep -A 100 'converter_paths' /tmp/dogfood-audit/lockfile-target.lock | grep -c '^      "[^"]*",$' || echo 0)" >> /tmp/dogfood-audit/lockfile-policy-summary.txt
echo "  MO overrides: $(grep -c '"max_public_types"' /tmp/dogfood-audit/lockfile-target.lock)" >> /tmp/dogfood-audit/lockfile-policy-summary.txt
echo "  CX exempt_paths entries: $(grep -A 5 '"exempt_paths"' /tmp/dogfood-audit/lockfile-target.lock | grep -c '^      "[^"]*",$' || echo 0)" >> /tmp/dogfood-audit/lockfile-policy-summary.txt
echo "  exceptions: $(grep -c '"rule":' /tmp/dogfood-audit/lockfile-target.lock)" >> /tmp/dogfood-audit/lockfile-policy-summary.txt
echo "  acknowledged_empty paradigms: $(grep -A 20 'acknowledged_empty' /tmp/dogfood-audit/lockfile-target.lock | grep -c '^    "[A-Z][A-Z]"' || echo 0)" >> /tmp/dogfood-audit/lockfile-policy-summary.txt
cat /tmp/dogfood-audit/lockfile-policy-summary.txt
```

These greps are heuristic — adjust if they undercount. Cross-check against the design spec's stated counts (47 OT canonicals, 5 DG features, 3 converter_paths, 2 MO overrides, 2 exempt_paths, 14 exceptions, 12 acknowledged_empty).

---

## Task 7: Build the audit JSON file

**Files:**
- Read: `/tmp/dogfood-audit/*-counts.txt`, `/tmp/dogfood-audit/lockfile-policy-summary.txt`, `/tmp/dogfood-audit/lockfile-target.lock`, `/tmp/dogfood-audit/post_39-summary.txt`, `/tmp/dogfood-audit/target-summary.txt`
- Create: `docs/superpowers/specs/2026-05-09-dogfood-audit.json`

Following the JSON shape defined in the design spec §"JSON shape". All counters are mutually-exclusive; `before_diagnostics == sum(all classes)` for each rule record.

- [ ] **Step 1: Compute the per-rule disposition counts.**

For each rule that has any non-zero count at any ref, compute the row:

- **CX001:** `before_diagnostics = pre_36 count`, `after_fatal = 0`, `after_warning = target count`, `suppressed_by_severity_tier = before_diagnostics` (all CX001 demotions are bulk-classified to severity tier per the design spec). If `target count != before_diagnostics`, the difference goes to `resolved_by_code` (deletion of code) or `unknown` (further investigation needed). If `target count > before_diagnostics`, the difference goes to `remaining_warning_debt` (new warnings appeared post-baseline; not part of dogfood "fix" claim).
- **CX002:** same shape as CX001.
- **ER007:** `before_diagnostics = pre_36 count`, `after_fatal = target count`, `accepted_by_exception = (pre_36 count - target count)` (the 9 ER007 lockfile exceptions). If the exception list covers fewer hits than were silenced, the remainder is `resolved_by_code`.
- **OT009:** same shape — 2 lockfile exceptions account for 2 silenced hits.
- **DC002:** same shape — 3 lockfile exceptions account for 3 silenced hits.
- **MO001:** `accepted_by_exception` counts hits silenced by `MO.overrides` (2 entries with full debt metadata).
- **LOCUS002:** `before_diagnostics = post_36 count` (LOCUS002 added by PR's introducing vacancy nudge — confirm at post_36 vs pre_36), `after_warning = target count`, `suppressed_by_acknowledged_empty = (before - target)` covering the 12 paradigm prefixes.

For rules where the measurement at `pre_36` failed (build failure), use PR-text fallback numbers and set `verified: false` on the per-rule record.

- [ ] **Step 2: Compute the totals block.**

```
active_fatals          = target strict-mode fatal count (probably 0)
active_warnings        = target default-mode warning count
accepted_debt_entries  = 14 (lockfile.exceptions) + 2 (MO.overrides) = 16
policy_suppressions    = (CX exempt_paths effects) + (OT converter_paths effects) + (DG public_api effects) + (acknowledged_empty effects)
severity_tier_demotions = CX001 + CX002 + LOCUS002 demotion counts
remaining_warning_debt = sum of per-rule remaining_warning_debt fields
```

- [ ] **Step 3: Build the per-PR records.**

PR #36: `merged: true`, `primary_mechanism: "suppressed_by_severity_tier"`, rule_deltas covering CX001 + CX002 with measured pre_36 and post_36 counts.
PR #39: `merged: true`, `primary_mechanism` is a *list* of 6 (per design spec); rule_deltas covering ER007 (×9 exception), OT009 (×2 exception), DC002 (×3 exception), MO001 (×2 override), LOCUS002 (×12 acknowledged_empty), plus the 47 OT canonicals folded into `resolved_by_code` with the design-spec note.
PR #41: `merged: false`, `primary_mechanism: "proposed_but_not_landed"`, `contributes_to_current_state: false`. Rule_deltas (test-extraction half + calibration half) sourced from PR description with `verified: false`.
PR #42: same shape as #41 but for CX001 calibration.

- [ ] **Step 4: Write the JSON.**

Create `docs/superpowers/specs/2026-05-09-dogfood-audit.json` with the full structure. Use this skeleton (fill counts from Steps 1–3):

```jsonc
{
  "audit_baseline_ref": "c479ce3",
  "audit_target_ref": "<HEAD SHA from target measurement>",
  "audit_date": "2026-05-09",
  "methodology": "see audit doc §Methodology",
  "measurement_mode": "bulk_cluster_for_uniform_cx_severity_tier; per-mechanism classification for lockfile suppressions",
  "verdict_taxonomy": [
    "resolved_by_code",
    "resolved_by_deletion",
    "resolved_by_symbol_move",
    "accepted_by_exception",
    "suppressed_by_public_api",
    "suppressed_by_converter_paths",
    "suppressed_by_exempt_paths",
    "suppressed_by_acknowledged_empty",
    "suppressed_by_budget_increase",
    "suppressed_by_override",
    "suppressed_by_severity_tier",
    "remaining_warning_debt",
    "remaining_fatal",
    "proposed_but_not_landed",
    "unknown"
  ],
  "totals": {
    "active_fatals": <int>,
    "active_warnings": <int>,
    "accepted_debt_entries": 16,
    "policy_suppressions": <int>,
    "severity_tier_demotions": <int>,
    "remaining_warning_debt": <int>
  },
  "rules": [
    {
      "rule": "CX001",
      "before_diagnostics": <int>,
      "after_fatal": 0,
      "after_warning": <int>,
      "resolved_by_code": 0,
      "resolved_by_deletion": 0,
      "resolved_by_symbol_move": 0,
      "accepted_by_exception": 0,
      "suppressed_by_public_api": 0,
      "suppressed_by_converter_paths": 0,
      "suppressed_by_exempt_paths": 0,
      "suppressed_by_acknowledged_empty": 0,
      "suppressed_by_budget_increase": 0,
      "suppressed_by_override": 0,
      "suppressed_by_severity_tier": <int>,
      "remaining_warning_debt": <int>,
      "remaining_fatal": 0,
      "proposed_but_not_landed": 0,
      "unknown": 0,
      "verdict": "not_remediated_remaining_warning_debt",
      "responsible_policy": [
        { "field": "rule_severity_tier", "source": "PR #36", "ref": "docs/PARADIGMS.md §Severity tiers" }
      ],
      "findings": []
    }
    /* ... CX002, ER007, OT009, DC002, MO001, LOCUS002, plus any others surfaced by measurement ... */
  ],
  "prs": [
    {
      "pr": 36,
      "title": "Severity tier policy + CX001/CX002 advisory elevation",
      "merged": true,
      "merged_at": "2026-05-09T14:47:31Z",
      "primary_mechanism": "suppressed_by_severity_tier",
      "rule_deltas": [
        { "rule": "CX001", "before_diagnostics": <int>, "after_fatal": 0, "after_warning": <int>, "class": "suppressed_by_severity_tier" },
        { "rule": "CX002", "before_diagnostics": <int>, "after_fatal": 0, "after_warning": <int>, "class": "suppressed_by_severity_tier" }
      ],
      "verdict": "blocking_status_changed_diagnostics_remained",
      "notes": "PR #36 changed severity tier; diagnostics did not disappear."
    },
    {
      "pr": 39,
      "title": "Locus self-onboarding completion",
      "merged": true,
      "merged_at": "2026-05-09T15:59:44Z",
      "primary_mechanism": [
        "accepted_by_exception",
        "suppressed_by_public_api",
        "suppressed_by_converter_paths",
        "suppressed_by_exempt_paths",
        "suppressed_by_acknowledged_empty",
        "suppressed_by_override"
      ],
      "rule_deltas": [
        { "rule": "ER007",    "class": "accepted_by_exception",        "count": 9 },
        { "rule": "OT009",    "class": "accepted_by_exception",        "count": 2 },
        { "rule": "DC002",    "class": "accepted_by_exception",        "count": 3 },
        { "rule": "MO001",    "class": "suppressed_by_override",        "count": 2 },
        { "rule": "LOCUS002", "class": "suppressed_by_acknowledged_empty", "count": 12 }
      ],
      "verdict": "mixed_legitimate_onboarding_plus_two_schema_gaps",
      "notes": "47 OT canonicals folded into resolved_by_code (source hints predate audit window). CX.exempt_paths and acknowledged_empty lack debt metadata; tracked as schema gaps in follow-up issues #1 and #2."
    },
    {
      "pr": 41,
      "title": "CX002 cleanup: extract tests + calibrate module budgets",
      "merged": false,
      "primary_mechanism": "proposed_but_not_landed",
      "contributes_to_current_state": false,
      "rule_deltas": [
        { "rule": "CX002", "class": "would_have_resolved_by_code",        "count": "<from PR text>", "source": "PR description", "verified": false, "note": "test-extraction half" },
        { "rule": "CX002", "class": "would_have_suppressed_by_budget_increase", "count": "<from PR text>", "source": "PR description", "verified": false, "note": "default_max_module_lines = 700" },
        { "rule": "CX002", "class": "would_have_suppressed_by_override",   "count": "<from PR text>", "source": "PR description", "verified": false, "note": "8 module_overrides" }
      ],
      "verdict": "proposed_but_not_landed",
      "notes": "Test extraction is a viable refactor candidate (issue #3). Calibration half would now fire PG001/PG002 under Policy Guard."
    },
    {
      "pr": 42,
      "title": "CX001 cleanup: calibrate per-function budget",
      "merged": false,
      "primary_mechanism": "proposed_but_not_landed",
      "contributes_to_current_state": false,
      "rule_deltas": [
        { "rule": "CX001", "class": "would_have_suppressed_by_budget_increase", "count": "<from PR text: 109>", "source": "PR description", "verified": false, "note": "default_max_function_lines = 120" },
        { "rule": "CX001", "class": "would_have_suppressed_by_override",   "count": "<from PR text: 6 modules>", "source": "PR description", "verified": false, "note": "6 per-file overrides" }
      ],
      "verdict": "proposed_but_not_landed",
      "notes": "Pure calibration; no code change. Re-evaluation tracked in issue #4 with debt metadata under Policy Guard regime."
    }
  ]
}
```

- [ ] **Step 5: Validate JSON syntax.**

```bash
python3 -m json.tool docs/superpowers/specs/2026-05-09-dogfood-audit.json > /dev/null && echo "valid JSON"
```

Expected: `valid JSON`. If invalid, fix syntax and re-validate.

- [ ] **Step 6: Verify the per-rule arithmetic invariant.**

For each rule record, confirm `before_diagnostics == sum(all class counters)`. The skeleton above puts CX001's full count into `suppressed_by_severity_tier` + `remaining_warning_debt`. If the sum doesn't match, classification is incomplete — find the unaccounted hits and assign them to `unknown` (which becomes a follow-up issue).

```bash
python3 <<'PY'
import json
with open("docs/superpowers/specs/2026-05-09-dogfood-audit.json") as f:
    audit = json.load(f)
class_keys = [
    "resolved_by_code","resolved_by_deletion","resolved_by_symbol_move",
    "accepted_by_exception","suppressed_by_public_api",
    "suppressed_by_converter_paths","suppressed_by_exempt_paths",
    "suppressed_by_acknowledged_empty","suppressed_by_budget_increase",
    "suppressed_by_override","suppressed_by_severity_tier",
    "remaining_warning_debt","remaining_fatal","proposed_but_not_landed","unknown",
]
ok = True
for r in audit["rules"]:
    s = sum(r.get(k, 0) for k in class_keys)
    bf = r.get("before_diagnostics", 0)
    if s != bf:
        print(f"MISMATCH {r['rule']}: before_diagnostics={bf} sum_of_classes={s}")
        ok = False
print("invariant holds" if ok else "FIX UNCLASSIFIED HITS")
PY
```

Expected: `invariant holds`. If `MISMATCH`, classify the gap.

- [ ] **Step 7: Commit.**

```bash
git add docs/superpowers/specs/2026-05-09-dogfood-audit.json
```

```bash
git commit -m "$(cat <<'EOF'
docs(audit): add dogfood-audit JSON — per-rule + per-PR forensic accounting

Structured records for the dogfood audit. 15-class verdict taxonomy
(locked in design spec). Counters are mutually-exclusive disposition
buckets: before_diagnostics == sum(all classes) for each rule record.

CX001/CX002 cluster bulk-classified as suppressed_by_severity_tier
(PR #36 demotion); ER007/OT009/DC002 hits classified as
accepted_by_exception (lockfile exceptions with expires + reason);
MO001 as suppressed_by_override (2 MO.overrides with full debt
metadata); LOCUS002 as suppressed_by_acknowledged_empty (12
paradigm prefixes — schema gap flagged for follow-up).

PR #41/#42 records carry merged: false, contributes_to_current_state:
false, rule_deltas with source: "PR description", verified: false.

Refs: #45
EOF
)"
```

---

## Task 8: Build the audit Markdown file

**Files:**
- Read: `docs/superpowers/specs/2026-05-09-dogfood-audit.json`, all `/tmp/dogfood-audit/*-summary.txt`
- Create: `docs/superpowers/specs/2026-05-09-dogfood-audit.md`

The narrative version. Lead with summary + per-rule table, then per-PR forensics, then honest-status, close with reproducibility commands.

- [ ] **Step 1: Draft the audit Markdown skeleton.**

Create `docs/superpowers/specs/2026-05-09-dogfood-audit.md` with this structure:

```markdown
# Dogfood Audit (2026-05-09)

**Purpose:** Reclassify each diagnostic's disposition across the dogfood-relevant PR sequence (#36, #39, #41, #42) so the dogfood claim is no longer a single binary "exit 0 under strict" but a counted breakdown of real fixes vs. policy suppressions vs. accepted debt.

**Source of truth:** This document and the companion JSON ([`2026-05-09-dogfood-audit.json`](2026-05-09-dogfood-audit.json)). When dogfood policy or claims change, update the audit and CLAUDE.md snapshot together.

**Issue:** [#45](https://github.com/ztripez/locus/issues/45). **Design spec:** [`2026-05-09-dogfood-audit-design.md`](2026-05-09-dogfood-audit-design.md).

## Honest project status (snapshot 2026-05-09)

- **Active fatals (under `--agent-strict`):** <N from target measurement>
- **Remaining warning debt:** <N — primarily CX001/CX002 demoted by PR #36 severity tier policy>
- **Accepted debt (with metadata):** 16 entries — 14 lockfile exceptions + 2 MO overrides
- **Policy suppressions (no debt metadata):** <N — primarily CX.exempt_paths and acknowledged_empty surfaces; tracked as schema gaps in follow-up issues #1, #2>
- **Severity-tier demotions:** <N — CX001 + CX002, blocking status changed but diagnostics remained as warnings>

The "exit 0 under strict" claim is structurally honest given current policy, but it is *not* a "zero diagnostics" claim. CX001 and CX002 warnings are still emitted; they are non-blocking by design (advisory tier, not narrowed by lockfile). Policy Guard (#46) prevents future widening.

## Per-rule disposition table

<table from JSON; columns: rule, before_diagnostics, after_fatal, after_warning, primary_class, verdict>

## Per-PR forensic accounting

### PR #36 — severity tier policy (merged 2026-05-09)

Mechanism: `suppressed_by_severity_tier`. Effect: <N> diagnostics demoted Fatal → Warning under `--agent-strict`. Phrasing: blocking status disappeared; diagnostics remained as warnings. Verdict: `not_remediated_remaining_warning_debt`.

What "exit 0 under strict" actually meant after this PR: the rules were demoted, not the code fixed. CX001 and CX002 are still visible warnings; `CheckMode::elevate_when_actionable` returns Warning when no narrowing config is present.

### PR #39 — self-onboarding lockfile (merged 2026-05-09)

Primary mechanisms: `accepted_by_exception`, `suppressed_by_public_api`, `suppressed_by_converter_paths`, `suppressed_by_exempt_paths`, `suppressed_by_acknowledged_empty`, `suppressed_by_override`.

- **47 OT canonicals** → `resolved_by_code` with note: *authority was already present in source hints before the audited PR window; PR #39 persisted it into locus.lock*.
- **5 DG features with public_api** → `suppressed_by_public_api` for any DG003/DG004 hits inside those API surfaces; legitimate declaration.
- **3 OT.converter_paths** → 1 legitimate (`locus_rust::*`, adapter authority per ADR), 2 carve-outs (`*::tests::*`, `*::layer_detection_tests::*`).
- **2 MO.overrides** → both with full debt metadata (`expires`, `reason`, `owner`, `debt_id`); `accepted_by_exception` shape.
- **2 CX.exempt_paths** → no debt metadata; flagged as schema gap (issue #1).
- **14 lockfile.exceptions** → all with `expires` + `reason`; `accepted_by_exception`.
- **12 acknowledged_empty paradigms** → no debt metadata; flagged as schema gap (issue #2).

Verdict: mixed legitimate onboarding plus two schema gaps. Most entries are legitimate; `CX.exempt_paths` and `acknowledged_empty` lack debt metadata because the schema doesn't carry it — not a bypass attempt.

### PR #41 — CX002 cleanup (CLOSED, NOT MERGED)

Contributes to current state: **no**. Rule_deltas sourced from PR description with `verified: false`.

Two halves with different verdicts:

- **Test extraction** (19 paradigm `rules.rs` → `rules_tests.rs`): legitimate refactor; would have been `resolved_by_code` for CX002 hits in those modules. Remains a viable candidate (issue #3).
- **`CX.default_max_module_lines = 700` + 8 `module_overrides`**: would have been `suppressed_by_budget_increase` + `suppressed_by_override`.

Verdict: `proposed_but_not_landed`. The PR bundled real refactor with policy calibration; on any future re-attempt the audit recommends splitting.

### PR #42 — CX001 cleanup (CLOSED, NOT MERGED)

Contributes to current state: **no**. Pure calibration: `CX.default_max_function_lines = 120` + 6 per-file overrides. No code change. Would have been `suppressed_by_budget_increase` + `suppressed_by_override` for all 109 CX001 hits.

Verdict: `proposed_but_not_landed`. The issue's framing is correct: this PR's "0 diagnostics" claim would have meant policy suppression, not remediation. Re-evaluation under Policy Guard (#46) would now fire PG001 + PG002 on this same shape (issue #4).

## Schema gaps (tracked in follow-up issues)

- **`paradigms.CX.exempt_paths`** carries no debt metadata (no `expires`, `reason`, `owner`, `debt_id`). Each pattern is a no-accountability suppression surface. → issue #1.
- **`acknowledged_empty`** is `Vec<String>`; carries no per-prefix metadata. Vacancy-silence is undated and unowned. → issue #2.

PG006 (Policy Guard) currently requires debt metadata on new MO.overrides — confirming these schema gaps are real, not invented.

## Refactor candidates (tracked in follow-up issues, non-blocking)

- **Split `locus_rust::visitor::scan_expr` per AST variant** (~298 lines; the largest CX001 contributor at the time of PR #42's calibration proposal). → issue #5.
- **Split `failure_lineage::rules` and `one_truth::rules` per rule** (largest paradigm rule files). → issue #6.

## Methodology

(Reproduce the §Methodology section from the design spec — Layer 1 measurement at four refs, Layer 2 deterministic disposition classification with the bulk-cluster rule for CX001/CX002, Layer 3 PR-text accounting for unmerged PRs.)

## Reproducibility

```bash
# Layer 1 — measure at four refs.
git worktree add /tmp/locus-measure-pre_36 c479ce3
cd /tmp/locus-measure-pre_36 && cargo run -p locus-cli -- check --workspace . > pre_36.txt
# (repeat for 86732e2, 12085ea, origin/main)

# Layer 2 — diff lockfile and severity policy.
git show 12085ea:locus.lock > post_39.lock
git show origin/main:locus.lock > target.lock
diff post_39.lock target.lock
```

## Audit metadata

- `audit_baseline_ref`: `c479ce3` (pre-#36)
- `audit_target_ref`: `<HEAD SHA>` (origin/main, post-#46)
- `audit_date`: 2026-05-09
- `measurement_mode`: `bulk_cluster_for_uniform_cx_severity_tier; per-mechanism classification for lockfile suppressions`
```

- [ ] **Step 2: Fill in measured values.**

Replace each `<N>` placeholder with the actual count from the JSON file (Task 7). Replace `<HEAD SHA>` with the value of `audit_target_ref` from the JSON. The honest-status snapshot block, the per-rule table, and the per-PR forensic counts all draw from `2026-05-09-dogfood-audit.json`.

- [ ] **Step 3: Build the per-rule disposition table.**

From the JSON's `rules` array, render a Markdown table with columns: `rule`, `before_diagnostics`, `after_fatal`, `after_warning`, `primary_class`, `verdict`. Use the rule with the highest non-zero counter as `primary_class`.

```python
# helper script — paste into a bash heredoc to run, or run interactively:
python3 <<'PY'
import json
with open("docs/superpowers/specs/2026-05-09-dogfood-audit.json") as f:
    audit = json.load(f)
class_keys = [
    "resolved_by_code","resolved_by_deletion","resolved_by_symbol_move",
    "accepted_by_exception","suppressed_by_public_api",
    "suppressed_by_converter_paths","suppressed_by_exempt_paths",
    "suppressed_by_acknowledged_empty","suppressed_by_budget_increase",
    "suppressed_by_override","suppressed_by_severity_tier",
    "remaining_warning_debt","remaining_fatal","proposed_but_not_landed","unknown",
]
print("| Rule | Before fatal | After fatal | After warning | Primary class | Verdict |")
print("|---|---:|---:|---:|---|---|")
for r in audit["rules"]:
    primary = max(class_keys, key=lambda k: r.get(k, 0))
    print(f"| {r['rule']} | {r.get('before_diagnostics',0)} | {r.get('after_fatal',0)} | {r.get('after_warning',0)} | `{primary}` | {r.get('verdict','')} |")
PY
```

Paste the script's output into the Markdown's `## Per-rule disposition table` section.

- [ ] **Step 4: Verify the audit MD references the JSON consistently.**

Counts in MD prose should match counts in JSON. Spot-check 3-4 rules:

```bash
grep -E '(CX001|CX002|ER007|MO001).*[0-9]+' docs/superpowers/specs/2026-05-09-dogfood-audit.md | head -10
python3 -c "import json; a=json.load(open('docs/superpowers/specs/2026-05-09-dogfood-audit.json')); [print(r['rule'], r.get('before_diagnostics',0), r.get('after_warning',0)) for r in a['rules']]"
```

If counts diverge, fix the MD inline.

- [ ] **Step 5: Commit.**

```bash
git add docs/superpowers/specs/2026-05-09-dogfood-audit.md
```

```bash
git commit -m "$(cat <<'EOF'
docs(audit): add dogfood audit narrative — per-PR forensics + honest status

Markdown audit companion to the JSON file. Leads with honest project
status snapshot (active fatals, remaining warning debt, accepted debt
with metadata, policy suppressions without metadata, severity-tier
demotions). Per-rule disposition table sourced from JSON.

Per-PR forensics for #36 (severity tier — blocking status disappeared,
diagnostics remained), #39 (mixed legitimate onboarding + two schema
gaps), #41 (proposed but not landed; test extraction viable as
follow-up, calibration half would fire PG001/PG002 today), #42
(proposed but not landed; pure calibration).

Schema gaps (CX.exempt_paths, acknowledged_empty) and refactor
candidates (scan_expr, paradigm rule file splits) tracked as
follow-up issues.

Reproducibility section gives exact `git worktree` + `cargo run`
commands. No new tooling.

Refs: #45
EOF
)"
```

---

## Task 9: Update CLAUDE.md "Project status" paragraph

**Files:**
- Modify: `CLAUDE.md` (the "Project status" section, specifically the paragraph that currently ends with "*Self-application clean-status now means **zero unexpected fatals**, not zero warnings.*")

- [ ] **Step 1: Locate the existing paragraph.**

```bash
grep -n "zero unexpected fatals" CLAUDE.md
```

Expected: one match, somewhere in the "Project status" section.

- [ ] **Step 2: Read the surrounding context to confirm the paragraph boundaries.**

Read `CLAUDE.md` lines `<grep_lineno - 5>` to `<grep_lineno + 5>` to confirm the paragraph spans correctly and you're replacing the whole block, not partial text.

- [ ] **Step 3: Replace via Edit tool.**

Use the Edit tool with `old_string` set to the full existing paragraph (multi-line, exactly as it appears in CLAUDE.md) and `new_string` set to:

```
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
> Snapshot as of 2026-05-09: <FILL FROM AUDIT JSON: N active fatals, N
> warning debt (split CX001/CX002), 14 lockfile exceptions, 12
> acknowledged_empty paradigms, 2 MO overrides, 2 CX exempt_paths
> patterns>.
```

The current CLAUDE.md uses plain prose, not blockquote — adapt the formatting to match the existing style. If the existing paragraph is plain prose (not `> ` blockquoted), drop the `> ` prefixes from the new text.

If a Layer 1 measurement at any historical ref failed (build couldn't complete) — meaning some `before_diagnostics` counters in the JSON carry `verified: false` — then **omit the snapshot line** entirely and replace it with: *"Measured snapshot in audit doc; partial measurement at historical refs (see audit `verified: false` flags)."*

- [ ] **Step 4: Sanity-check the edit.**

```bash
grep -n "Self-application status is not" CLAUDE.md
grep -c "zero unexpected fatals" CLAUDE.md
```

Expected: the new paragraph is present; the old phrase is gone (count = 0).

- [ ] **Step 5: Commit.**

```bash
git add CLAUDE.md
```

```bash
git commit -m "$(cat <<'EOF'
docs(claude): replace dogfood-status paragraph with audit-aware wording

Replaces "self-application clean-status means zero unexpected fatals,
not zero warnings" with explicit enumeration of remaining surfaces:
CX001/CX002 warning debt, accepted lockfile exceptions,
acknowledged-empty paradigms, declared public API / converter
authority, and policy suppressions.

Audit doc (docs/superpowers/specs/2026-05-09-dogfood-audit.md) is the
source of truth for snapshot numbers; CLAUDE.md carries a measured
snapshot for visibility, with the audit doc reference for drift.

Refs: #45
EOF
)"
```

---

## Task 10: Open follow-up issue #1 — `paradigms.CX.exempt_paths` debt-metadata schema

- [ ] **Step 1: Open the issue.**

```bash
gh issue create --repo ztripez/locus --title "Design debt-metadata schema for paradigms.CX.exempt_paths" --label "dogfood,architecture,high-priority" --body "$(cat <<'EOF'
## Problem

`paradigms.CX.exempt_paths` is currently a `Vec<String>` of glob patterns. Each pattern silences CX001/CX002 (and any other CX rules) for the matching files. Today there is no schema for `expires`, `reason`, `owner`, or `debt_id` on these entries.

The dogfood audit ([docs/superpowers/specs/2026-05-09-dogfood-audit.md](docs/superpowers/specs/2026-05-09-dogfood-audit.md)) flags this as a no-accountability suppression surface: the audit cannot tell whether `*::tests::*` and `locus_air::*` were added as principled carve-outs or as untracked silencing.

PG006 (Policy Guard) already requires debt metadata on new `MO.overrides`. The same shape should apply to `CX.exempt_paths` for consistency.

## Goal

Extend the lockfile schema so each `CX.exempt_paths` entry can carry:

- `pattern`: the glob (the existing string value, lifted into a struct field)
- `expires`: ISO date
- `reason`: free-text justification
- `owner`: GitHub handle / team
- `debt_id`: stable identifier
- `introduced_by`: PR reference

Backwards compatibility: parse legacy `Vec<String>` entries as `pattern`-only structs with `expires=None` (and surface those legacy entries in `locus debt`).

## Acceptance criteria

- New struct landed in `crates/locus-core/src/paradigms/complexity_budget/lockfile_schema.rs`.
- Existing 2 `CX.exempt_paths` entries in `locus.lock` upgraded with metadata, or migrated by `locus init`.
- PG006-equivalent guard fires on new exempt_paths entries lacking debt fields.
- `locus debt` lists CX exempt_paths debt alongside MO.overrides debt and lockfile.exceptions.

## Related

- Source: dogfood audit (#45)
- Sibling schema-gap issue: same shape, applied to `acknowledged_empty` (see audit doc follow-up list)
- Pattern reference: existing `MO.overrides` debt metadata in `crates/locus-core/src/paradigms/module_ownership/lockfile_schema.rs`
EOF
)"
```

Expected: prints the new issue URL.

- [ ] **Step 2: Capture the issue number.**

Save the issue number in a scratch file for cross-referencing in later issues' bodies:

```bash
gh issue list --repo ztripez/locus --label "dogfood,architecture,high-priority" --state open --search "Design debt-metadata schema for paradigms.CX.exempt_paths" --json number --jq '.[0].number' > /tmp/dogfood-audit/issue-1-number.txt
cat /tmp/dogfood-audit/issue-1-number.txt
```

---

## Task 11: Open follow-up issue #2 — `acknowledged_empty` debt-metadata schema

- [ ] **Step 1: Open the issue.**

```bash
gh issue create --repo ztripez/locus --title "Design debt-metadata schema for acknowledged_empty" --label "dogfood,architecture,high-priority" --body "$(cat <<'EOF'
## Problem

`Lockfile.acknowledged_empty` is currently a `Vec<String>` of paradigm prefixes (BO, CF, CR, DA, ER, FL, FO, PA, RM, RW, TA, UT — 12 entries in the current `locus.lock`). Each prefix silences `LOCUS002` (vacancy nudge) for that paradigm. Today there is no per-prefix `expires`, `reason`, or `owner`.

The dogfood audit ([docs/superpowers/specs/2026-05-09-dogfood-audit.md](docs/superpowers/specs/2026-05-09-dogfood-audit.md)) flags this as the largest no-accountability suppression surface: 12 paradigms are silently acknowledged-empty with no expiry and no rationale recorded.

## Goal

Extend the lockfile schema so each `acknowledged_empty` entry can carry:

- `prefix`: the paradigm prefix (the existing string value, lifted into a struct field)
- `expires`: ISO date
- `reason`: free-text justification — why is this paradigm legitimately vacant?
- `owner`: GitHub handle / team
- `debt_id`: stable identifier
- `introduced_by`: PR reference

Backwards compatibility: parse legacy `Vec<String>` entries as `prefix`-only structs with `expires=None`. Surface legacy entries in `locus debt`.

## Acceptance criteria

- New struct landed in `crates/locus-core/src/lockfile/...` (the `Lockfile` definition).
- Existing 12 `acknowledged_empty` entries in `locus.lock` upgraded with metadata, or migrated by `locus init`.
- PG004-equivalent guard fires on new acknowledged_empty entries lacking debt fields.
- `locus debt` lists acknowledged_empty debt alongside MO.overrides debt, lockfile.exceptions, and CX.exempt_paths (#1).

## Related

- Source: dogfood audit (#45)
- Sibling schema-gap issue: same shape, applied to `paradigms.CX.exempt_paths` (see audit doc follow-up list)
- Pattern reference: existing `MO.overrides` debt metadata
EOF
)"
```

- [ ] **Step 2: Capture the issue number.**

```bash
gh issue list --repo ztripez/locus --label "dogfood,architecture,high-priority" --state open --search "Design debt-metadata schema for acknowledged_empty" --json number --jq '.[0].number' > /tmp/dogfood-audit/issue-2-number.txt
cat /tmp/dogfood-audit/issue-2-number.txt
```

---

## Task 12: Open follow-up issue #3 — re-land PR #41 test extraction without budget calibration

- [ ] **Step 1: Open the issue.**

```bash
gh issue create --repo ztripez/locus --title "Re-land PR #41 test extraction without budget calibration" --label "dogfood,architecture,high-priority" --body "$(cat <<'EOF'
## Problem

PR #41 ([cx002-cleanup](https://github.com/ztripez/locus/pull/41)) bundled two distinct kinds of work:

1. **Real refactor** — extract inline `#[cfg(test)] mod tests {...}` blocks from 19 paradigm `rules.rs` files into sibling `rules_tests.rs` files. ~14k lines of test code relocated. This is a legitimate `resolved_by_code` shape: production module sizes dropped to a healthy median.
2. **Policy calibration** — `CX.default_max_module_lines = 700` workspace default + 8 `module_overrides`. This is a `suppressed_by_budget_increase` + `suppressed_by_override` shape.

The PR was closed without merging. The dogfood audit ([docs/superpowers/specs/2026-05-09-dogfood-audit.md](docs/superpowers/specs/2026-05-09-dogfood-audit.md)) recommends the test-extraction half on its own merits.

## Goal

Re-land **only** the test extraction. Do **not** include `CX.default_max_module_lines` raise or `module_overrides`.

## Acceptance criteria

- 19 `rules.rs` files have their `#[cfg(test)] mod tests {...}` blocks moved to sibling `rules_tests.rs` files via `#[path = "rules_tests.rs"] mod tests;`.
- `cargo test --workspace` green; same test count as before.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `locus.lock` unchanged (no calibration).
- Resulting CX002 count drops on its own merits as production modules shrink.
- If any module remains over the default CX002 budget after extraction, either further-split the production code or open a per-module debt entry with `expires` + `reason` (under Policy Guard PG002 + PG006).

## Constraints

- Under Policy Guard (#46), any `module_overrides` raise without debt metadata fires PG002/PG006. Splitting work means this PR has no calibration; if any module still exceeds budget, the override decision is its own scoped PR with debt metadata.

## Related

- Source: dogfood audit (#45)
- Original PR: #41 (closed, not merged)
- Reference for OT.converter_paths test-pattern adjustment: PR #41's body discusses adding `*::rules_tests` and `*::rules_tests::*`. Include those carve-outs in this PR (legitimate consequence of the test extraction).
EOF
)"
```

- [ ] **Step 2: Capture the issue number.**

```bash
gh issue list --repo ztripez/locus --label "dogfood,architecture,high-priority" --state open --search "Re-land PR #41 test extraction" --json number --jq '.[0].number' > /tmp/dogfood-audit/issue-3-number.txt
cat /tmp/dogfood-audit/issue-3-number.txt
```

---

## Task 13: Open follow-up issue #4 — re-evaluate PR #42 calibration with Policy Guard debt metadata

- [ ] **Step 1: Open the issue.**

```bash
gh issue create --repo ztripez/locus --title "Re-evaluate PR #42 calibration with Policy Guard debt metadata" --label "dogfood,architecture,high-priority" --body "$(cat <<'EOF'
## Problem

PR #42 ([cx001-cleanup](https://github.com/ztripez/locus/pull/42)) was closed without merging. It proposed:

- `CX.default_max_function_lines = 120` (from default 50)
- 6 per-file overrides on `visitor`, `lockfile_schema`, `std_rt`, `abstraction_discipline::rules`, `responsibility::rules`, `one_truth::init`.

No code change. The dogfood audit ([docs/superpowers/specs/2026-05-09-dogfood-audit.md](docs/superpowers/specs/2026-05-09-dogfood-audit.md)) classifies this as `proposed_but_not_landed` with a verdict that the PR's "0 diagnostics" claim would have meant policy suppression, not remediation.

Under Policy Guard (#46) — landed in main — this exact shape now fires:

- **PG001** on the `default_max_function_lines` raise.
- **PG002** on each new `module_override`.
- **PG006** on overrides missing debt metadata (`expires`, `reason`, `owner`, `debt_id`).

## Goal

Decide whether `CX.default_max_function_lines = 120` is the right rule-engine density for Locus's own source.

## Two paths

**Path A — accept the calibration.** Re-propose with full debt metadata: `expires`, `reason` ("rule-engine density: each rule walks AIR and builds detailed `why`/`suggested_fix` content"), `owner`, `debt_id`, paired with at least one named refactor (e.g., split `scan_expr` from issue #5) to demonstrate the calibration isn't covering symptoms. Use `--allow-policy-calibration` to clear PG001/PG002 (downgraded to Advisory under that flag).

**Path B — reject the calibration.** Keep `default_max_function_lines = 50`. Refactor the long functions surfaced by CX001 (issues #5, #6 plus any new ones) until the count drops on its own. No calibration debt.

## Acceptance criteria

- Either Path A lands with full PG006-compliant debt metadata, or Path B lands with one or more named refactor PRs that drop the CX001 count without touching `default_max_function_lines`.
- The decision is documented in either CLAUDE.md or `docs/PARADIGMS.md` so future calibration proposals know the project's stance.

## Related

- Source: dogfood audit (#45)
- Original PR: #42 (closed, not merged)
- Policy Guard: #44 / #46
- Refactor candidates that would reduce calibration pressure: see audit doc follow-up list (the `scan_expr` split and the FL/OT per-rule splits)
EOF
)"
```

- [ ] **Step 2: Capture the issue number.**

```bash
gh issue list --repo ztripez/locus --label "dogfood,architecture,high-priority" --state open --search "Re-evaluate PR #42 calibration" --json number --jq '.[0].number' > /tmp/dogfood-audit/issue-4-number.txt
cat /tmp/dogfood-audit/issue-4-number.txt
```

---

## Task 14: Open follow-up issue #5 — split `locus_rust::visitor::scan_expr` per AST variant

- [ ] **Step 1: Open the issue.**

```bash
gh issue create --repo ztripez/locus --title "Refactor candidate: split locus_rust::visitor::scan_expr per AST variant" --label "refactor,complexity-budget,dogfood-debt" --body "$(cat <<'EOF'
This is a named refactor candidate from the dogfood audit, not a release blocker by itself.

## Context

PR #42 (closed, not merged) proposed a 300-line per-file override on `crates/locus-rust/src/visitor.rs` because `scan_expr` is ~298 lines — an AST dispatcher with one `match` arm per `syn::Expr` variant. The dogfood audit ([docs/superpowers/specs/2026-05-09-dogfood-audit.md](docs/superpowers/specs/2026-05-09-dogfood-audit.md)) lists this as the single largest CX001 contributor and the most natural candidate for splitting.

## Goal

Split `scan_expr` into per-variant handlers (e.g., `scan_expr_call`, `scan_expr_match`, `scan_expr_method_call`, etc.) so the dispatcher itself is small and each handler stays well below the default CX001 budget (50 lines). After this lands, the visitor module no longer needs a per-file CX1 override.

## Acceptance criteria

- `scan_expr` is ≤ 50 lines (just dispatch).
- Per-variant handlers are each ≤ 50 lines.
- All existing visitor tests pass.
- AIR emission output is byte-identical to pre-split (run `cargo run -p locus-cli -- emit-air --workspace tests/fixtures/sample-crate --pretty` before and after; diff should be empty).
- No new `CX.module_overrides` entry needed for `locus_rust::visitor` after this lands.

## Related

- Source: dogfood audit (#45)
- Reduces calibration pressure for the PR #42 re-evaluation issue (see audit doc follow-up list).
EOF
)"
```

- [ ] **Step 2: Capture the issue number.**

```bash
gh issue list --repo ztripez/locus --label "refactor,complexity-budget,dogfood-debt" --state open --search "split locus_rust::visitor::scan_expr" --json number --jq '.[0].number' > /tmp/dogfood-audit/issue-5-number.txt
cat /tmp/dogfood-audit/issue-5-number.txt
```

---

## Task 15: Open follow-up issue #6 — paradigm rule file splits in FL/OT

- [ ] **Step 1: Open the issue.**

```bash
gh issue create --repo ztripez/locus --title "Refactor candidates: per-rule splits in failure_lineage and one_truth" --label "refactor,complexity-budget,dogfood-debt" --body "$(cat <<'EOF'
This is a named refactor candidate from the dogfood audit, not a release blocker by itself.

## Context

`crates/locus-core/src/paradigms/failure_lineage/rules.rs` and `one_truth/rules.rs` are the two largest paradigm rule files, with the most rules of any paradigm (FL: 9, OT: 12). After PR #41 would have extracted tests, both production modules would still have been ~1300 lines — large enough to motivate the per-file budget overrides PR #41 proposed.

The dogfood audit ([docs/superpowers/specs/2026-05-09-dogfood-audit.md](docs/superpowers/specs/2026-05-09-dogfood-audit.md)) lists per-rule splits as the structural fix that would let the workspace default budget apply without per-file overrides.

## Goal

Split `failure_lineage::rules` and `one_truth::rules` into per-rule submodules. Each rule (FL001, FL002, …, OT001, OT002, …) gets its own file under the paradigm's `rules/` directory. The paradigm's `rules.rs` becomes a small registration module that imports each rule's `register` function.

## Acceptance criteria

- Per-rule files under `crates/locus-core/src/paradigms/failure_lineage/rules/` and `one_truth/rules/`.
- Each rule file ≤ ~250 lines of production code.
- All existing tests pass.
- Diagnostic output is byte-identical to pre-split.
- `cargo test -p locus-core --test docs_drift` green (rule count unchanged).
- No new `CX.module_overrides` entry needed for either path after this lands.

## Constraints

- Don't bundle this with PR #41 test extraction (#3) — that's a separate, mechanical refactor. This is a per-rule production split, conceptually distinct.

## Related

- Source: dogfood audit (#45)
- Reduces calibration pressure for the PR #42 re-evaluation issue (see audit doc follow-up list).
- Sibling refactor: the `scan_expr` per-AST-variant split.
EOF
)"
```

- [ ] **Step 2: Capture the issue number.**

```bash
gh issue list --repo ztripez/locus --label "refactor,complexity-budget,dogfood-debt" --state open --search "per-rule splits in failure_lineage and one_truth" --json number --jq '.[0].number' > /tmp/dogfood-audit/issue-6-number.txt
cat /tmp/dogfood-audit/issue-6-number.txt
```

---

## Task 16: Cross-link follow-up issues into the audit doc, clean up, push, open PR

**Files:**
- Modify: `docs/superpowers/specs/2026-05-09-dogfood-audit.md` (add the actual issue numbers)
- Cleanup: remove `/tmp/locus-measure-*` worktrees and `/tmp/dogfood-audit/` scratch

- [ ] **Step 1: Read captured issue numbers.**

```bash
for i in 1 2 3 4 5 6; do
  echo "issue-$i: $(cat /tmp/dogfood-audit/issue-$i-number.txt)"
done
```

Expected: 6 lines, each with the GitHub issue number.

- [ ] **Step 2: Replace placeholder `#1`–`#6` references in the audit MD with real numbers.**

The audit MD draft from Task 8 uses ordinal placeholders (`issue #1` through `issue #6`) when referencing follow-up issues. Patch each to the real GitHub issue number using the captured numbers.

For each ordinal `N` in 1..6, run:

```bash
real=$(cat /tmp/dogfood-audit/issue-$N-number.txt)
echo "ordinal #$N → real #$real"
```

Use the Edit tool on `docs/superpowers/specs/2026-05-09-dogfood-audit.md` to substitute each occurrence. The design spec (`2026-05-09-dogfood-audit-design.md`) stays as-is — its `#1`–`#6` references are abstract.

The follow-up issue *bodies* themselves use prose references ("the sibling schema-gap issue", "the named refactor candidate"), not ordinal numbers — no substitution needed there. The audit doc is the single navigation hub with real numbers.

Verify the substitution:

```bash
grep -E 'issue #[0-9]+' docs/superpowers/specs/2026-05-09-dogfood-audit.md | head -10
```

Expected: only real GitHub issue numbers (matching the captured values), no leftover `#1`–`#6` ordinals.

- [ ] **Step 3: Commit cross-link updates.**

```bash
git add docs/superpowers/specs/2026-05-09-dogfood-audit.md
```

```bash
git commit -m "$(cat <<'EOF'
docs(audit): cross-link follow-up issue numbers in audit narrative

Replace ordinal placeholders (#1–#6) in the audit with real issue
numbers after `gh issue create`. Sibling cross-links also added to
each issue body via `gh issue edit`.

Refs: #45
EOF
)"
```

If no changes were necessary (issue numbers happened to match the placeholders, unlikely), skip the commit and continue.

- [ ] **Step 4: Clean up scratch worktrees.**

```bash
git worktree remove /tmp/locus-measure-pre_36
```

```bash
git worktree remove /tmp/locus-measure-post_36
```

```bash
git worktree remove /tmp/locus-measure-post_39
```

```bash
git worktree list
```

Expected: only the original repo and `dogfood-audit` branch worktrees remain.

- [ ] **Step 5: Verify final commit set.**

```bash
git log --oneline origin/main..HEAD
```

Expected: 4 commits — `bd34ecf` (design spec), the JSON audit commit, the MD audit commit, the CLAUDE.md commit, and (optionally) the cross-link commit. 4 or 5 commits total.

- [ ] **Step 6: Push the branch.**

```bash
git push -u origin dogfood-audit
```

Expected: branch pushed; tracking set.

- [ ] **Step 7: Open the PR.**

```bash
gh pr create --title "Dogfood audit: classify diagnostic dispositions across PR #36/#39/#41/#42 — close #45" --body "$(cat <<'EOF'
## Summary

Closes #45. Lands the retrospective dogfood audit that distinguishes real fixes from policy suppressions across the dogfood-relevant PR sequence (#36, #39, #41, #42). Replaces the `--agent-strict exits 0` claim with a counted breakdown.

## What's in this PR

- **Design spec:** [`docs/superpowers/specs/2026-05-09-dogfood-audit-design.md`](docs/superpowers/specs/2026-05-09-dogfood-audit-design.md). 15-class verdict taxonomy (locked), three-layer methodology, six follow-up issues.
- **Structured audit:** [`docs/superpowers/specs/2026-05-09-dogfood-audit.json`](docs/superpowers/specs/2026-05-09-dogfood-audit.json). Per-rule and per-PR records; arithmetically auditable counters (`before_diagnostics == sum(all classes)`).
- **Narrative audit:** [`docs/superpowers/specs/2026-05-09-dogfood-audit.md`](docs/superpowers/specs/2026-05-09-dogfood-audit.md). Honest project status snapshot, per-rule disposition table, per-PR forensics, methodology, reproducibility commands.
- **CLAUDE.md update:** replaces the existing "self-application clean-status now means zero unexpected fatals" paragraph with explicit enumeration of remaining surfaces and a measured snapshot.
- **6 follow-up issues opened:** debt-metadata schemas for `CX.exempt_paths` and `acknowledged_empty`; re-land path for PR #41 test extraction; re-evaluation path for PR #42 calibration; refactor candidates for `scan_expr` and FL/OT rule files.

## Findings

- **PR #36** changed blocking status, not diagnostics. CX001/CX002 demoted from Fatal to Warning under `--agent-strict`; `134` diagnostics remained visible as warnings.
- **PR #39** is mixed-legitimate: 47 OT canonicals were already source-hint annotated and merely persisted to lockfile (counted as `resolved_by_code`); 14 lockfile exceptions and 2 MO overrides carry full debt metadata; **2 surfaces lack metadata schema**: `CX.exempt_paths` and `acknowledged_empty`. Schema gaps tracked in follow-up issues.
- **PR #41** is split-classified: test extraction would have been `resolved_by_code`; budget calibration would have been `suppressed_by_budget_increase` + `suppressed_by_override`. Did not land.
- **PR #42** is pure calibration. Did not land. Under Policy Guard (#46) it would now fire PG001 + PG002.

## What this PR does NOT do

- No lockfile schema changes (debt metadata gaps tracked as separate issues).
- No code refactors (refactor candidates tracked as separate issues).
- No re-landing of PR #41 or PR #42.
- No new tooling.
- No README change (verified to not overclaim).
- No new Policy Guard test cases.

## Test plan

- [ ] `cargo test --workspace` green (no code changes; should remain green).
- [ ] `cargo run -p locus-cli -- check --workspace . --agent-strict` exit code matches `target` measurement (probably exit 0).
- [ ] JSON syntax valid: `python3 -m json.tool docs/superpowers/specs/2026-05-09-dogfood-audit.json > /dev/null`.
- [ ] Per-rule arithmetic invariant holds: each `before_diagnostics == sum(all class counters)`.
- [ ] CLAUDE.md no longer contains the phrase "zero unexpected fatals".
- [ ] All 6 follow-up issues exist and are properly cross-linked.

## Acceptance criteria mapping (#45)

- AC1 (no longer reports only `0 fatal`): audit doc honest-status section + CLAUDE.md update.
- AC2 (separates real fixes from policy suppression): 15-class taxonomy + per-rule counters.
- AC3 (PR #42 = policy suppression): §PR-42 forensic, `proposed_but_not_landed` verdict.
- AC4 (PR #41 split-accounted): §PR-41 forensic, two-halves treatment.
- AC5 (PR #39 lockfile classified): §PR-39 forensic, per-mechanism breakdown.
- AC6 (suppressions needing debt metadata or refactor follow-up): 6 follow-up issues.
- AC7 (honest wording): CLAUDE.md update with measured snapshot.

Closes #45
EOF
)"
```

Expected: prints PR URL.

- [ ] **Step 8: Final verification.**

```bash
gh pr view --repo ztripez/locus --json title,body,baseRefName,headRefName,state | python3 -m json.tool
```

Expected: PR opened, base = main, head = dogfood-audit, state = OPEN. Verify the body matches the heredoc template above.

```bash
ls /tmp/dogfood-audit/ /tmp/locus-measure-* 2>&1 | head -20
```

Expected: scratch dir may still exist (that's fine; clean up via `rm -rf /tmp/dogfood-audit /tmp/locus-measure-* 2>/dev/null` if desired). Worktrees should be gone.

---

## Self-review checklist

After plan execution, the implementing agent should confirm:

- [ ] All 6 follow-up issues exist and have correct labels.
- [ ] JSON audit file is valid and the per-rule invariant holds for every rule record.
- [ ] Markdown audit references match JSON counts.
- [ ] CLAUDE.md no longer contains "zero unexpected fatals".
- [ ] PR opened with explicit AC mapping in body.
- [ ] No code changes outside the listed doc files.
- [ ] No new lockfile schema changes.
- [ ] No `--no-verify` or hook bypasses used.
- [ ] No mention of Claude / AI assistants in commit messages or PR body.
