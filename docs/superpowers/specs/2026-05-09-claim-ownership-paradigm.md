# CL — Claim Ownership paradigm

Date: 2026-05-09
Issue: #16 (related: #1, #5, #8)

## Problem

Comments and documentation often contain maintenance-relevant claims —
"see #123", "temporary workaround", "keep in sync with X", "safe because
the input is validated", "only implementation" — that silently become
stale authority. They steer humans and agents but are rarely re-checked.
When the underlying truth drifts, the text becomes a hidden source of
architectural error.

The core question: **does this text have authority to make the claim it
is making?**

## Goal

A deterministic, narrow paradigm that detects high-risk natural-language
claims in comments and docs without becoming a prose linter and without
LLM-in-the-loop. Each claim class identifies a structural shape (a
trigger phrase + missing evidence), not subjective prose quality.

## Claim classes

Six structural classes, ordered by maturity for first implementation:

### Reference claims (CL001)

External references to issues, PRs, ADRs, tickets, or URLs that aren't
backed by local rationale.

- **Triggers:** `#\d+` (GitHub-style), `https?://...`, `[A-Z]{2,}-\d+`
  (Jira-style — disabled in MVP, follow-up).
- **Bad:** `See #123.`
- **Good:** `Use the compat path because mobile clients still send v1
  payloads. See #123 for the migration plan.`
- **Evidence shape:** the doc block must contain enough non-reference text
  to constitute a local rationale. MVP heuristic: ≥ 5 non-reference words.

### Temporal claims (CL002)

Claims that something is temporary, current, planned, deprecated, legacy,
or pending without an expiry, owner, or removal condition.

- **Triggers:** `temporary`, `for now`, `currently`, `until`, `planned`,
  `not yet`, `deprecated`, `legacy`.
- **Evidence shape:** date / version / owner / issue ref / removal
  condition present in the same block.

### Synchronization claims (CL003)

Text saying two things must stay aligned with no checker named.

- **Triggers:** `keep in sync`, `must match`, `same as`, `do not change
  without updating`.
- **Evidence shape:** both sides named (symbol or path) AND a checker /
  test / manual-review marker.

### Generated/provenance claims (CL004)

A file or block claims to be generated, copied, vendored, or derived,
with no source artifact or regeneration command.

- **Triggers:** `generated`, `do not edit`, `copied from`, `vendored from`.
- **Evidence shape:** source path/URL + regeneration command in the same
  comment block.

### Status/cardinality claims (CL005)

Broad assertions of status or quantity that aren't backed by a generated
or checked source.

- **Triggers:** `only`, `all`, `none`, `complete`, `stub`, `unused`,
  `dead`, `single implementation`, `implemented`.
- **Evidence shape:** a generated / checked source for the claim, OR
  narrower wording that doesn't claim global truth.

### Safety claims (CL006)

Text saying something is safe because of an invariant, with no enforcing
symbol named.

- **Triggers:** `safe because`, `cannot fail`, `already validated`,
  `unchecked`.
- **Evidence shape:** the enforcing invariant — symbol, type, function, or
  test — named locally.

## AIR / fact model

For the MVP (CL001 only), no new AIR types are needed. The existing
`AirType.doc` and `AirFunction.doc` joined-doc-text fields carry what
CL001 needs. Free-floating block comments and Markdown documents are
**out of scope for the first slice**; they need a new AIR type or fact
to land cleanly:

```rust
TextClaim {
    file,
    span,
    surface: Comment | Markdown | ScriptComment | ConfigComment,
    text,
    claim_kinds: Vec<ClaimKind>,
    references: Vec<String>,
    evidence: ClaimEvidence,
}

ClaimEvidence {
    has_local_rationale: bool,
    has_date: bool,
    has_owner: bool,
    has_removal_condition: bool,
    has_checker_command: bool,
    has_source_path: bool,
    has_regeneration_command: bool,
    named_symbols_or_paths: Vec<String>,
}
```

This shape is documented for the follow-up loader/AIR work that lands
CL002–CL006.

## Severity tiers

Per `docs/PARADIGMS.md` §"Severity tiers":

| Rule | Tier | Default | Under `--agent-strict` |
|---|---|---|---|
| CL001 | Strict-after-onboarding (gated by toggle) | Warning | Fatal — but only when the toggle is on |
| CL002–CL006 | Advisory until dogfooded | Warning | Warning until ledger evidence promotes them |

CL001's "narrowing" knob is the lockfile toggle
`paradigms.CL.require_local_rationale` (default `false` → silent). Once
the user opts in, the rule fires. This mirrors DC001's
`require_public_docs` opt-in.

## First implementation scope (MVP)

1. **CL001 only** — orphan external reference detection.
2. **Doc comments on items** — `AirType.doc` and `AirFunction.doc`.
   Free-floating comments and Markdown defer to follow-up.
3. **Triggers**: `#\d+` (GitHub-style issue/PR refs) and `https?://...`
   URLs. Other formats land later.
4. **Heuristic**: a doc text is "orphan" if, after stripping recognised
   reference tokens, fewer than 5 word tokens remain.
5. **Opt-in via `paradigms.CL.require_local_rationale = true`**. Default
   off so existing repos aren't suddenly noisy.
6. **Severity**: Warning by default; `mode.elevate(Severity::Warning)`
   produces Fatal under `--agent-strict`. The toggle gates whether the
   rule fires at all, so elevation is straightforward.

## Acceptance criteria mapping (#16)

- [x] Paradigm/rule design in this doc and `docs/PARADIGMS.md`.
- [x] Claim classes and deterministic trigger phrases listed.
- [x] Generic fixture (Markdown-style and Rust-comment cases) — see
      `tests/fixtures/cl-claims/`.
- [x] CL001 implemented with tests.
- [x] CL001 catches `See #123.` style orphan references.
- [x] CL001 allows references when surrounding block has rationale.
- [x] Diagnostics explain that external references are traceability, not
      durable rationale.
- [x] Agent-strict applies via the `require_local_rationale` toggle, so
      MVP is opt-in for changed-file CI gating.

## Non-goals (carried from #16)

- Do not grade prose quality.
- Do not require LLM semantic review.
- Do not ban issue links or ADR links.
- Do not force every normal explanatory comment to carry metadata.
- Do not make broad natural-language inference a release blocker.

## Follow-ups

- CL002–CL006 implementations once a `TextClaim` AIR/fact shape lands.
- Markdown loader + free-floating comment loader for the surfaces beyond
  doc-on-item.
- Locus's own dogfood toggle for CL001 — once CL001's false-positive
  profile is established on diverse repos, flip Locus's lockfile to
  `require_local_rationale = true` and address its own orphan refs.
