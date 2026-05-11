# Governance Spine — Design Spec

**Status:** approved design; ready for implementation planning.
**Issue:** [#71 — Transition Locus from rule-runner linter to architecture governance engine](https://github.com/ztripez/locus/issues/71).
**Scope:** initial-scope items 1–5 from the epic. Later work (`locus observe`, git
archaeology, mutation detection, declared transitions, fit/miss) is explicitly
out of scope and gets its own specs.

## Problem

Locus currently runs every paradigm's rules as independent checks that emit
`Diagnostic` values directly from `Paradigm::check()`. That keeps it in linter
territory: a rule's finding *is* the final user-facing output. There is no
contextual decision layer between observation and emission, so it is impossible
for one rule's finding to be downgraded, suppressed, marked as transition debt,
or correlated with another rule's finding by *governance logic* rather than
post-hoc filtering inside the CLI.

The target shape is:

```
rules / sensors  ->  findings / evidence  ->  policy decisions  ->  diagnostics
```

Rules become evidence producers. Policies are first-class decision-makers.
Diagnostics are the *output* of governance, not the substrate of it.

## Non-goals (this spec)

- `locus observe` mode.
- Git-history architecture archaeology.
- Architecture mutation detection.
- Declared transitions and mixed-mode governance.
- Architecture fit / miss assessment.
- Public Cargo release.
- Migration of `apply_exceptions`, `--changed` filter, or Policy Guard
  (`policy_guard.rs`) into the new policy layer. Those stay as CLI
  post-processing for this spec and become real `PolicyDefinition` impls in
  future epics. `DecisionStatus::AcceptedException` exists in the type model
  as a forward-compat slot, not as MVP behavior.
- Migration of PG-family rules. **Do not migrate PG-family rules in this spec.**

## Strangler stance

The spec is a strangler. The existing `paradigm.check() -> Vec<Diagnostic>`
path continues to run, indefinitely, until each rule is migrated. New rules
target `RuleDefinition`; legacy rules stay where they are. A compatibility
adapter wraps each legacy diagnostic into a synthetic `RuleFinding` and runs
it through a pass-through policy so the on-the-wire output is unchanged.

The legacy path is **transitional**, not "equal architecture." Naming and
comments must say so explicitly. New rules must implement `RuleDefinition`,
not be added inside legacy `Paradigm::check`.

## Architecture overview

New module: `locus-core::governance` (a plain module for MVP; promotable to
its own crate when the dependency graph paradigm complains).

```
                                              +-----------------------+
                          (registered rules) ->|                       |
                                              | governance::pipeline  |
            (legacy Paradigm::check output) ->| (rules -> findings    |---> Diagnostics
                                              |  -> policies          |
                                              |  -> decisions         |
                                              |  -> materialize)      |
                                              +-----------------------+
                                                    |    ^
                                                    v    |
                                              FindingStore / Decision log
                                              (retained for future
                                               observe / debt / SARIF)
```

Pipeline phases:

1. **Phase A — migrated rules observe.** `RuleDefinition::observe` produces
   structured `RuleFinding`s.
2. **Phase B — legacy adapter.** `LegacyParadigmRuleAdapter` runs each
   legacy `Paradigm::check`, filters out diagnostics whose `rule_id` is
   already covered by a registered `RuleDefinition` (per-diagnostic-code
   filter, not per-paradigm), and synthesizes a `RuleFinding` for the rest.
3. **Phase C — policies decide.** Policies run in `PolicyRegistry::standard()`
   order. Each sees the full `FindingStore` plus prior decisions. Policies
   may emit new findings (`FindingSource::Policy`) but MVP does **not**
   require multi-pass / fixed-point evaluation — single pass only.
4. **Phase D — materialize.** Each `Decision` becomes a `Diagnostic` (or is
   recorded but dropped, depending on `DecisionStatus`).

The CLI's existing `apply_exceptions`, `--changed`, and Policy Guard
post-processing run **after** materialization for this spec. They become
policies in future work.

## Core types

All in `locus-core::governance` unless noted.

### Identity newtypes

`const`-constructible so static registries work:

```rust
pub struct RuleId(&'static str);       // "CX001"
pub struct ParadigmId(&'static str);   // "CX"
pub struct PolicyId(&'static str);     // "default-pass-through", "registry-integrity"
pub struct FindingId(u64);             // deterministic counter
```

`FindingIdMinter` produces `FindingId`s via a deterministic counter. The
spec invariant:

> Finding IDs must be deterministic for a fixed input, registry order, and
> policy order.

This matters for stable JSON / SARIF output later.

### `RuleFinding`

```rust
pub struct RuleFinding {
    pub id: FindingId,
    pub source: FindingSource,
    pub rule_id: Option<RuleId>,             // None for legacy / policy findings
    pub paradigm_id: Option<ParadigmId>,     // direct for query convenience
    pub default_severity: Severity,
    pub span: Option<AirSpan>,               // None ⇒ synthetic `<governance>` span at materialization
    pub concept: Option<String>,
    pub message: String,                     // default render; policy may annotate via rationale
    pub evidence: Vec<Evidence>,             // multi-signal
    pub why: Vec<String>,
    pub suggested_fix: Option<String>,
}

pub enum FindingSource {
    RegisteredRule(RuleId),
    LegacyDiagnostic { rule_code: String, paradigm: Option<ParadigmId> },
    Policy(PolicyId),
}
```

Direct `rule_id` / `paradigm_id` fields duplicate information available in
`source`, deliberately, so reporting and registry validation are not forced
to pattern-match on the source enum every time.

### `Evidence`

```rust
pub enum Evidence {
    // Typed variants — added as rules migrate. MVP ships at least:
    ComplexityBudget { lines: u32, budget: u32, override_match: Option<String> },
    InferenceConfidence { score: Confidence, signals: Vec<String> },
    // Catch-all for migrated rules whose evidence schema is not yet typed:
    Structured(serde_json::Value),
    // Synthetic adapter payload — no schema, just the original diagnostic prose:
    Legacy(LegacyEvidence),
}

pub struct LegacyEvidence {
    pub original_message: String,
    pub original_why: Vec<String>,
    pub original_suggested_fix: Option<String>,
}

pub enum Confidence { Low, Medium, High }
```

Numeric confidence (`f32`) is deliberately not used in MVP. The deterministic
tier helper `Severity::from_confidence(f32, mode)` stays available to legacy
code, but new rules use `Confidence` and map it at the rule layer.

### `Decision` / `DecisionStatus` / `SeverityChange`

```rust
pub struct Decision {
    pub finding_id: FindingId,
    pub policy: PolicyId,
    pub severity: Severity,              // final
    pub status: DecisionStatus,
    pub severity_change: SeverityChange,
    pub rationale: Vec<String>,          // policy's own reasoning
}

pub enum DecisionStatus {
    Active,                  // normal violation, emitted
    Advisory,                // informational, emitted
    SuppressedByPolicy,      // recorded, NOT emitted as Diagnostic
    AcceptedException,       // recorded, NOT emitted (reserved for ExceptionPolicy migration)
    KnownTransitionDebt,     // emitted; visible migration backlog
}

pub enum SeverityChange {
    Unchanged,
    Downgraded { from: Severity },
    Elevated   { from: Severity },
}
```

`DecisionStatus` describes **architectural state**, not severity mutation.
`SeverityChange` describes severity mutation independently. They are
orthogonal.

**Invariant (MVP):** every finding gets exactly one decision per run.
Multiple decisions for the same `finding_id` is an internal error.
`validate_decisions(&decisions, &store)` enforces it after policy
evaluation.

## Traits and registries

### `RuleDefinition`

```rust
pub trait RuleDefinition: Send + Sync {
    fn id(&self) -> RuleId;
    fn paradigm(&self) -> ParadigmId;
    fn title(&self) -> &'static str;
    fn default_severity(&self) -> Severity;
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding>;
}

pub struct RuleContext<'a> {
    pub air: &'a AirWorkspace,
    pub lockfile: &'a Lockfile,
    pub mode: CheckMode,
    pub rule_registry: &'a RuleRegistry,           // read-only
    pub paradigm_registry: &'a ParadigmRegistry,   // read-only
    pub finding_ids: &'a FindingIdMinter,
}
```

`CheckMode` stays in `RuleContext` for the strangler phase so migrated rules
preserve existing severity behavior. Long-term, mode decisions move to
policies — explicitly out of scope for this spec.

### `ParadigmDefinition`

```rust
pub trait ParadigmDefinition: Send + Sync {
    fn id(&self) -> ParadigmId;
    fn title(&self) -> &'static str;
    /// Rules migrated to RuleDefinition. May be empty during transition.
    /// Anything not in this list still runs via the legacy Paradigm::check
    /// path and is wrapped by LegacyParadigmRuleAdapter.
    fn rules(&self) -> &'static [&'static dyn RuleDefinition];
}
```

The existing `Paradigm` trait stays in place, **explicitly marked
transitional** in both code comments and `CLAUDE.md` / `AGENTS.md`:

```rust
// Transitional legacy execution surface.
// New rules must implement RuleDefinition.
// This trait will be removed after all rule codes migrate.
pub trait Paradigm { ... }
```

`Paradigm::init` and `Paradigm::suggest` are unaffected; their reshape (if
ever) is a future epic.

### `PolicyDefinition`

```rust
pub trait PolicyDefinition: Send + Sync {
    fn id(&self) -> PolicyId;
    fn title(&self) -> &'static str;
    fn decide(&self, ctx: &PolicyContext<'_>) -> PolicyOutput;
}

pub struct PolicyOutput {
    pub decisions: Vec<Decision>,
    pub new_findings: Vec<RuleFinding>,   // policy-emitted findings
}

pub struct PolicyContext<'a> {
    pub air: &'a AirWorkspace,
    pub lockfile: &'a Lockfile,
    pub mode: CheckMode,
    pub rule_registry: &'a RuleRegistry,
    pub paradigm_registry: &'a ParadigmRegistry,
    pub policy_registry: &'a PolicyRegistry,
    pub findings: &'a FindingStore,
    pub prior_decisions: &'a [Decision],
    pub finding_ids: &'a FindingIdMinter,
}
```

### Registries

```rust
pub struct RuleRegistry      { /* Vec<&'static dyn RuleDefinition> */ }
pub struct ParadigmRegistry  { /* Vec<&'static dyn ParadigmDefinition> */ }
pub struct PolicyRegistry    { /* Vec<&'static dyn PolicyDefinition> */ }

impl RuleRegistry {
    pub fn standard() -> Self { /* validated at construction */ }
    pub fn find(&self, id: &RuleId) -> Option<&'static dyn RuleDefinition>;
    pub fn contains_code(&self, code: &str) -> bool;   // used by legacy adapter
    pub fn for_paradigm(&self, p: ParadigmId) -> impl Iterator<...>;
    pub fn iter(&self) -> ...;
}
```

**Construction-time validation** (returns structured error in CLI / runtime;
panics in tests for fail-fast). MVP can panic at static init; the design
contract is that registry construction returns a recoverable error where
practical:

- Every `RuleDefinition.id()` is distinct.
- Every `RuleDefinition.id().as_str()` starts with `RuleDefinition.paradigm().as_str()`
  (`CX001` → `CX`).
- Every `RuleDefinition.paradigm()` resolves in `ParadigmRegistry`.
- Every `ParadigmDefinition::rules()` entry is also in `RuleRegistry`.
- For every legacy `Paradigm` in the legacy registry there is a matching
  `ParadigmDefinition` with the same prefix.

**`PolicyRegistry::standard()` ordering:**

```
1. RegistryIntegrityPolicy   (P3 — see migration plan)
2. (future) ExceptionPolicy, TransitionPolicy, ArchitecturePolicy, ...
N. DefaultPassThroughPolicy  (always last)
```

The invariant:

> `DefaultPassThroughPolicy` is always last. It decides every finding that no
> prior policy decided. No other policy is permitted in the last slot.

`ParadigmRegistry::standard()` includes all current legacy paradigms with an
initially empty `rules()` slice. As rules migrate, they are appended to the
appropriate paradigm's `rules()` and removed (or gated) in the corresponding
legacy `Paradigm::check`.

## Pipeline

```rust
// locus-core::governance::pipeline

pub struct GovernanceOutput {
    pub diagnostics: Vec<Diagnostic>,   // emitted (suppressed decisions dropped)
    pub decisions: Vec<Decision>,       // full log incl. suppressed
    pub findings: FindingStore,
}

pub fn run(
    air: &AirWorkspace,
    lockfile: &Lockfile,
    mode: CheckMode,
) -> GovernanceOutput {
    let rules     = RuleRegistry::standard();
    let paradigms = ParadigmRegistry::standard();
    let policies  = PolicyRegistry::standard();
    let minter    = FindingIdMinter::new();
    let mut store = FindingStore::new();

    // Phase A: migrated rules observe.
    let rule_ctx = RuleContext { air, lockfile, mode,
                                 rule_registry: &rules,
                                 paradigm_registry: &paradigms,
                                 finding_ids: &minter };
    for rule in rules.iter() {
        for f in rule.observe(&rule_ctx) {
            store.insert(f);
        }
    }

    // Phase B: legacy adapter — per-diagnostic-code filter.
    LegacyParadigmRuleAdapter::run(
        &legacy_paradigm_registry(), air, lockfile, mode,
        &rules, &minter, &mut store,
    );

    // Phase C: policies in registry order. Single pass.
    let mut decisions: Vec<Decision> = Vec::new();
    for policy in policies.iter() {
        let pctx = PolicyContext {
            air, lockfile, mode,
            rule_registry: &rules,
            paradigm_registry: &paradigms,
            policy_registry: &policies,
            findings: &store,
            prior_decisions: &decisions,
            finding_ids: &minter,
        };
        let PolicyOutput { decisions: more, new_findings } = policy.decide(&pctx);
        for f in new_findings { store.insert(f); }
        decisions.extend(more);
    }

    validate_decisions(&decisions, &store);   // one decision per finding

    // Phase D: materialize.
    let diagnostics = decisions.iter()
        .filter_map(|d| materialize(d, &store))
        .collect();

    GovernanceOutput { diagnostics, decisions, findings: store }
}

fn materialize(decision: &Decision, store: &FindingStore) -> Option<Diagnostic> {
    if matches!(decision.status,
                DecisionStatus::SuppressedByPolicy | DecisionStatus::AcceptedException) {
        return None;
    }
    let f = store.get(decision.finding_id)?;
    let mut why = f.why.clone();
    why.extend(decision.rationale.iter().cloned());
    Some(Diagnostic {
        rule_id: emitted_rule_code(f),
        severity: decision.severity,
        span: f.span.clone().unwrap_or_else(synthetic_governance_span),
        concept: f.concept.clone(),
        message: f.message.clone(),
        why,
        suggested_fix: f.suggested_fix.clone(),
    })
}

fn emitted_rule_code(f: &RuleFinding) -> String {
    // Prefer the registered rule's id; fall back to the legacy code or the
    // policy id, depending on source. Legacy codes preserve verbatim so
    // existing snapshots match.
    match (&f.rule_id, &f.source) {
        (Some(r), _)                                              => r.as_str().to_string(),
        (None, FindingSource::LegacyDiagnostic { rule_code, .. }) => rule_code.clone(),
        (None, FindingSource::Policy(p))                          => p.as_str().to_string(),
        (None, FindingSource::RegisteredRule(r))                  => r.as_str().to_string(),
    }
}

fn synthetic_governance_span() -> AirSpan {
    // Explicit synthetic sentinel — NOT a real file. Reporters should treat
    // `<governance>` as a "no source location" marker rather than rendering
    // a fake file:line. Compat shim only; future spec will make
    // `Diagnostic.span` optional and remove this. Implementation may
    // construct via the existing `AirSpan::new("<governance>", 0, 0)`
    // pattern (matching how `LOCUS002` uses `locus.lock:1`) or via a new
    // `AirSpan::synthetic` constructor — the plan decides.
    AirSpan::new("<governance>", 0, 0)
}
```

### Legacy adapter

```rust
// locus-core::governance::legacy
//
// TRANSITIONAL. Removed when all paradigm rule codes migrate to RuleDefinition.

pub struct LegacyParadigmRuleAdapter;

impl LegacyParadigmRuleAdapter {
    pub fn run(
        paradigms: &[Box<dyn Paradigm>],
        air: &AirWorkspace,
        lockfile: &Lockfile,
        mode: CheckMode,
        rule_registry: &RuleRegistry,
        minter: &FindingIdMinter,
        store: &mut FindingStore,
    ) {
        for p in paradigms {
            let prefix = ParadigmId::new(p.rule_prefix());
            for diag in p.check(air, lockfile, mode) {
                // Per-diagnostic filter, not per-paradigm. If only some of
                // a paradigm's rule codes have migrated, the rest still go
                // through legacy synthesis.
                if rule_registry.contains_code(&diag.rule_id) {
                    continue;
                }
                store.insert(synthesize_legacy_finding(diag, prefix, minter));
            }
        }
    }
}
```

### `DefaultPassThroughPolicy`

```rust
impl PolicyDefinition for DefaultPassThroughPolicy {
    fn id(&self) -> PolicyId       { PolicyId::new("default-pass-through") }
    fn title(&self) -> &'static str { "Default Pass-Through" }

    fn decide(&self, ctx: &PolicyContext<'_>) -> PolicyOutput {
        let decided: HashSet<FindingId> =
            ctx.prior_decisions.iter().map(|d| d.finding_id).collect();

        let decisions = ctx.findings.iter()
            .filter(|f| !decided.contains(&f.id))
            .map(|f| Decision {
                finding_id: f.id,
                policy: self.id(),
                severity: f.default_severity,
                status: match f.default_severity {
                    Severity::Advisory => DecisionStatus::Advisory,
                    _                  => DecisionStatus::Active,
                },
                severity_change: SeverityChange::Unchanged,
                rationale: Vec::new(),       // legacy findings stay byte-identical
            })
            .collect();

        PolicyOutput { decisions, new_findings: Vec::new() }
    }
}
```

**Compat invariant:** when `DefaultPassThroughPolicy` decides a
`FindingSource::LegacyDiagnostic` finding, the materialized `Diagnostic` is
byte-identical to what the legacy `Paradigm::check` produced. The
materializer must not append rationale to legacy findings under pass-through.

## RegistryIntegrityPolicy (the first dogfood policy)

`RegistryIntegrityPolicy` is the first real policy. It governs Locus's own
governance abstractions: `Rule`, `Paradigm`, `Policy`. It reports the state
of the registries to the user; it does not enforce architecture mutation.

**Diagnostic code: `LOCUS003`** (slots alongside `LOCUS001` expired-exception
and `LOCUS002` vacant-paradigm). Owned by `RegistryIntegrityPolicy`.

> Governance-layer diagnostic codes are registered the same way rule codes
> are. `LOCUS003` is registered as a governance/policy diagnostic code owned
> by `RegistryIntegrityPolicy`. The principle: rules are registered,
> policies are registered, policy diagnostic codes are registered. A small
> static table is enough for MVP, but the contract matters.

### Checks emitted

Most invariants are enforced at registry-construction time (fail-fast in
tests, structured error in CLI). The policy re-affirms them at runtime as a
governance surface:

1. **Rule uniqueness** — every `RuleDefinition.id()` distinct.
2. **Rule ↔ paradigm prefix consistency** — `CX001` belongs to `CX`.
3. **Rule paradigm is registered** — `RuleDefinition.paradigm()` resolves.
4. **`Paradigm.rules()` lists registered rules** — every entry in
   `ParadigmDefinition::rules()` is also in `RuleRegistry`.
5. **Legacy paradigm parity** — every legacy `Paradigm` has a matching
   `ParadigmDefinition`. Missing parity emits Warning (`Active`).
6. **Migration debt visibility** — **one finding per unique legacy rule code**
   observed this run, status `KnownTransitionDebt`, severity `Advisory`.
   Message: `"rule code <CODE> emitted via legacy paradigm runner; not yet
   migrated to RuleDefinition (<N> observations this run)"`. Dedup by
   rule code is mandatory — otherwise migration-debt noise scales with the
   project's legacy diagnostic count.
7. **Policy IDs in decisions are registered** — every `Decision.policy`
   resolves in `PolicyRegistry`.

Checks 1–5 and 7 are typically silent (registry construction caught them).
Check 6 is the visible governance output of this spec.

### Strict-mode behavior

> `KnownTransitionDebt` remains `Advisory` under `--agent-strict` unless a
> future `TransitionDeadlinePolicy` elevates it.

This is a hard acceptance criterion. Without it, surfacing migration debt
breaks every dogfood run.

## Migration scope — rules

Three firm picks, two stretch.

| Rule | Why this one | Evidence variant exercised |
|------|--------------|----------------------------|
| **CX001** (per-fn line budget) | Structural; lockfile-driven; cleanest typed evidence | `Evidence::ComplexityBudget { lines, budget, override_match }` |
| **OT002** (inference-shaped canonical ownership) | Confidence-tier rule; proves `Confidence` enum carries through | `Evidence::InferenceConfidence { score: Confidence, signals }` |
| **DG001** (forbidden import) | Deterministic; lockfile-config-driven; concept-tagged | `Evidence::Structured(json)` with `{ from_symbol, to_symbol, edge_pattern }` |

Stretch:

- **MO005** (module membership) — only if cheap
- **FL003** (silent `.ok()/.err()` discard) — AIR-pattern matching, no
  lockfile config

> If DG001 migration balloons in scope (coupling with shared DG helpers
> turns out heavier than expected), swap it for **FL003**. The selection
> principle matters more than the exact third rule: one structural-budget
> rule, one inference/confidence rule, one deterministic
> lockfile-config-or-AIR-pattern rule.

PG-family and exception-suppression rules are deliberately not in this list.

## CLI integration

`crates/locus-cli/src/commands/check.rs` changes minimally:

```rust
// Was:
//   let mut all = Vec::new();
//   for paradigm in registry() {
//       all.extend(paradigm.check(&air, &lockfile, mode));
//   }
let out = locus_core::governance::run(&air, &lockfile, mode);
let mut all = out.diagnostics;

// Existing post-processing unchanged in this spec:
let all       = apply_exceptions(all, &air, &lockfile, Some(&today));
let mut all   = apply_changed_filter(all, &args)?;
append_policy_guard(&mut all, &lockfile, &args, mode)?;
```

`GovernanceOutput.decisions` and `.findings` are exposed but unused at the
CLI in MVP. Future epics consume them for `locus observe`, richer `locus
debt`, SARIF, etc.

## PR phasing

All three PRs are part of this single spec.

### P1 — Governance spine (no behavior change)

- All types from "Core types" and "Traits and registries".
- Pipeline (`governance::run`), `LegacyParadigmRuleAdapter`,
  `DefaultPassThroughPolicy`.
- `RuleRegistry::standard()` is empty.
- `ParadigmRegistry::standard()` includes all current legacy paradigms with
  empty `rules()`.
- `PolicyRegistry::standard()` contains only `DefaultPassThroughPolicy`.
- CLI rewired to call `governance::run`.
- Legacy `Paradigm::check` annotated as transitional in code +
  `CLAUDE.md` / `AGENTS.md`.

**Acceptance:** legacy-compatibility snapshots for
`tests/fixtures/sample-crate` and the Locus workspace itself match
byte-identically before/after. Snapshots are captured **before** the P1 PR
merges and checked in alongside it.

### P2 — Migrate three rules

One PR per rule (CX001, then OT002, then DG001 or FL003) to keep diffs
reviewable. Each rule PR:

- Adds `RuleDefinition` struct, registers in `RuleRegistry::standard()` and
  the corresponding `ParadigmDefinition::rules()`.
- **Deletes or disables** the corresponding code path in the legacy
  `paradigm.check()`. The invariant is "no double-fire," not "immediate
  deletion." Gating the legacy emission is acceptable if shared helper
  logic makes deletion messy.
- Moves / adapts per-rule tests to drive `rule.observe(&ctx)` directly.

**Acceptance:** legacy-compatibility snapshots remain byte-identical
(finding-id stamps are internal; on-the-wire diagnostics unchanged).

### P3 — RegistryIntegrityPolicy

- `RegistryIntegrityPolicy` added to `PolicyRegistry::standard()`, before
  pass-through.
- `LOCUS003` registered as a governance diagnostic code.
- New diagnostics are intentional. They are **not** mixed into the
  legacy-compatibility snapshot. They are captured in a separate
  `tests/snapshots/governance-diagnostics.*` (or equivalent) snapshot.

  > Compatibility snapshots cover pass-through behavior only.
  > Governance-policy snapshots cover intentional new output.

- Stretch rules (MO005, FL003) may be folded into P3 if scope allows;
  otherwise they spin out as their own PRs after P3.

Each PR is independently shippable. Each preserves the strangler invariant:
legacy paths and new paths produce identical user-visible output until a
governance policy explicitly says otherwise.

## Output stability contract

- **P1 / P2:** for every diagnostic emitted by the legacy CLI today, the
  new pipeline emits the same `rule_id`, `severity`, `span`, `concept`,
  `message`, `why`, `suggested_fix`. Verified by checked-in golden
  snapshots.
- **P3:** introduces new governance diagnostics (`LOCUS003` migration debt;
  any parity-gap warnings). These belong in a separate snapshot group, not
  merged into the legacy-compatibility snapshot.

## Forward-compat constraints for later epics

These are not implemented here, but the types must not block them:

- **`locus observe`** — must be able to run the pipeline with a no-op
  policy set that records findings without materializing diagnostics. The
  `FindingStore` + `GovernanceOutput` return shape already supports this.
- **Exception/Changed/PolicyGuard migration** — `DecisionStatus` has
  `SuppressedByPolicy` and `AcceptedException` slots ready. CLI
  post-processing relocates into policies without reshaping core types.
- **Multi-pass policies / fixed-point evaluation** — single-pass policy
  evaluation is MVP. The `PolicyContext` carries `prior_decisions`; future
  policies can be ordered or layered without changing the pipeline's
  interface.
- **Architecture archaeology** — `FindingSource::Policy` and structured
  `Evidence` provide a place for findings derived from git history without
  re-shaping the rule layer.

## Self-review checklist

- [x] Does this preserve current output in P1/P2? Yes — pass-through is
      last, byte-identical for legacy findings, golden snapshots required.
- [x] Does pass-through run last? Yes — invariant; `PolicyRegistry::standard()`
      enforces it.
- [x] Is every finding decided once? Yes — `validate_decisions` asserts it;
      duplicates are internal errors.
- [x] Are legacy diagnostics filtered per rule code? Yes — adapter's filter
      is per-`Diagnostic.rule_id`, not per-paradigm.
- [x] Are governance diagnostics separate from compatibility snapshots? Yes
      — P3 snapshots `LOCUS003` and friends separately.
- [x] Are later epics explicitly out of scope? Yes — observe, archaeology,
      mutation detection, transitions, fit/miss listed as non-goals;
      exception/changed/PG migration also non-goals.
- [x] `KnownTransitionDebt` non-fatal under `--agent-strict`? Yes —
      acceptance criterion.
- [x] Spanless findings use an explicit synthetic span, not a fake file
      path? Yes — `<governance>`.
- [x] Governance-layer diagnostic codes registered? Yes — `LOCUS003` owned
      by `RegistryIntegrityPolicy`.
- [x] Registry construction validates invariants without panicking in
      production? Yes — design contract; MVP may panic at static init,
      production should surface structured errors where practical.

## Out of scope (re-stated for clarity)

- `locus observe` mode.
- Git-history archaeology.
- Architecture mutation detection.
- Declared transitions.
- Architecture fit/miss assessment.
- Migration of exception suppression, `--changed` filtering, or Policy
  Guard into `PolicyDefinition`.
- Migration of PG-family rules.
- Public Cargo release.

Each of those gets its own spec.
