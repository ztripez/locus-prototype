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
| CX001/CX002/CX007/CX008 | (varies) | DEBT | Default noisy thresholds on un-onboarded repo | Record per-module overrides where justified | @core | Good strict candidates after threshold pass |
| DC002/DC004 | (varies) | MIXED | Some real residue, some phrase overreach | Tag FP/TP examples and tighten phrase lists if needed | @docs | Keep Warning in human mode |
| ER001/ER007 | (varies) | MIXED | Taxonomy drift + intentional legacy spread | Distinguish true drift from migration state | @core | Strict for new deltas only |

## Notes

- Baseline counts for DG003/OT004 come from `docs/superpowers/specs/2026-05-09-self-onboarding-findings.md`.
- 2026-05-09 rerun snapshot (via `cargo run -p locus-cli -- check --workspace . --json`) shows: `DG003=0`, `OT004=0`, `CX001=106`, `CX002=27`, `ER007=11`, `LOCUS002=13`.
- Machine-readable companion: `docs/superpowers/specs/2026-05-09-dogfood-false-positive-ledger.json` (same rows/labels as this table).
- Keep the Markdown table and JSON companion in sync in the same PR.
