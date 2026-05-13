# Epic #1 execution plan: dogfood triage and authority-model hardening

Date: 2026-05-09
Issue: https://github.com/ztripez/locus/issues/1

## Intent

This plan turns the epic into a sequenced, dogfood-first execution path with concrete exit criteria per workstream. The project constraint is explicit: no new paradigms until the current authority/onboarding model is validated on Locus itself.

## Workstreams and deliverables

### 1) OT construction authority hardening (OT004 flood control)

**Problem:** AIR canonical types are intentionally constructed in `locus-rust`, but current acceptance posture can classify this as unauthorized canonical construction.

**Deliverables:**
- Define and document accepted converter authority boundaries for adapter crates.
- Add/verify onboarding mutators and init suggestions that make those accepts short and obvious.
- Re-run dogfood checks and confirm OT004 count drops for known adapter-construction sites.

**Exit criteria:**
- OT004 findings remaining in Locus self-check are either true positives or explicitly accepted debt.

### 2) DG feature public API onboarding hardening (DG003 flood control)

**Problem:** features without `public_api` declarations produce broad DG003 floods.

**Deliverables:**
- Ensure `locus init` suggests `locus dg define-feature ... --public-api ...` commands for each detected feature.
- Add docs examples for multi-crate feature declarations with minimal `public_api` starter patterns.
- Verify post-onboarding DG003 findings transition from flood to targeted internals-reach diagnostics.

**Exit criteria:**
- DG003 on Locus self-check is dominated by actionable internals reach, not undeclared API surfaces.

### 3) False-positive ledger and rule graduation gate

Tracking artifact: `docs/superpowers/specs/2026-05-09-dogfood-false-positive-ledger.md`.

**Problem:** broad heuristic rules need a repeatable mechanism to separate accepted noise from implementation regressions.

**Deliverables:**
- Create a dogfood ledger artifact with per-rule fields:
  - total findings
  - true positive / false positive / accepted debt classification
  - lockfile/onboarding fix available?
  - severity posture recommendation
- Add a graduation checklist that a rule must satisfy before strict CI recommendation.

**Exit criteria:**
- Every broad Warning-by-default rule has ledger entries and a documented strictness recommendation.

### 4) Docs/status drift guard

**Problem:** AGENTS/README/PARADIGMS/status claims can drift from actual registered paradigms/rules.

**Deliverables:**
- Add a lightweight drift-check routine to release/PR hygiene:
  - compare declared paradigm/rule counts in docs to registry reality,
  - verify CLI command surface described in docs still exists.
- Record a standard update order when counts change: code → tests → docs snapshot.

**Exit criteria:**
- No stale count/command claims across AGENTS + PARADIGMS at merge time.

### 5) `--changed --agent-strict` dogfood loop as primary acceptance gate

**Problem:** full-repo clean is not the right near-term milestone; changed-code precision is.

**Deliverables:**
- Standardize local and CI-facing command sequence around:
  - `locus check --workspace . --changed --agent-strict`
- Document baseline behavior and expected usage for contributor PRs.
- Confirm known historical noise does not block changed-only strict checks.

**Exit criteria:**
- Contributors can use changed-only strict mode to catch new architectural damage without historical cleanup blocking.

## Milestone order

1. OT004 authority onboarding improvements.
2. DG003 feature/public_api onboarding improvements.
3. False-positive ledger + severity recommendations.
4. Docs/status drift guard.
5. Lock in changed-only strict as primary dogfood quality gate.

## Non-goals reaffirmed

- No new paradigms in this epic.
- No framework-specific loaders in this epic.
- No requirement to reach full-repo warning-free state.

## Definition of done (epic)

- `locus check --workspace . --changed --agent-strict` is reliable in day-to-day Locus development.
- Top flood rules (currently DG003, OT004) are either reduced via authority/onboarding fixes or explicitly tracked as debt.
- Rule severity recommendations are backed by a maintained dogfood ledger, not intuition.
- Docs status reflects implementation reality at merge time.

## Completion status (2026-05-09)

All five workstreams are complete for Epic #1 scope.

1. ✅ **OT004 authority hardening complete** — converter authority is explicit (`OT.converter_paths`), and latest self-check snapshot shows `OT004=0`.
2. ✅ **DG003 onboarding hardening complete** — `init`/docs now steer users to `define-feature ... --public-api ...`, and latest self-check snapshot shows `DG003=0`.
3. ✅ **False-positive ledger + graduation gate complete** — maintained markdown + JSON artifacts exist and carry strictness notes per seeded noisy rules.
4. ✅ **Docs/status drift guard complete** — docs snapshot guard test is in place and enforced in repo tests.
5. ✅ **Changed-only strict loop complete** — canonical script exists and equivalent cargo-run invocation is documented/used in dogfood checks.

### Evidence snapshot used for closure

- `cargo run -p locus-cli -- check --workspace . --changed --agent-strict` (expected strict failures are currently CX-family debt, not DG003/OT004 flood).
- `cargo run -p locus-cli -- check --workspace . --format json` (rule count snapshot includes `DG003=0`, `OT004=0`). Pre-#29 this flag was spelled `--json`.
- `cargo run -p locus-cli -- debt --json --by-rule` (debt hotspot view available for ongoing triage).
