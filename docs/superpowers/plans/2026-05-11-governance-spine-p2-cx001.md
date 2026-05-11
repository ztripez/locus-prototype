# Governance Spine P2 — CX001 Migration Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate CX001 (function-line-budget rule) from the legacy `Paradigm::check()` path to a registered `RuleDefinition` impl emitting typed `Evidence::ComplexityBudget` findings. After this PR, `locus check` output remains byte-identical, but CX001's path is `RuleDefinition::observe` → `RuleFinding` → `DefaultPassThroughPolicy` → `Diagnostic` instead of going through the legacy adapter.

**Architecture:** First per-rule migration of the strangler. `Cx001Rule` struct implements `RuleDefinition`, registered in `RuleRegistry::standard()` and `CxParadigmDef::rules()`. The legacy `rules::cx001` function is removed; the `cx001` call is removed from `ComplexityBudget::check`. The legacy adapter's per-diagnostic-code filter now sees `CX001` in the rule registry and skips it — confirming the strangler invariant in action for the first time.

**Tech Stack:** Rust 2024 edition, existing governance spine from P1 (#79). No new dependencies.

**Spec:** [docs/superpowers/specs/2026-05-11-governance-spine-design.md](../specs/2026-05-11-governance-spine-design.md) §"Migration scope — rules" + §"PR phasing → P2".

**Prerequisite:** P1 merged (commit `a000e15`). `RuleRegistry`, `RuleDefinition`, `Evidence::ComplexityBudget`, `CxParadigmDef` all already exist.

---

## File structure (P2-CX001)

**Create:**
- `crates/locus-core/src/paradigms/complexity_budget/rules/mod.rs` — sub-module root
- `crates/locus-core/src/paradigms/complexity_budget/rules/cx001.rs` — `Cx001Rule` struct + `RuleDefinition` impl + per-rule tests

**Modify:**
- `crates/locus-core/src/paradigms/complexity_budget/mod.rs` — register the new sub-module path; remove the `rules::cx001(...)` call from `ComplexityBudget::check`
- `crates/locus-core/src/paradigms/complexity_budget/rules.rs` — remove `pub fn cx001`, `cx001_check_file`, `cx001_diagnostic`, `cx001_why`. Module renamed to keep CX002/007/008 intact.
- `crates/locus-core/src/paradigms/complexity_budget/rules_tests.rs` — port the 11 CX001 tests to drive `Cx001Rule::observe`; keep CX002/007/008 tests untouched.
- `crates/locus-core/src/governance/registry.rs` — register `Cx001Rule` in `RuleRegistry::standard()`.
- `crates/locus-core/src/governance/paradigm_impls.rs` — populate `CxParadigmDef::rules()` slice with `&Cx001Rule`.

**Note on the module layout:**

Today CX has a single `rules.rs` file. OT already uses the per-rule pattern (`paradigms/one_truth/rules/ot001.rs`, `ot002.rs`, …). We follow that pattern: create a `rules/` directory holding `mod.rs` + `cx001.rs`. The existing `rules.rs` file gets renamed to `rules/legacy.rs` (or stays as `rules.rs` if we keep CX002/007/008 there alongside `pub mod cx001`). The simplest path:

- Keep `rules.rs` as-is (still holding CX002/007/008).
- Add `rules/cx001.rs` as a sibling, exposed via `rules.rs` having `pub mod cx001;` and forwarding.

Wait — Rust can't have both `rules.rs` AND `rules/` next to each other at the same level. Two options:

**Option A — promote `rules.rs` to a directory.** Move `rules.rs` → `rules/mod.rs`. Add `rules/cx001.rs` as a sibling. Existing `cx002`/`cx007`/`cx008` stay in `rules/mod.rs` until they migrate too.

**Option B — keep `rules.rs` and put CX001 elsewhere.** E.g. add a new top-level `cx_rules` directory. Awkward.

This plan uses **Option A** — promotes `rules.rs` to a directory. Less surprise; matches OT's existing pattern.

So the final file map is:

- **Move (rename):** `crates/locus-core/src/paradigms/complexity_budget/rules.rs` → `crates/locus-core/src/paradigms/complexity_budget/rules/mod.rs`
- **Move (rename):** `crates/locus-core/src/paradigms/complexity_budget/rules_tests.rs` stays — it's a sibling file, not under `rules/`.
- **Create:** `crates/locus-core/src/paradigms/complexity_budget/rules/cx001.rs`

---

## Acceptance criteria (P2-CX001)

- `cargo build --workspace` succeeds.
- `cargo test --workspace` passes (no test regressions; CX001 tests now drive `Cx001Rule::observe`).
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `cargo fmt --all -- --check` clean.
- `cargo test -p locus-cli --test governance_compat` — compatibility snapshot still byte-identical.
- `cargo run -p locus-cli -- check --workspace . --agent-strict` exits 0 with the same 103 warnings baseline as P1 merged (zero new findings on the migrated code).
- `RuleRegistry::standard().contains_code("CX001")` returns `true` (new invariant test).
- The legacy adapter's per-diagnostic-code filter now skips CX001 — verified by an integration test that asserts a known CX001 violation flows through `RuleDefinition`, not through `LegacyParadigmRuleAdapter` (check the `FindingSource` of the resulting finding via `governance::run`).

---

## Task 1: Worktree setup

**Files:** none (environment only)

- [ ] **Step 1: Create the isolated worktree**

The session controller MUST create a worktree before the implementer begins. Run via the controller's worktree skill (not the implementer):

Expected outcome: working directory becomes `/mnt/code/projects/locus/.claude/worktrees/governance-spine-p2-cx001` or similar, on branch `worktree-governance-spine-p2-cx001`.

- [ ] **Step 2: Verify baseline tests pass**

Run: `cargo test --workspace 2>&1 | grep "test result:" | awk '{passed += $4; failed += $6} END {print "passed:", passed, "failed:", failed}'`
Expected: `passed: 1153 failed: 0` (same baseline as post-P1).

- [ ] **Step 3: Capture pre-migration baseline outputs**

```bash
mkdir -p /tmp/locus-p2-cx001-baseline
cargo run -p locus-cli --quiet -- check --workspace tests/fixtures/sample-crate \
    > /tmp/locus-p2-cx001-baseline/sample-crate.txt 2>&1
cargo run -p locus-cli --quiet -- check --workspace tests/fixtures/sample-crate --agent-strict \
    > /tmp/locus-p2-cx001-baseline/sample-crate-strict.txt 2>&1
cargo run -p locus-cli --quiet -- check --workspace . \
    > /tmp/locus-p2-cx001-baseline/self.txt 2>&1
cargo run -p locus-cli --quiet -- check --workspace . --agent-strict \
    > /tmp/locus-p2-cx001-baseline/self-strict.txt 2>&1
```

These are the "before" recordings for the byte-identical check at the end. No commit.

---

## Task 2: Promote `rules.rs` to `rules/` directory

**Files:**
- Rename: `crates/locus-core/src/paradigms/complexity_budget/rules.rs` → `crates/locus-core/src/paradigms/complexity_budget/rules/mod.rs`

- [ ] **Step 1: Move the file**

```bash
cd crates/locus-core/src/paradigms/complexity_budget
mkdir rules
git mv rules.rs rules/mod.rs
cd -
```

- [ ] **Step 2: Verify the build still works**

Run: `cargo build -p locus-core 2>&1 | tail -3`
Expected: clean build. No source changes were needed — Rust treats `rules/mod.rs` as equivalent to `rules.rs` for the `pub mod rules;` declaration in the parent.

- [ ] **Step 3: Verify CX tests still pass**

Run: `cargo test -p locus-core --lib paradigms::complexity_budget 2>&1 | grep "test result:"`
Expected: same pass count as before the move.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "refactor(#71): promote complexity_budget rules.rs to rules/ directory"
```

---

## Task 3: Add `Cx001Rule` stub + first failing test

**Files:**
- Create: `crates/locus-core/src/paradigms/complexity_budget/rules/cx001.rs`
- Modify: `crates/locus-core/src/paradigms/complexity_budget/rules/mod.rs` (add `pub mod cx001;`)

- [ ] **Step 1: Write a stub Cx001Rule and a failing test**

Create `crates/locus-core/src/paradigms/complexity_budget/rules/cx001.rs`:

```rust
//! CX001 — function exceeds its line budget.
//!
//! Migrated to `RuleDefinition` in P2 (epic #71). Replaces the legacy
//! `super::cx001()` function. Walks `AirItem::Function` items, compares
//! each function's `line_count` against the effective budget (override or
//! workspace default or built-in fallback), and emits a `RuleFinding`
//! with `Evidence::ComplexityBudget` for each function that overshoots.

// locus: ot canonical

use crate::diagnostics::Severity;
use crate::governance::evidence::Evidence;
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

pub struct Cx001Rule;

pub static CX001_RULE: Cx001Rule = Cx001Rule;

const CX001_ID: RuleId = RuleId::new("CX001");
const CX_PARADIGM: ParadigmId = ParadigmId::new("CX");

impl RuleDefinition for Cx001Rule {
    fn id(&self) -> RuleId {
        CX001_ID
    }
    fn paradigm(&self) -> ParadigmId {
        CX_PARADIGM
    }
    fn title(&self) -> &'static str {
        "function exceeds its line budget"
    }
    fn default_severity(&self) -> Severity {
        Severity::Warning
    }
    fn observe(&self, _ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        // Implemented in Task 4.
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::CheckMode;
    use crate::governance::ids::FindingIdMinter;
    use crate::governance::registry::{ParadigmRegistry, RuleRegistry};
    use crate::lockfile::Lockfile;
    use locus_air::{AirFile, AirFunction, AirItem, AirPackage, AirSpan, AirWorkspace, Visibility};

    /// Build a workspace with one function whose line_count overshoots the
    /// 50-line built-in fallback budget. The migrated rule should emit
    /// exactly one finding.
    #[test]
    fn fires_on_function_over_built_in_fallback_budget() {
        let air = workspace_with_function("crate_a::module_b::overlong_fn", 73);
        let lf = Lockfile::empty();
        let rules = RuleRegistry::standard();
        let paradigms = ParadigmRegistry::empty();
        let minter = FindingIdMinter::new();
        let ctx = RuleContext {
            air: &air,
            lockfile: &lf,
            mode: CheckMode::Human,
            rule_registry: &rules,
            paradigm_registry: &paradigms,
            finding_ids: &minter,
        };

        let findings = Cx001Rule.observe(&ctx);
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.source, FindingSource::RegisteredRule(RuleId::new("CX001")));
        assert_eq!(f.rule_id, Some(RuleId::new("CX001")));
        assert_eq!(f.paradigm_id, Some(ParadigmId::new("CX")));
        assert_eq!(f.default_severity, Severity::Warning);
        assert!(f.message.contains("overlong_fn"));
        assert!(f.message.contains("73 lines"));
        assert!(f.message.contains("budget 50"));

        // Evidence is typed.
        assert_eq!(f.evidence.len(), 1);
        match &f.evidence[0] {
            Evidence::ComplexityBudget {
                lines,
                budget,
                override_match,
            } => {
                assert_eq!(*lines, 73);
                assert_eq!(*budget, 50);
                assert_eq!(*override_match, None);
            }
            other => panic!("expected ComplexityBudget evidence, got {other:?}"),
        }
    }

    /// Test helper: build a one-file workspace containing a single function
    /// with the given symbol and line_count.
    fn workspace_with_function(symbol: &str, line_count: u32) -> AirWorkspace {
        AirWorkspace::new(vec![AirPackage {
            name: "crate_a".to_string(),
            files: vec![AirFile {
                path: "src/module_b.rs".to_string(),
                module_path: Some("crate_a::module_b".to_string()),
                items: vec![AirItem::Function(AirFunction {
                    symbol: symbol.to_string(),
                    visibility: Visibility::Public,
                    span: AirSpan::new("src/module_b.rs", 1, line_count),
                    line_count,
                    decorators: Vec::new(),
                    path_segments: Vec::new(),
                    symbol_segments: Vec::new(),
                    callees: Vec::new(),
                    discards: Vec::new(),
                    fallbacks: Vec::new(),
                })],
                imports: Vec::new(),
                facts: Vec::new(),
                hints: Vec::new(),
            }],
        }])
    }
}
```

**Note:** the helper assumes a specific shape for `AirPackage`/`AirFile`/`AirFunction`. The actual field set may differ — check `crates/locus-air/src/lib.rs` for the current struct definitions before writing the helper. Adjust missing/extra fields. If `Visibility::Public` or any other constructor doesn't exist by that path, look up the correct one.

- [ ] **Step 2: Wire into rules/mod.rs**

Edit `crates/locus-core/src/paradigms/complexity_budget/rules/mod.rs`. Find the top of the file (the existing doc-comment block and `use` statements) and immediately after the `use` statements, add:

```rust
pub mod cx001;
```

- [ ] **Step 3: Run the failing test to confirm the harness works**

Run: `cargo test -p locus-core paradigms::complexity_budget::rules::cx001 -- --nocapture`
Expected: 1 test fails (assertion fires on `findings.len() == 0`, expected `1`). This confirms the test compiles and runs against the stub.

If the test fails to compile (e.g. `AirFunction` field set has drifted), fix the helper to match the current `locus-air` schema. Run `cargo build -p locus-core --tests` to see compile errors, fix, re-run.

- [ ] **Step 4: Commit the failing test**

```bash
git add crates/locus-core/src/paradigms/complexity_budget/rules/
git commit -m "test(#71): failing test for Cx001Rule migration"
```

---

## Task 4: Implement `Cx001Rule::observe`

**Files:**
- Modify: `crates/locus-core/src/paradigms/complexity_budget/rules/cx001.rs`

- [ ] **Step 1: Port the legacy implementation into `observe`**

The legacy `super::cx001()` lives in `rules/mod.rs` (post-rename). Read it for reference; the new version replicates the logic but produces `RuleFinding`s.

Replace the stub `observe` in `cx001.rs` with:

```rust
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::super::lockfile_schema::{CxOverride, CxSection};
        let section: CxSection = ctx
            .lockfile
            .paradigm_section("CX")
            .unwrap_or_default();
        let default_budget = section.effective_default();
        let mut out = Vec::new();
        for pkg in &ctx.air.packages {
            for file in &pkg.files {
                let Some(module_path) = file.module_path.as_deref() else {
                    continue;
                };
                check_file(file, module_path, &section, default_budget, ctx, &mut out);
            }
        }
        out
    }
}

fn check_file(
    file: &locus_air::AirFile,
    module_path: &str,
    section: &super::super::lockfile_schema::CxSection,
    default_budget: u32,
    ctx: &RuleContext<'_>,
    out: &mut Vec<RuleFinding>,
) {
    use super::super::lockfile_schema::CxOverride;
    use locus_air::AirItem;

    let matched_override = section.matching_override(module_path);
    let budget = matched_override
        .map(|o| o.max_function_lines)
        .unwrap_or(default_budget);
    let narrowed = matched_override.is_some() || section.default_max_function_lines.is_some();
    for item in &file.items {
        let AirItem::Function(func) = item else {
            continue;
        };
        if func.line_count <= budget {
            continue;
        }
        out.push(make_finding(
            func,
            budget,
            matched_override,
            narrowed,
            section,
            default_budget,
            ctx,
        ));
    }
}

fn make_finding(
    func: &locus_air::AirFunction,
    budget: u32,
    matched_override: Option<&super::super::lockfile_schema::CxOverride>,
    narrowed: bool,
    section: &super::super::lockfile_schema::CxSection,
    default_budget: u32,
    ctx: &RuleContext<'_>,
) -> RuleFinding {
    let severity = ctx.mode.elevate_when_actionable(Severity::Warning, narrowed);
    let message = format!(
        "function `{}` is {} lines, budget {} ({})",
        func.symbol,
        func.line_count,
        budget,
        match matched_override {
            Some(o) => format!("override `{}`", o.module),
            None => "workspace default".to_string(),
        }
    );
    let mut why = vec![
        format!("function `{}` spans {} line(s)", func.symbol, func.line_count),
        if let Some(o) = matched_override {
            format!("budget {budget} from override `module = {}`", o.module)
        } else {
            format!("budget {budget} (workspace default)")
        },
    ];
    if matched_override.is_none() && section.default_max_function_lines.is_none() {
        why.push(format!(
            "no `default_max_function_lines` configured; using built-in fallback {}",
            default_budget
        ));
    }
    RuleFinding {
        id: ctx.finding_ids.next(),
        source: FindingSource::RegisteredRule(CX001_ID),
        rule_id: Some(CX001_ID),
        paradigm_id: Some(CX_PARADIGM),
        default_severity: severity,
        span: Some(func.span.clone()),
        concept: None,
        message,
        evidence: vec![Evidence::ComplexityBudget {
            lines: func.line_count,
            budget,
            override_match: matched_override.map(|o| o.module.clone()),
        }],
        why,
        suggested_fix: Some(
            "split the function into smaller steps each owning one decision, \
             or — if this length is intended (e.g. a parser arm or state \
             machine) — raise the budget by adding an override to \
             `paradigms.CX.overrides` in `locus.lock`"
                .into(),
        ),
    }
}
```

The `default_severity` field of `RuleFinding` is what the pipeline's pass-through policy uses to set the emitted `Diagnostic`'s severity. The legacy `cx001_diagnostic` set `severity: mode.elevate_when_actionable(...)` directly on the Diagnostic — to preserve byte-identical output, the rule must compute the elevation here and pass it through. The `default_severity` name is a historical mismatch; here it's the *effective* severity after mode elevation.

**Watch out for `matching_override` — it might return `Option<&CxOverride>`. If it returns owned values or differs, adjust.**

- [ ] **Step 2: Run the test**

Run: `cargo test -p locus-core paradigms::complexity_budget::rules::cx001 -- --nocapture`
Expected: PASS.

If the test fails on the message or budget number, check that `effective_default()` returns 50 when `default_max_function_lines` is `None`. The legacy code matched this; the new code should too.

- [ ] **Step 3: Clippy + commit**

Run: `cargo clippy -p locus-core --all-targets -- -D warnings`
Expected: clean.

```bash
git add crates/locus-core/src/paradigms/complexity_budget/rules/cx001.rs
git commit -m "feat(#71): Cx001Rule observes Evidence::ComplexityBudget"
```

---

## Task 5: Register `Cx001Rule` in the rule registry

**Files:**
- Modify: `crates/locus-core/src/governance/registry.rs`
- Modify: `crates/locus-core/src/governance/paradigm_impls.rs`

- [ ] **Step 1: Wire `Cx001Rule` into `RuleRegistry::standard()`**

Edit `crates/locus-core/src/governance/registry.rs`. Find `impl RuleRegistry` → `pub fn standard()`:

```rust
    /// Empty registry. P1 wires no migrated rules.
    pub fn standard() -> Self {
        Self { rules: Vec::new() }
    }
```

Replace with:

```rust
    /// Migrated rules. Grows as rules move from legacy `Paradigm::check`
    /// to `RuleDefinition` impls. CX001 lands in P2; others follow in
    /// subsequent PRs.
    pub fn standard() -> Self {
        Self {
            rules: vec![&crate::paradigms::complexity_budget::rules::cx001::CX001_RULE],
        }
    }
```

- [ ] **Step 2: Populate `CxParadigmDef::rules()`**

Edit `crates/locus-core/src/governance/paradigm_impls.rs`. The `paradigm_def!` macro generates `fn rules(&self) -> &'static [&'static dyn RuleDefinition] { &[] }` for every paradigm. We need to special-case CX to return `&[&CX001_RULE]`.

Replace the `paradigm_def!(CxParadigmDef, "CX", "Complexity Budget");` line with an explicit impl:

```rust
pub struct CxParadigmDef;
impl ParadigmDefinition for CxParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("CX")
    }
    fn title(&self) -> &'static str {
        "Complexity Budget"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        &[&crate::paradigms::complexity_budget::rules::cx001::CX001_RULE]
    }
}
```

(Other paradigms keep using the macro. CX is the first to break out of the macro because it has a non-empty `rules()`.)

- [ ] **Step 3: Run the registry's parity test**

Run: `cargo test -p locus-core --lib governance::registry`
Expected: 7 tests pass (existing 6 + nothing new but Cx001Rule must satisfy uniqueness + prefix-match invariants).

- [ ] **Step 4: Add a new registry test asserting CX001 is registered**

Append to `crates/locus-core/src/governance/registry.rs` test module:

```rust
    #[test]
    fn rule_registry_contains_cx001_after_p2_migration() {
        let reg = RuleRegistry::standard();
        assert!(reg.contains_code("CX001"));
        let rule = reg.find(&RuleId::new("CX001")).expect("CX001 missing");
        assert_eq!(rule.paradigm().as_str(), "CX");
        assert_eq!(rule.default_severity(), crate::diagnostics::Severity::Warning);
    }
```

Run: `cargo test -p locus-core --lib governance::registry`
Expected: 8 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/locus-core/src/governance/registry.rs crates/locus-core/src/governance/paradigm_impls.rs
git commit -m "feat(#71): register Cx001Rule in RuleRegistry::standard() and CX paradigm"
```

---

## Task 6: Verify legacy adapter now skips CX001

At this point both paths run CX001:
1. New: `Cx001Rule::observe` (registered) emits a `RuleFinding`.
2. Legacy: `ComplexityBudget::check` still calls `rules::cx001` (not removed yet) → legacy adapter would normally wrap it… BUT now the per-diagnostic-code filter sees `RuleRegistry::contains_code("CX001") == true` and SKIPS the legacy CX001 diagnostics.

This is the strangler in action. Verify it before removing the legacy call.

**Files:**
- Create: `crates/locus-core/tests/governance_cx001_strangler.rs` (new integration test)

- [ ] **Step 1: Write an integration test asserting CX001 is sourced from the registered rule**

Create `crates/locus-core/tests/governance_cx001_strangler.rs`:

```rust
//! Verifies the strangler invariant: after CX001 migrates to
//! RuleDefinition, every CX001 finding in the governance pipeline comes
//! from the registered rule (FindingSource::RegisteredRule), NOT from
//! the legacy adapter (FindingSource::LegacyDiagnostic).

use locus_core::diagnostics::CheckMode;
use locus_core::governance;
use locus_core::governance::finding::FindingSource;
use locus_core::governance::ids::RuleId;
use locus_core::lockfile::Lockfile;
use locus_air::{AirFile, AirFunction, AirItem, AirPackage, AirSpan, AirWorkspace, Visibility};

#[test]
fn cx001_findings_come_from_registered_rule_not_legacy_adapter() {
    // Workspace with one overlong function.
    let air = AirWorkspace::new(vec![AirPackage {
        name: "demo".into(),
        files: vec![AirFile {
            path: "src/mod.rs".into(),
            module_path: Some("demo::module".into()),
            items: vec![AirItem::Function(AirFunction {
                symbol: "demo::module::big_fn".into(),
                visibility: Visibility::Public,
                span: AirSpan::new("src/mod.rs", 1, 200),
                line_count: 200,
                decorators: Vec::new(),
                path_segments: Vec::new(),
                symbol_segments: Vec::new(),
                callees: Vec::new(),
                discards: Vec::new(),
                fallbacks: Vec::new(),
            })],
            imports: Vec::new(),
            facts: Vec::new(),
            hints: Vec::new(),
        }],
    }]);
    let lf = Lockfile::empty();

    let out = governance::run(&air, &lf, CheckMode::Human);

    // Collect all CX001 findings from the store.
    let cx001_findings: Vec<_> = out
        .findings
        .iter()
        .filter(|f| {
            matches!(&f.rule_id, Some(r) if *r == RuleId::new("CX001"))
                || matches!(
                    &f.source,
                    FindingSource::LegacyDiagnostic { rule_code, .. } if rule_code == "CX001"
                )
        })
        .collect();

    // Expect exactly one CX001 finding for the one overlong function.
    assert_eq!(cx001_findings.len(), 1, "expected exactly one CX001 finding");

    // It must be RegisteredRule, not LegacyDiagnostic.
    match &cx001_findings[0].source {
        FindingSource::RegisteredRule(r) => {
            assert_eq!(r.as_str(), "CX001");
        }
        FindingSource::LegacyDiagnostic { rule_code, .. } => {
            panic!(
                "CX001 finding still flows through legacy adapter (rule_code={rule_code}); \
                 strangler filter is not working"
            );
        }
        other => panic!("unexpected source: {other:?}"),
    }
}
```

Adjust the AIR helper if `AirFunction` field names differ from what's shown.

- [ ] **Step 2: Run the test — expect it to FAIL (or pass with TWO findings)**

Run: `cargo test -p locus-core --test governance_cx001_strangler 2>&1 | tail -10`

Expected: FAIL with one of:
- `assertion failed: expected exactly one CX001 finding` (because both paths emitted; the assertion count is 2)
- OR the finding came from `LegacyDiagnostic` (the new rule path's finding loses to the legacy one due to insertion-order semantics; check which) — this should NOT happen because `Cx001Rule::observe` runs in Phase A before the legacy adapter in Phase B; the legacy adapter then skips CX001.

If the test passes already (`exactly one CX001 finding` AND `FindingSource::RegisteredRule`), excellent — the strangler is already working because we registered the rule in Task 5. The legacy `rules::cx001` still runs from `ComplexityBudget::check` but the adapter drops its output. Move to Task 7.

If two findings appear, it means Phase A's rule observation produced one finding AND Phase B's legacy adapter ALSO synthesized one. That would indicate `rule_registry.contains_code("CX001")` returns false at the call site. Check that `RuleRegistry::standard()` from Task 5 actually wires `CX001_RULE`.

- [ ] **Step 3: Commit (test stays even if it passes)**

```bash
git add crates/locus-core/tests/governance_cx001_strangler.rs
git commit -m "test(#71): assert CX001 findings come from RegisteredRule, not legacy adapter"
```

---

## Task 7: Remove legacy `rules::cx001` call from `ComplexityBudget::check`

**Files:**
- Modify: `crates/locus-core/src/paradigms/complexity_budget/mod.rs`

- [ ] **Step 1: Remove the `cx001` invocation**

In `crates/locus-core/src/paradigms/complexity_budget/mod.rs`, find:

```rust
    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic> {
        let section: lockfile_schema::CxSection =
            lockfile.paradigm_section(CX_PREFIX).unwrap_or_default();
        let mut diags = rules::cx001(air, &section, mode);
        diags.extend(rules::cx002(air, &section, mode));
        diags.extend(rules::cx007(air, &section, mode));
        diags.extend(rules::cx008(air, &section, mode));
        diags
    }
```

Replace with:

```rust
    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic> {
        let section: lockfile_schema::CxSection =
            lockfile.paradigm_section(CX_PREFIX).unwrap_or_default();
        // CX001 migrated to RuleDefinition (#71 P2). Run via the governance
        // pipeline; the legacy adapter's per-rule-code filter keeps the
        // strangler invariant by dropping any CX001 diagnostic emitted here.
        let mut diags = rules::cx002(air, &section, mode);
        diags.extend(rules::cx007(air, &section, mode));
        diags.extend(rules::cx008(air, &section, mode));
        diags
    }
```

- [ ] **Step 2: Run full test sweep**

Run: `cargo test --workspace 2>&1 | grep "test result:" | awk '{passed += $4; failed += $6} END {print "passed:", passed, "failed:", failed}'`
Expected: same overall pass count (existing CX001 tests in `rules_tests.rs` may break if they call `rules::cx001(...)` directly — that's Task 8). For now: ALL existing-CX001-tests breaking is expected; everything else must still pass.

If non-CX001 tests fail, stop and investigate.

- [ ] **Step 3: Verify the compatibility snapshot still matches**

Run: `cargo test -p locus-cli --test governance_compat 2>&1 | tail -3`
Expected: PASS — sample-crate output is byte-identical because the new rule emits the same diagnostic content as the old function did.

If it fails, the rule's output diverges from the legacy diagnostic in some field. Diff `cargo run -p locus-cli -- check --workspace tests/fixtures/sample-crate` against `/tmp/locus-p2-cx001-baseline/sample-crate.txt` to find the drift.

- [ ] **Step 4: Commit**

```bash
git add crates/locus-core/src/paradigms/complexity_budget/mod.rs
git commit -m "feat(#71): remove legacy rules::cx001 call from ComplexityBudget::check"
```

---

## Task 8: Migrate existing CX001 tests to drive `Cx001Rule::observe`

**Files:**
- Modify: `crates/locus-core/src/paradigms/complexity_budget/rules_tests.rs`

- [ ] **Step 1: Inspect the existing CX001 tests**

Run: `grep -n "fn cx001_\|^use\|cx001(" crates/locus-core/src/paradigms/complexity_budget/rules_tests.rs | head -40`

Identify all `#[test] fn cx001_*` functions (there are 11 per pre-migration inspection). They all follow the pattern:

```rust
let diags = cx001(&air, &section, CheckMode::Human);
assert!(!diags.is_empty());
assert_eq!(diags[0].rule_id, "CX001");
```

- [ ] **Step 2: Add a test-only helper to call `Cx001Rule::observe`**

At the top of `rules_tests.rs` (after existing `use` statements), add:

```rust
#[cfg(test)]
use crate::governance::finding::RuleFinding;
#[cfg(test)]
use crate::governance::ids::{FindingIdMinter, RuleId};
#[cfg(test)]
use crate::governance::registry::{ParadigmRegistry, RuleRegistry};
#[cfg(test)]
use crate::governance::rule::{RuleContext, RuleDefinition};
#[cfg(test)]
use crate::paradigms::complexity_budget::rules::cx001::Cx001Rule;
#[cfg(test)]
use crate::lockfile::Lockfile;

/// Build a CheckMode-parameterized `Vec<RuleFinding>` for CX001.
/// Replaces the legacy `cx001(&air, &section, mode)` call in tests.
///
/// NOTE: `section` is no longer passed directly — the rule reads it from
/// the lockfile via `ctx.lockfile.paradigm_section("CX")`. To reproduce
/// the old "build a CxSection by hand and pass to cx001" pattern, build
/// a lockfile with the section embedded.
#[cfg(test)]
fn observe_cx001(air: &AirWorkspace, section: &CxSection, mode: CheckMode) -> Vec<RuleFinding> {
    let mut lf = Lockfile::empty();
    lf.set_paradigm_section("CX", serde_json::to_value(section).unwrap());
    let rules = RuleRegistry::standard();
    let paradigms = ParadigmRegistry::empty();
    let minter = FindingIdMinter::new();
    let ctx = RuleContext {
        air,
        lockfile: &lf,
        mode,
        rule_registry: &rules,
        paradigm_registry: &paradigms,
        finding_ids: &minter,
    };
    Cx001Rule.observe(&ctx)
}
```

**Verify `Lockfile::set_paradigm_section` exists** before adopting. If it doesn't, find or add a method that lets a test stash a serialized section into the lockfile. Look at existing tests that build a lockfile with paradigm sections for reference (e.g. `paradigms/one_truth/rules_tests.rs`).

If `set_paradigm_section` isn't available, the alternative is to bypass the lockfile and call the legacy `cx001()`-style logic directly via a private helper exposed for tests. Don't go that route; if needed, add `Lockfile::set_paradigm_section` and commit it separately first.

- [ ] **Step 3: Migrate each CX001 test, one at a time**

For each `fn cx001_*` test, replace the call shape:

```rust
let diags = cx001(&air, &section, CheckMode::Human);
```

with:

```rust
let findings = observe_cx001(&air, &section, CheckMode::Human);
```

And replace assertions on `diags[0].rule_id`, `.severity`, `.message`, etc. with the equivalent on `findings[0].rule_id` (which is `Option<RuleId>`, not `String`), `.default_severity`, `.message`. Mapping table:

| Legacy `Diagnostic` field | New `RuleFinding` field |
|---|---|
| `d.rule_id == "CX001"` | `f.rule_id == Some(RuleId::new("CX001"))` |
| `d.severity` | `f.default_severity` |
| `d.message` | `f.message` |
| `d.span` | `f.span.as_ref().unwrap()` (note: `Option<AirSpan>`) |
| `d.why` | `f.why` |
| `d.suggested_fix` | `f.suggested_fix` |

Walk through each test. Adjust comparisons; add at least one assertion per test on the typed `Evidence::ComplexityBudget` (proves the migration carried the right data).

For tests that assert `diags.is_empty()`, change to `findings.is_empty()`.

Commit each migrated test in its own commit if it's a big test, or batch 3-4 small ones. Aim for ≤5 commits total in this task.

- [ ] **Step 4: Run all CX001 tests**

Run: `cargo test -p locus-core --lib paradigms::complexity_budget::rules_tests 2>&1 | grep "test result:"`
Expected: same pass count as pre-migration (11+ tests, all passing).

- [ ] **Step 5: Commit final test migration**

```bash
git add crates/locus-core/src/paradigms/complexity_budget/rules_tests.rs
git commit -m "test(#71): migrate CX001 rules_tests to drive Cx001Rule::observe"
```

---

## Task 9: Remove legacy `pub fn cx001` (and helpers)

**Files:**
- Modify: `crates/locus-core/src/paradigms/complexity_budget/rules/mod.rs`

- [ ] **Step 1: Verify no callers remain**

Run: `grep -rn "rules::cx001\|::cx001(" crates/ | grep -v "rules/cx001" | grep -v "_tests\|tests::"`

Expected: zero hits. If anything remains, address it before deleting.

- [ ] **Step 2: Delete the legacy functions**

In `crates/locus-core/src/paradigms/complexity_budget/rules/mod.rs`, delete:
- `fn cx001_why(...)` (lines ~24-47)
- `fn cx001_check_file(...)` (lines ~70-107)
- `pub fn cx001(...)` (lines ~109-121)
- `fn cx001_diagnostic(...)` (lines ~123-155)

Also delete any imports that become unused (e.g. `super::lockfile_schema::CxOverride`, `Visibility`, `AirSpan`) if they were only used by the deleted functions.

The file goes from ~520 lines to ~360 — CX001 logic moves entirely into `rules/cx001.rs`.

- [ ] **Step 3: Build + test**

Run: `cargo build -p locus-core 2>&1 | tail -3`
Expected: clean build (or compilation errors revealing unused imports — fix them).

Run: `cargo test -p locus-core 2>&1 | grep "test result:" | awk '{passed += $4; failed += $6} END {print "passed:", passed, "failed:", failed}'`
Expected: same pass count as Task 8.

- [ ] **Step 4: Commit**

```bash
git add crates/locus-core/src/paradigms/complexity_budget/rules/mod.rs
git commit -m "refactor(#71): remove legacy rules::cx001 (replaced by Cx001Rule)"
```

---

## Task 10: Full sweep + diff check

**Files:** none new

- [ ] **Step 1: fmt + clippy**

Run:
```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
```
Expected: clean. Commit fmt changes if any:

```bash
git status --short
git diff --stat
# If non-empty:
git add -u
git commit -m "style(#71): cargo fmt"
```

- [ ] **Step 2: Full test sweep**

Run: `cargo test --workspace`
Expected: 1153+ pass, 0 fail. The exact count grows by the new tests in `cx001.rs` (1) + the new strangler integration test (1) + the new registry test (1) — expect roughly 1156.

- [ ] **Step 3: Compatibility golden snapshot**

Run: `cargo test -p locus-cli --test governance_compat`
Expected: PASS — byte-identical output on `sample-crate`.

- [ ] **Step 4: Manual diff against pre-P2 baseline**

```bash
mkdir -p /tmp/locus-p2-cx001-after
cargo run -p locus-cli --quiet -- check --workspace tests/fixtures/sample-crate \
    > /tmp/locus-p2-cx001-after/sample-crate.txt 2>&1
diff /tmp/locus-p2-cx001-baseline/sample-crate.txt /tmp/locus-p2-cx001-after/sample-crate.txt
echo "diff exit: $?"
```
Expected: `diff exit: 0` (zero output, perfectly identical).

Repeat for `sample-crate-strict.txt`. Both must be identical.

- [ ] **Step 5: Locus self-workspace check**

```bash
cargo run -p locus-cli --quiet -- check --workspace . --agent-strict > /tmp/locus-p2-cx001-after/self-strict.txt 2>&1
diff /tmp/locus-p2-cx001-baseline/self-strict.txt /tmp/locus-p2-cx001-after/self-strict.txt | head -30
```

Expected: small diff or none. The only legitimate diffs would be:
- Updated line counts if functions moved (e.g. `Cx001Rule::observe` is a new function and might trigger CX001 on itself depending on length).
- Updated severities if any function crosses the budget boundary due to refactoring.

If anything new fires, investigate. New findings on new code may need lockfile entries (same pattern as P1's Task 17).

- [ ] **Step 6: Commit final cleanup if needed**

If lockfile entries are required:

```bash
git add locus.lock
git commit -m "chore(#71): lockfile entries for Cx001Rule migration findings"
```

---

## Task 11: Open PR

**Files:** none new

- [ ] **Step 1: Verify clean state**

```bash
git status
git log --oneline main..HEAD
```

Expected: every commit references epic #71. Working tree clean.

- [ ] **Step 2: Push and open PR**

```bash
git push -u origin <branch-name>
gh pr create --title "feat(#71): governance spine P2 — migrate CX001 to RuleDefinition" --body "$(cat <<'EOF'
## Summary

P2 of epic #71, first per-rule migration. Moves CX001 (function-line-budget rule) from the legacy `Paradigm::check()` path to a registered `RuleDefinition` implementation emitting typed `Evidence::ComplexityBudget` findings.

- New: `Cx001Rule` in `crates/locus-core/src/paradigms/complexity_budget/rules/cx001.rs`.
- New: per-rule directory `rules/` (promoted from `rules.rs`); CX002/007/008 still live in `rules/mod.rs` until they migrate.
- Registered: `Cx001Rule` in `RuleRegistry::standard()`; `CxParadigmDef::rules()` now returns `&[&CX001_RULE]`.
- Removed: legacy `pub fn cx001`, `cx001_check_file`, `cx001_diagnostic`, `cx001_why` from `rules/mod.rs`; the `cx001` call from `ComplexityBudget::check`.
- Migrated: 11 CX001 tests in `rules_tests.rs` now drive `Cx001Rule::observe` instead of legacy `cx001(...)`.

Spec: `docs/superpowers/specs/2026-05-11-governance-spine-design.md` §"Migration scope — rules".
Plan: `docs/superpowers/plans/2026-05-11-governance-spine-p2-cx001.md`.

## Strangler invariant

A new integration test (`crates/locus-core/tests/governance_cx001_strangler.rs`) asserts CX001 findings emerge from `FindingSource::RegisteredRule`, not from `FindingSource::LegacyDiagnostic`. The per-diagnostic-code filter in `LegacyParadigmRuleAdapter` is now exercised on real data for the first time.

## Validation

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace` (1156 pass; 3 new tests over post-P1 baseline)
- [x] Sample-crate compatibility snapshot byte-identical (golden test in `crates/locus-cli/tests/governance_compat.rs`)
- [x] Locus self-dogfood: 0 errors / 103 warnings under `--agent-strict`, matching post-P1 baseline (or lockfile updated if new findings appeared on the new code)

## Out of scope (deferred to subsequent P2 PRs)

- **OT002 migration:** Inference-shaped rule; exercises `Evidence::InferenceConfidence` + the `Confidence` enum. Detailed plan written after this PR lands.
- **DG001 (or FL003 fallback) migration:** Deterministic lockfile-config-driven rule; exercises `Evidence::Structured`. Detailed plan written after OT002 lands.

## Test plan

- [ ] Reviewer runs `cargo test -p locus-core --test governance_cx001_strangler` and confirms PASS.
- [ ] Reviewer runs `cargo test -p locus-cli --test governance_compat` and confirms PASS (byte-identical sample-crate output).
- [ ] Spot-check `crates/locus-core/src/paradigms/complexity_budget/rules/cx001.rs` — typed Evidence variant carries `lines`/`budget`/`override_match`.
- [ ] Spot-check `rules_tests.rs` — at least one migrated test asserts on `Evidence::ComplexityBudget` fields (not just message).

EOF
)"
```

- [ ] **Step 3: Return the PR URL**

---

## Self-review checklist (performed)

- [x] **Spec coverage:** P2 outline from the spec ("migrate CX001 to RuleDefinition; preserve byte-identical output via pass-through") is fully covered by Tasks 3–9. The compat snapshot check (Task 10) verifies the output stability contract.
- [x] **Placeholder scan:** No "TBD" / "implement later". Every code step shows actual code. The one ambiguity (test helper depending on `Lockfile::set_paradigm_section`) is flagged with a fallback instruction.
- [x] **Type consistency:** `Cx001Rule.observe` produces `RuleFinding` fields matching what `RuleRegistry`/`materialize` expect. `Evidence::ComplexityBudget { lines, budget, override_match }` matches the type defined in `governance/evidence.rs`. `default_severity` is `Severity` (not a custom thing).
- [x] **Strangler invariant:** Task 6's integration test explicitly verifies the per-diagnostic-code filter works. Task 7 (legacy call removal) AND Task 9 (legacy function deletion) both maintain the invariant via the compat snapshot check.
- [x] **Test migration:** Task 8 walks through the 11 existing tests with an explicit field-mapping table.
- [x] **Dogfood:** Task 10 covers the Locus self-workspace check. The plan calls out the possibility of new findings on the new code and how to handle them (lockfile entries, same as P1 Task 17).

End of P2-CX001 plan.
