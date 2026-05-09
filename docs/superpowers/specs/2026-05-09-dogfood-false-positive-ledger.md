# Dogfood false-positive ledger (Epic #1)

Date: 2026-05-09 (updated 2026-05-09)
Scope: Locus self-application triage
Related: `docs/superpowers/plans/2026-05-09-issue-1-epic-execution.md` (Workstream #3)

## Purpose

This ledger is the per-rule triage surface for dogfooding. It separates:

- **implementation regressions** (true positives),
- **onboarding gaps** (fix with lockfile/init onboarding),
- **accepted debt** (known, intentional for now),
- **spec/heuristic mismatch** (candidate false positives needing rule tuning).

The rule is simple: strictness decisions should come from this ledger, not memory.

## How to refresh

1. Run changed-only strict for PR safety:

```bash
locus check --workspace . --changed --agent-strict
```

2. Run full check for inventory shaping:

```bash
locus check --workspace .
```

3. Run suppression hotspot view:

```bash
locus debt --by-rule
locus debt --json --by-rule
```

4. Update this table for any rule whose totals or classification changed.

## Classification legend

- **TP** — true positive architectural problem.
- **FP** — likely false positive (heuristic/spec drift).
- **ONBOARD** — expected until lockfile/onboarding declarations are added.
- **DEBT** — known, accepted, intentionally deferred.

## Graduation checklist (per rule)

A rule is ready for broad strict recommendation when all are true:

- [ ] At least one dogfood incident where the rule caught real agent-introduced damage.
- [ ] False-positive rate is documented and acceptable for changed-only strict usage.
- [ ] There is a short, deterministic fix path (command/mutator/annotation) in docs.
- [ ] The rule's severity posture matches observed economics (Warning vs Fatal under strict).
- [ ] Any onboarding-shaped floods are addressed by init suggestions or explicit lockfile guidance.

## Ledger

| Rule | Last observed count | Class | Primary cause | Next action | Owner | Strictness note |
|---|---:|---|---|---|---|---|
| DG003 | 0 (2026-05-09 rerun) | ONBOARD ✅ | Feature onboarding/docs landed (`--public-api` guidance now present); no active DG003 in current self-check snapshot | Keep feature/public_api onboarding path as default in `init`; recheck each dogfood pass | @core | Keep Warning→Fatal under `--agent-strict`; now behaving as intended for changed-only strict |
| OT004 | 0 (2026-05-09 rerun) | ONBOARD ✅ | Converter authority model is now explicit via `OT.converter_paths`; no active OT004 in current self-check snapshot | Keep converter-path onboarding guidance in docs and lockfile mutator flow | @core | Strict-ready in changed-only mode; remaining risk is onboarding drift, not rule noise |
| CX001 | 106 (2026-05-09) | DEBT | Default noisy threshold on un-onboarded repo (50-line default) | Set workspace `default_max_function_lines` once Locus's own dense rule files are triaged; per-module overrides for CX rule files / paradigm host | @core | **Advisory** (uses `elevate_when_actionable`). Stays Warning under `--agent-strict` until `default_max_function_lines` or a per-module override is set; then elevates to Fatal. Closed `#6`. |
| CX002 | 27 (2026-05-09) | DEBT | Default noisy threshold on un-onboarded repo (400-line default) | Same as CX001 but for `default_max_module_lines` / `module_overrides` | @core | **Advisory** (uses `elevate_when_actionable`). Same gating as CX001. Closed `#6`. |
| CX007 | 1 (2026-05-09) | DEBT | One file (likely the CLI dispatcher) crosses the public-item budget | Add `paradigms.CX.max_public_items` override or refactor the file | @core | Classified Advisory in `docs/PARADIGMS.md` §"Severity tiers"; rule code still uses plain `elevate` until the next pass. |
| CX008 | 0 (2026-05-09) | — | Orchestration-paths gating already silences self-application | — | @core | Strict-after-onboarding. The `orchestration_paths` lockfile field is the user's narrowing knob. |
| DC001 | 0 (2026-05-09) | DEBT | Opt-in via `paradigms.DC.require_public_docs` (defaults to `false`) | Leave opt-in; revisit once we want to gate strict on documented public APIs | @docs | Already advisory-shaped: silent until the toggle is set. Closed `#6` w.r.t. severity policy. |
| DC002/DC004 | 3 (DC002) / 0 (DC004) | MIXED | Some real residue, some phrase overreach | Tag FP/TP examples and tighten phrase lists if needed | @docs | Strict-after-onboarding tier; keep Warning in human mode. |
| ER001/ER007 | 0 / 11 | MIXED | Taxonomy drift + intentional legacy spread | Distinguish true drift from migration state | @core | Strict-after-onboarding; useful for new deltas via `--changed --agent-strict`. |
| MO001 | 2 (2026-05-09) | DEBT | Default public-types-per-file budget on un-onboarded repo | Classified Advisory; will move to `elevate_when_actionable` in a follow-up sweep | @core | Advisory tier in policy table; rule code still uses plain `elevate`. |
| CF002 | 0 (rule body deferred) | — | Stub today — lockfile fields ship; rule body lands once a filesystem-aware loader is available | When implementing, default to `elevate_when_actionable` per the policy table | @core | Advisory tier in policy table; nothing to fire yet. |

## Notes

- Baseline counts for DG003/OT004 come from `docs/superpowers/specs/2026-05-09-self-onboarding-findings.md`.
- 2026-05-09 rerun snapshot (via `cargo run -p locus-cli -- check --workspace . --json`) shows: `DG003=0`, `OT004=0`, `CX001=106`, `CX002=27`, `ER007=11`, `LOCUS002=13`.
- Machine-readable companion: `docs/superpowers/specs/2026-05-09-dogfood-false-positive-ledger.json` (same rows/labels as this table).
- Keep the Markdown table and JSON companion in sync in the same PR.
