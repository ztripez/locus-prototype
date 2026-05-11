# Governance Spine P2 — DG001 Migration Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate DG001 (forbidden import) from the legacy `Paradigm::check()` path to a registered `RuleDefinition` impl emitting typed `Evidence::Structured(json)` findings. After this PR, `locus check` output remains byte-identical, but DG001's path is `RuleDefinition::observe` → `RuleFinding` → `DefaultPassThroughPolicy` → `Diagnostic`.

**Architecture:** Third per-rule migration. DG001 is **deterministic and lockfile-config-driven** (unlike CX001/OT002 which were structural and inference-shaped). It iterates AIR imports, walks `section.forbidden_edges`, and emits one Fatal diagnostic per match. The new rule preserves the Fatal default, structures the evidence as `Evidence::Structured(json)` with the edge pattern + matched paths, and keeps byte-identical message/why/suggested_fix.

**Tech Stack:** Rust 2024 edition, existing governance spine + CX001 + OT002 migrations. No new dependencies.

**Spec:** [docs/superpowers/specs/2026-05-11-governance-spine-design.md](../specs/2026-05-11-governance-spine-design.md) §"Migration scope — rules". DG001 is the deterministic-lockfile-config variant exercising `Evidence::Structured`.

**Prior PRs:** P1 spine (#79), P2-CX001 (#80), P2-OT002 (#81).

**Reference plans:**
- [P2-CX001](2026-05-11-governance-spine-p2-cx001.md) — covers the `rules.rs → rules/mod.rs` promotion + transitional `// locus: allow MO005` annotations on remaining flat helpers. **DG has the same flat layout as pre-migration CX**, so the same promotion + MO005 fixup applies.
- [P2-OT002](2026-05-11-governance-spine-p2-ot002.md) — covers the `Paradigm::check`-to-`governance::run` migration for integration tests (`ot_basic.rs`). **DG has analogous integration tests** in `dg_basic.rs` that need the same treatment.

**Fallback:** If DG001 migration turns out heavier than expected due to deep coupling with DG002/003/004 helpers, **swap to FL003** (silent `.ok()/.err()` discard) per the spec. FL003 is the simpler fallback — single rule, no lockfile config, AIR-pattern matching only.

---

## File structure (P2-DG001)

**Rename (Task 2):**
- `crates/locus-core/src/paradigms/dependency_graph/rules.rs` → `crates/locus-core/src/paradigms/dependency_graph/rules/mod.rs`

**Create (Task 3):**
- `crates/locus-core/src/paradigms/dependency_graph/rules/dg001.rs` — `Dg001Rule` struct + `RuleDefinition` impl + per-rule tests.

**Create (Task 5):**
- `crates/locus-core/tests/governance_dg001_strangler.rs` — strangler-invariant integration test.

**Modify:**
- `crates/locus-core/src/paradigms/dependency_graph/rules/mod.rs` (post-promote) — add `pub mod dg001;`, fix `rules_tests.rs` path attribute, add `// locus: allow MO005` annotations on dg002/003/004 helper functions.
- `crates/locus-core/src/paradigms/dependency_graph/mod.rs` — remove the `rules::dg001(...)` call from `DependencyGraph::check`.
- `crates/locus-core/src/paradigms/dependency_graph/rules_tests.rs` — migrate 5 DG001 tests to drive `Dg001Rule`.
- `crates/locus-core/src/governance/registry.rs` — register `Dg001Rule` in `RuleRegistry::standard()`; add DG001 tests.
- `crates/locus-core/src/governance/paradigm_impls.rs` — break `DgParadigmDef` out of the macro and populate `rules()` with `&DG001_RULE`.
- `crates/locus-core/tests/dg_basic.rs` — migrate `for paradigm.check(...)` calls to `governance::run` (same pattern as `ot_basic.rs` in P2-OT002).

---

## Acceptance criteria (P2-DG001)

- `cargo build --workspace` succeeds.
- `cargo test --workspace` passes (5 DG001 tests migrated + new strangler/registry tests).
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `cargo fmt --all -- --check` clean.
- `cargo test -p locus-cli --test governance_compat` — compatibility snapshot byte-identical.
- `cargo test -p locus-core --test governance_dg001_strangler` — new integration test passes (DG001 findings come from `FindingSource::RegisteredRule`).
- `cargo run -p locus-cli -- check --workspace . --agent-strict` exits 0 with `0 errors / 104 warnings` baseline (matches post-P2-OT002).
- `RuleRegistry::standard().validate().is_ok()` still passes (debug_assert from P2-CX001 enforces uniqueness/prefix invariants).
- `RuleRegistry::standard().contains_code("DG001")` returns `true`.

---

## Task 1: Worktree setup + baseline capture

**Files:** none (environment only)

- [ ] **Step 1: Create the isolated worktree**

Controller creates worktree via `EnterWorktree` named `governance-spine-p2-dg001`. Working directory becomes `/mnt/code/projects/locus/.claude/worktrees/governance-spine-p2-dg001` on branch `worktree-governance-spine-p2-dg001`.

- [ ] **Step 2: Verify baseline tests pass**

Run: `cargo test --workspace 2>&1 | grep "test result:" | awk '{passed += $4; failed += $6} END {print "passed:", passed, "failed:", failed}'`
Expected: `passed: 1158+ failed: 0` (post-P2-OT002 baseline).

- [ ] **Step 3: Capture pre-migration baseline outputs**

```bash
mkdir -p /tmp/locus-p2-dg001-baseline
cargo run -p locus-cli --quiet -- check --workspace tests/fixtures/sample-crate \
    > /tmp/locus-p2-dg001-baseline/sample-crate.txt 2>&1
cargo run -p locus-cli --quiet -- check --workspace tests/fixtures/sample-crate --agent-strict \
    > /tmp/locus-p2-dg001-baseline/sample-crate-strict.txt 2>&1
cargo run -p locus-cli --quiet -- check --workspace . --agent-strict \
    > /tmp/locus-p2-dg001-baseline/self-strict.txt 2>&1
```

Expected: self-strict summary `0 error(s), 104 warning(s), 0 advisory.`

---

## Task 2: Promote `rules.rs` to `rules/` directory

(Same shape as P2-CX001 Task 2. See that plan for the rationale.)

**Files:**
- Rename: `crates/locus-core/src/paradigms/dependency_graph/rules.rs` → `crates/locus-core/src/paradigms/dependency_graph/rules/mod.rs`

- [ ] **Step 1: Move the file**

```bash
cd crates/locus-core/src/paradigms/dependency_graph
mkdir rules
git mv rules.rs rules/mod.rs
cd -
```

- [ ] **Step 2: Fix the `rules_tests` path attribute**

The current `rules.rs` ends with `#[cfg(test)] #[path = "rules_tests.rs"] mod rules_tests;`. After the rename to `rules/mod.rs`, the path becomes relative to the `rules/` directory and won't resolve. Fix it:

Find in `crates/locus-core/src/paradigms/dependency_graph/rules/mod.rs`:

```rust
#[cfg(test)]
#[path = "rules_tests.rs"]
mod rules_tests;
```

Change to:

```rust
#[cfg(test)]
#[path = "../rules_tests.rs"]
mod rules_tests;
```

- [ ] **Step 3: Verify build + tests**

Run: `cargo build -p locus-core 2>&1 | tail -3`
Expected: clean build.

Run: `cargo test -p locus-core dependency_graph 2>&1 | grep "test result:"`
Expected: same DG test pass count as before the move.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "refactor(#71): promote dependency_graph rules.rs to rules/ directory"
```

---

## Task 3: Add `Dg001Rule` stub + failing test

**Files:**
- Create: `crates/locus-core/src/paradigms/dependency_graph/rules/dg001.rs`
- Modify: `crates/locus-core/src/paradigms/dependency_graph/rules/mod.rs` — add `pub mod dg001;`

- [ ] **Step 1: Write the stub file**

Create `crates/locus-core/src/paradigms/dependency_graph/rules/dg001.rs`:

```rust
//! DG001 — forbidden import.
//!
//! Migrated to `RuleDefinition` in P2 (epic #71). Replaces the legacy
//! `super::dg001()` function. Walks `AirImport` items in every file,
//! compares each against `section.forbidden_edges`, and emits a
//! `RuleFinding` with `Evidence::Structured(json)` for each match. Always
//! Fatal: a forbidden edge is, by the user's own declaration, a
//! directional violation.

// locus: ot canonical

use crate::diagnostics::{CheckMode, Severity};
use crate::governance::evidence::Evidence;
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

pub struct Dg001Rule;

pub static DG001_RULE: Dg001Rule = Dg001Rule;

const DG001_ID: RuleId = RuleId::new("DG001");
const DG_PARADIGM: ParadigmId = ParadigmId::new("DG");

impl RuleDefinition for Dg001Rule {
    fn id(&self) -> RuleId {
        DG001_ID
    }
    fn paradigm(&self) -> ParadigmId {
        DG_PARADIGM
    }
    fn title(&self) -> &'static str {
        "forbidden import"
    }
    fn default_severity(&self) -> Severity {
        Severity::Fatal
    }
    fn observe(&self, _ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        // Implemented in Task 4.
        Vec::new()
    }
}

#[cfg(test)]
mod dg001_rule_tests {
    use super::*;
    use crate::governance::ids::FindingIdMinter;
    use crate::governance::registry::{ParadigmRegistry, RuleRegistry};
    use crate::lockfile::Lockfile;
    use locus_air::{
        AirFile, AirImport, AirItem, AirPackage, AirSpan, AirWorkspace,
    };

    /// Build a workspace with one file importing a forbidden path. The
    /// migrated rule should emit exactly one DG001 finding.
    #[test]
    fn fires_on_import_matching_forbidden_edge() {
        let air = workspace_with_forbidden_import();
        let lf = lockfile_with_forbidden_edge();
        let findings = run_observe(&air, &lf, CheckMode::Human);

        assert_eq!(findings.len(), 1, "expected one DG001 finding, got {findings:?}");
        assert_finding_shape(&findings[0]);
        assert_structured_evidence(&findings[0]);
    }

    fn run_observe(air: &AirWorkspace, lf: &Lockfile, mode: CheckMode) -> Vec<RuleFinding> {
        let rules = RuleRegistry::standard();
        let paradigms = ParadigmRegistry::empty();
        let minter = FindingIdMinter::new();
        let ctx = RuleContext {
            air,
            lockfile: lf,
            mode,
            rule_registry: &rules,
            paradigm_registry: &paradigms,
            finding_ids: &minter,
        };
        Dg001Rule.observe(&ctx)
    }

    fn assert_finding_shape(f: &RuleFinding) {
        assert_eq!(f.source, FindingSource::RegisteredRule(DG001_ID));
        assert_eq!(f.rule_id, Some(DG001_ID));
        assert_eq!(f.paradigm_id, Some(DG_PARADIGM));
        // DG001 default is Fatal (forbidden edge is the user's own declaration).
        assert_eq!(f.default_severity, Severity::Fatal);
        assert!(
            f.message.contains("forbidden import"),
            "expected legacy-compatible message, got `{}`",
            f.message
        );
    }

    fn assert_structured_evidence(f: &RuleFinding) {
        assert_eq!(f.evidence.len(), 1);
        match &f.evidence[0] {
            Evidence::Structured(json) => {
                assert_eq!(json["from_pattern"], "pkg::feature_a::*");
                assert_eq!(json["to_pattern"], "pkg::feature_b::*");
                assert_eq!(json["importer_module"], "pkg::feature_a::handler");
                assert_eq!(json["import_path"], "pkg::feature_b::internal");
            }
            other => panic!("expected Structured evidence, got {other:?}"),
        }
    }

    fn workspace_with_forbidden_import() -> AirWorkspace {
        AirWorkspace::new(vec![AirPackage {
            name: "pkg".into(),
            version: "0.0.1".into(),
            root_dir: "/tmp/pkg".into(),
            files: vec![AirFile {
                path: "src/feature_a/handler.rs".into(),
                module_path: Some("pkg::feature_a::handler".into()),
                items: vec![AirItem::Import(AirImport {
                    path: "pkg::feature_b::internal".into(),
                    span: AirSpan::new("src/feature_a/handler.rs", 1, 1),
                })],
                hints: Vec::new(),
                parse_error: None,
                line_count: 5,
            }],
        }])
    }

    fn lockfile_with_forbidden_edge() -> Lockfile {
        let mut lf = Lockfile::default();
        let section = serde_json::json!({
            "forbidden_edges": [
                {
                    "from": "pkg::feature_a::*",
                    "to": "pkg::feature_b::*",
                    "reason": "feature isolation: A and B don't talk directly"
                }
            ],
            "features": [],
            "shared_paths": []
        });
        lf.paradigms.insert("DG".to_string(), section);
        lf
    }
}
```

**Schema verification:** before pasting, confirm `AirImport`'s field set in `crates/locus-air/src/lib.rs`. The current shape is `{ path: String, span: AirSpan }` per the legacy `dg001` code. If it has additional fields (e.g. `kind` or `is_star`), include them. Same for `AirPackage`, `AirFile`, `AirWorkspace` — but those shapes are stable (used identically in CX001 + OT002 migrations).

**`DgSection.forbidden_edges` schema** is in `crates/locus-core/src/paradigms/dependency_graph/lockfile_schema.rs`:

```rust
pub struct ForbiddenEdge {
    pub from: String,
    pub to: String,
    pub reason: Option<String>,
}
```

The JSON literal in `lockfile_with_forbidden_edge` must serialize to that shape. If `DgSection` has additional required fields beyond `forbidden_edges`/`features`/`shared_paths`, add them to the JSON (with defaults).

- [ ] **Step 2: Add `pub mod dg001;` to `rules/mod.rs`**

Edit `crates/locus-core/src/paradigms/dependency_graph/rules/mod.rs`. After the existing `use` block at the top of the file (around line 18, after `use crate::diagnostics::...`), add:

```rust
pub mod dg001;
```

- [ ] **Step 3: Run the failing test**

Run: `cargo test -p locus-core --lib paradigms::dependency_graph::rules::dg001::dg001_rule_tests -- --nocapture`

Expected: 1 test FAILS with `expected one DG001 finding, got []`. Confirms the harness compiles and runs against the stub.

If it fails to compile, inspect the error and adjust the AIR fixture or lockfile JSON. Most likely drift: `AirImport` field set or `DgSection` required fields.

- [ ] **Step 4: Commit**

```bash
git add crates/locus-core/src/paradigms/dependency_graph/rules/
git commit -m "test(#71): failing test for Dg001Rule migration"
```

---

## Task 4: Implement `Dg001Rule::observe`

**Files:**
- Modify: `crates/locus-core/src/paradigms/dependency_graph/rules/dg001.rs`

- [ ] **Step 1: Port the legacy logic**

Find the stub:

```rust
    fn observe(&self, _ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        // Implemented in Task 4.
        Vec::new()
    }
}
```

Replace with the real body + helpers AFTER the closing `}` of the impl:

```rust
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::super::lockfile_schema::DgSection;
        let section: DgSection = ctx
            .lockfile
            .paradigm_section("DG")
            .unwrap_or_default();
        if section.forbidden_edges.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for pkg in &ctx.air.packages {
            for file in &pkg.files {
                let Some(module_path) = file.module_path.as_deref() else {
                    continue;
                };
                check_file(file, module_path, &section, ctx, &mut out);
            }
        }
        out
    }
}

fn check_file(
    file: &locus_air::AirFile,
    module_path: &str,
    section: &super::super::lockfile_schema::DgSection,
    ctx: &RuleContext<'_>,
    out: &mut Vec<RuleFinding>,
) {
    use locus_air::AirItem;
    use super::super::lockfile_schema::matches_pattern;
    for item in &file.items {
        let AirItem::Import(imp) = item else {
            continue;
        };
        for edge in &section.forbidden_edges {
            if !matches_pattern(&edge.from, module_path) {
                continue;
            }
            if !matches_pattern(&edge.to, &imp.path) {
                continue;
            }
            out.push(make_finding(module_path, imp, edge, ctx));
            break; // one diagnostic per (file, import) — match legacy semantics
        }
    }
}

fn make_finding(
    module_path: &str,
    imp: &locus_air::AirImport,
    edge: &super::super::lockfile_schema::ForbiddenEdge,
    ctx: &RuleContext<'_>,
) -> RuleFinding {
    let mut why = vec![
        format!("importer `{module_path}` matches `from = {}`", edge.from),
        format!("import `{}` matches `to = {}`", imp.path, edge.to),
    ];
    if let Some(reason) = &edge.reason {
        why.push(format!("reason: {reason}"));
    }
    let severity = ctx.mode.elevate(Severity::Fatal);
    let mut json = serde_json::json!({
        "from_pattern": &edge.from,
        "to_pattern": &edge.to,
        "importer_module": module_path,
        "import_path": &imp.path,
    });
    if let Some(reason) = &edge.reason {
        json["reason"] = serde_json::Value::String(reason.clone());
    }
    RuleFinding {
        id: ctx.finding_ids.next(),
        source: FindingSource::RegisteredRule(DG001_ID),
        rule_id: Some(DG001_ID),
        paradigm_id: Some(DG_PARADIGM),
        default_severity: severity,
        span: Some(imp.span.clone()),
        concept: None,
        message: format!(
            "forbidden import: `{module_path}` must not reach `{}`",
            imp.path
        ),
        evidence: vec![Evidence::Structured(json)],
        why,
        suggested_fix: Some(
            "remove the import, or route the call through an accepted \
             intermediary (port, application service, or shared crate); \
             if the edge is wrong, edit `paradigms.DG.forbidden_edges` in \
             `locus.lock`"
                .into(),
        ),
        diagnostic_code: None,
    }
}
```

**Severity note:** DG001 is always Fatal by default (legacy used `mode.elevate(Severity::Fatal)`). Under `--agent-strict`, `elevate(Fatal)` returns `Fatal` unchanged — there's no elevation past Fatal. The new code preserves this.

- [ ] **Step 2: Run the test — expect PASS**

Run: `cargo test -p locus-core --lib paradigms::dependency_graph::rules::dg001::dg001_rule_tests -- --nocapture`
Expected: PASS.

- [ ] **Step 3: Clippy + commit**

```bash
cargo clippy -p locus-core --all-targets -- -D warnings 2>&1 | tail -3
git add crates/locus-core/src/paradigms/dependency_graph/rules/dg001.rs
git commit -m "feat(#71): Dg001Rule observes Evidence::Structured"
```

---

## Task 5: Strangler-invariant integration test

**Files:**
- Create: `crates/locus-core/tests/governance_dg001_strangler.rs`

- [ ] **Step 1: Write the integration test**

Create `crates/locus-core/tests/governance_dg001_strangler.rs`:

```rust
//! Verifies the strangler invariant for DG001: every DG001 finding from
//! the governance pipeline comes from `FindingSource::RegisteredRule`,
//! NOT from `FindingSource::LegacyDiagnostic`. The per-diagnostic-code
//! filter in `LegacyParadigmRuleAdapter` is now exercised on a third
//! rule code (CX001 + OT002 + DG001).

use locus_air::{AirFile, AirImport, AirItem, AirPackage, AirSpan, AirWorkspace};
use locus_core::CheckMode;
use locus_core::governance::{self, FindingSource, RuleId};
use locus_core::lockfile::Lockfile;

#[test]
fn dg001_findings_come_from_registered_rule_not_legacy_adapter() {
    let air = AirWorkspace::new(vec![AirPackage {
        name: "pkg".into(),
        version: "0.0.1".into(),
        root_dir: "/tmp/pkg".into(),
        files: vec![AirFile {
            path: "src/feature_a/handler.rs".into(),
            module_path: Some("pkg::feature_a::handler".into()),
            items: vec![AirItem::Import(AirImport {
                path: "pkg::feature_b::internal".into(),
                span: AirSpan::new("src/feature_a/handler.rs", 1, 1),
            })],
            hints: Vec::new(),
            parse_error: None,
            line_count: 5,
        }],
    }]);
    let mut lf = Lockfile::default();
    let section = serde_json::json!({
        "forbidden_edges": [
            {
                "from": "pkg::feature_a::*",
                "to": "pkg::feature_b::*",
                "reason": "feature isolation"
            }
        ],
        "features": [],
        "shared_paths": []
    });
    lf.paradigms.insert("DG".to_string(), section);

    let out = governance::run(&air, &lf, CheckMode::Human);

    let dg001_findings: Vec<_> = out
        .findings
        .iter()
        .filter(|f| {
            matches!(&f.rule_id, Some(r) if *r == RuleId::new("DG001"))
                || matches!(
                    &f.source,
                    FindingSource::LegacyDiagnostic { rule_code, .. } if rule_code == "DG001"
                )
        })
        .collect();

    assert_eq!(
        dg001_findings.len(),
        1,
        "expected exactly one DG001 finding (no double-fire), got {} findings: {:?}",
        dg001_findings.len(),
        dg001_findings
    );

    match &dg001_findings[0].source {
        FindingSource::RegisteredRule(r) => {
            assert_eq!(r.as_str(), "DG001");
        }
        FindingSource::LegacyDiagnostic { rule_code, .. } => {
            panic!(
                "DG001 finding still flows through legacy adapter (rule_code={rule_code}); \
                 strangler filter is not working"
            );
        }
        other => panic!("unexpected source for DG001 finding: {other:?}"),
    }
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p locus-core --test governance_dg001_strangler 2>&1 | tail -5`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/locus-core/tests/governance_dg001_strangler.rs
git commit -m "test(#71): assert DG001 findings come from RegisteredRule, not legacy adapter"
```

---

## Task 6: Register `Dg001Rule` in `RuleRegistry` and `DgParadigmDef`

**Files:**
- Modify: `crates/locus-core/src/governance/registry.rs`
- Modify: `crates/locus-core/src/governance/paradigm_impls.rs`

- [ ] **Step 1: Extend `RuleRegistry::standard()`**

In `crates/locus-core/src/governance/registry.rs`, find the `pub fn standard()` body that currently wires CX001 + OT002:

```rust
            rules: vec![
                &crate::paradigms::complexity_budget::rules::cx001::CX001_RULE,
                &crate::paradigms::one_truth::rules::ot002::OT002_RULE,
            ],
```

Insert DG001 (alphabetical after CX, before OT — actual order doesn't matter functionally, but keep it sorted-by-prefix for readability):

```rust
            rules: vec![
                &crate::paradigms::complexity_budget::rules::cx001::CX001_RULE,
                &crate::paradigms::dependency_graph::rules::dg001::DG001_RULE,
                &crate::paradigms::one_truth::rules::ot002::OT002_RULE,
            ],
```

- [ ] **Step 2: Break `DgParadigmDef` out of the macro**

In `crates/locus-core/src/governance/paradigm_impls.rs`, find:

```rust
paradigm_def!(DgParadigmDef, "DG", "Dependency Graph");
```

Replace with explicit impl (matches CX/OT pattern):

```rust
// DG breaks out of the macro — third paradigm with a migrated rule
// (DG001 in P2 #71).
pub struct DgParadigmDef;
impl ParadigmDefinition for DgParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("DG")
    }
    fn title(&self) -> &'static str {
        "Dependency Graph"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        static RULES: [&dyn RuleDefinition; 1] =
            [&crate::paradigms::dependency_graph::rules::dg001::DG001_RULE];
        &RULES
    }
}
```

- [ ] **Step 3: Add registry tests**

In `crates/locus-core/src/governance/registry.rs`, find `rule_registry_contains_ot002_after_p2_migration` and `ot_paradigm_def_lists_ot002_rule`. After those, append:

```rust
    #[test]
    fn rule_registry_contains_dg001_after_p2_migration() {
        let reg = RuleRegistry::standard();
        assert!(
            reg.contains_code("DG001"),
            "DG001 must be in RuleRegistry::standard() after P2-DG001"
        );
        let rule = reg.find(&RuleId::new("DG001")).expect("DG001 missing");
        assert_eq!(rule.paradigm().as_str(), "DG");
        // DG001 is always Fatal — forbidden edge is the user's own
        // declaration, not an inferred budget.
        assert_eq!(
            rule.default_severity(),
            crate::diagnostics::Severity::Fatal
        );
    }

    #[test]
    fn dg_paradigm_def_lists_dg001_rule() {
        let reg = ParadigmRegistry::standard();
        let dg = reg
            .find(&ParadigmId::new("DG"))
            .expect("DG ParadigmDefinition missing");
        let rule_ids: Vec<&str> = dg.rules().iter().map(|r| r.id().as_str()).collect();
        assert_eq!(rule_ids, vec!["DG001"]);
    }
```

- [ ] **Step 4: Run tests + clippy**

Run: `cargo test -p locus-core --lib governance::registry 2>&1 | grep "test result:"`
Expected: 14 tests pass (12 post-OT002 + 2 new).

Run: `cargo clippy -p locus-core --all-targets -- -D warnings 2>&1 | tail -3`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/locus-core/src/governance/registry.rs crates/locus-core/src/governance/paradigm_impls.rs
git commit -m "feat(#71): register Dg001Rule in RuleRegistry::standard() and DG paradigm"
```

---

## Task 7: Remove legacy `rules::dg001` call from `DependencyGraph::check`

**Files:**
- Modify: `crates/locus-core/src/paradigms/dependency_graph/mod.rs`

- [ ] **Step 1: Drop the dg001 invocation**

In `crates/locus-core/src/paradigms/dependency_graph/mod.rs`, find the `DependencyGraph::check` body. The current shape is approximately:

```rust
        let mut diags = rules::dg001(air, &section, mode);
        diags.extend(rules::dg002(air, &section, mode));
        diags.extend(rules::dg003(air, &section, mode));
        diags.extend(rules::dg004(air, &section, mode));
```

Remove the dg001 line:

```rust
        // DG001 migrated to RuleDefinition (#71 P2). Runs via the
        // governance pipeline; the legacy adapter's per-rule-code filter
        // drops any DG001 diagnostic emitted here.
        let mut diags = rules::dg002(air, &section, mode);
        diags.extend(rules::dg003(air, &section, mode));
        diags.extend(rules::dg004(air, &section, mode));
```

- [ ] **Step 2: Compat snapshot still byte-identical**

Run: `cargo test -p locus-cli --test governance_compat 2>&1 | tail -3`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/locus-core/src/paradigms/dependency_graph/mod.rs
git commit -m "feat(#71): remove legacy rules::dg001 call from DependencyGraph::check"
```

---

## Task 8: Migrate DG001 unit tests + dg_basic integration tests

**Files:**
- Modify: `crates/locus-core/src/paradigms/dependency_graph/rules_tests.rs`
- Modify: `crates/locus-core/tests/dg_basic.rs`

- [ ] **Step 1: Add a test helper at the top of `rules_tests.rs`**

Add an `observe_dg001` helper, parallel to the `observe_cx001` / `observe_ot002` helpers from prior migrations. After the existing top-of-file imports, add:

```rust
// DG001 migrated to RuleDefinition (#71 P2). Tests call this helper
// instead of the legacy `dg001(...)` function; helper constructs the
// RuleContext + lockfile shape that `Dg001Rule::observe` expects.
use crate::governance::evidence::Evidence;
use crate::governance::finding::RuleFinding;
use crate::governance::ids::{FindingIdMinter, RuleId};
use crate::governance::registry::{ParadigmRegistry, RuleRegistry};
use crate::governance::rule::{RuleContext, RuleDefinition};
use crate::lockfile::Lockfile;
use crate::paradigms::dependency_graph::rules::dg001::Dg001Rule;

fn observe_dg001(
    air: &locus_air::AirWorkspace,
    section: &DgSection,
    mode: crate::diagnostics::CheckMode,
) -> Vec<RuleFinding> {
    let mut lf = Lockfile::default();
    lf.paradigms.insert(
        "DG".to_string(),
        serde_json::to_value(section).expect("DgSection must serialize"),
    );
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
    Dg001Rule.observe(&ctx)
}
```

- [ ] **Step 2: Migrate the 5 DG001 tests**

For each of the 5 `#[test] fn dg001_*` tests in `rules_tests.rs`:

| Old | New |
|---|---|
| `let diags = dg001(&air, &section, CheckMode::Human);` | `let findings = observe_dg001(&air, &section, CheckMode::Human);` |
| `diags.is_empty()` | `findings.is_empty()` |
| `diags.len()` | `findings.len()` |
| `diags[0].rule_id == "DG001"` | `findings[0].rule_id == Some(RuleId::new("DG001"))` |
| `diags[0].severity == Severity::Fatal` | `findings[0].default_severity == Severity::Fatal` |
| `diags[0].message` | `findings[0].message` |
| `diags[0].why` | `findings[0].why` |
| `diags[0].suggested_fix` | `findings[0].suggested_fix` |
| `diags[0].span` | `findings[0].span.as_ref().unwrap()` |

For tests that previously asserted a fired diagnostic, append an `Evidence::Structured` assertion checking the JSON payload carries `from_pattern`, `to_pattern`, `importer_module`, `import_path`. Example:

```rust
match &findings[0].evidence[0] {
    Evidence::Structured(json) => {
        assert_eq!(json["from_pattern"], "crate::a::*");
        assert_eq!(json["to_pattern"], "crate::b::*");
    }
    other => panic!("expected Structured evidence, got {other:?}"),
}
```

- [ ] **Step 3: Migrate dg_basic integration tests**

Same pattern as P2-OT002's `ot_basic.rs` migration. Find every `for paradigm in registry() { diags.extend(paradigm.check(&air, &lockfile, mode)); }` block in `crates/locus-core/tests/dg_basic.rs` and replace with:

```rust
let diags = governance::run(&air, &lockfile, mode).diagnostics;
```

Add `use locus_core::governance;` to the imports if not already present.

Same for any `for p in &registry { ... }` or `for p in registry() { ... }` variants.

- [ ] **Step 4: Run all DG tests**

Run: `cargo test -p locus-core dependency_graph 2>&1 | grep "test result:"`
Expected: same pass count as pre-migration.

Run: `cargo test -p locus-core --test dg_basic 2>&1 | grep "test result:"`
Expected: same pass count.

- [ ] **Step 5: Commit**

```bash
git add crates/locus-core/src/paradigms/dependency_graph/rules_tests.rs crates/locus-core/tests/dg_basic.rs
git commit -m "test(#71): migrate DG001 unit and integration tests to Dg001Rule"
```

---

## Task 9: Delete legacy `pub fn dg001` + helpers

**Files:**
- Modify: `crates/locus-core/src/paradigms/dependency_graph/rules/mod.rs`

- [ ] **Step 1: Verify no remaining callers**

Run: `grep -rn "rules::dg001\|use.*dg001::dg001\|dg001(&\b" crates/ 2>&1 | grep -v "rules::dg001::DG001_RULE\|rules::dg001::Dg001Rule\|use crate::paradigms::dependency_graph::rules::dg001::Dg001Rule\|tests/governance_dg001"`

Expected: zero hits (or only hits pointing at `Dg001Rule`/`DG001_RULE` references).

- [ ] **Step 2: Delete the legacy `pub fn dg001` and `dg001_diagnostic`**

In `crates/locus-core/src/paradigms/dependency_graph/rules/mod.rs`, delete:
- `fn dg001_diagnostic(...)` (lines ~20-51)
- `pub fn dg001(...)` (lines ~53-89, including the doc-comment block above)

Trim any unused imports that result (e.g. if `dg001_diagnostic` was the only user of some import).

- [ ] **Step 3: Build + test**

Run: `cargo build -p locus-core 2>&1 | tail -3`
Expected: clean.

Run: `cargo test -p locus-core 2>&1 | grep "test result:" | awk '{passed += $4; failed += $6} END {print "passed:", passed, "failed:", failed}'`
Expected: same pass count as Task 8.

- [ ] **Step 4: Commit**

```bash
git add crates/locus-core/src/paradigms/dependency_graph/rules/mod.rs
git commit -m "refactor(#71): remove legacy dg001 fn (replaced by Dg001Rule)"
```

---

## Task 10: Full sweep + diff check + dogfood cleanup

(Same shape as P2-CX001 Task 10 / P2-OT002 Task 9.)

- [ ] **Step 1: fmt + clippy**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
```

Commit any fmt changes:

```bash
git add -u
git commit -m "style(#71): cargo fmt"
```

- [ ] **Step 2: Full test sweep**

Run: `cargo test --workspace 2>&1 | grep "test result:" | awk '{passed += $4; failed += $6} END {print "passed:", passed, "failed:", failed}'`
Expected: matches post-OT002 baseline +3 (1 strangler integration + 2 registry tests).

- [ ] **Step 3: Compatibility snapshot**

Run: `cargo test -p locus-cli --test governance_compat`
Expected: PASS.

- [ ] **Step 4: Locus self-dogfood check**

```bash
mkdir -p /tmp/locus-p2-dg001-after
cargo run -p locus-cli --quiet -- check --workspace . --agent-strict \
    > /tmp/locus-p2-dg001-after/self-strict.txt 2>&1
grep "summary:" /tmp/locus-p2-dg001-after/self-strict.txt
```

Expected: `summary: 0 error(s), 104 warning(s), 0 advisory.` (matches post-P2-OT002 baseline).

Likely findings on new code:
- **MO005 ×~12** on `dg002`/`dg003`/`dg004` helpers in `rules/mod.rs` (same pattern as CX001's transitional MO005 surge — flat helpers in a `mod.rs` get flagged).
  - **Fix:** Add `// locus: allow MO005 — DG rule helper; transitional in rules/mod.rs until split to per-file like DG001 (#71 follow-up)` above each affected fn. P2-CX001 used this exact pattern; copy from there.
- **OT004** on test fixtures constructing `AirImport`. Possible — depends on whether `AirImport` is a registered canonical. Check by running the dogfood check; add `// locus: allow OT004` annotations as needed.
- **CX001** on new helpers if any exceed 50 lines. Refactor if so (lesson from P2-OT002 review: don't accept fixture-test drift; split into helpers).

- [ ] **Step 5: Sample-crate diff against pre-P2-DG001 baseline**

```bash
cargo run -p locus-cli --quiet -- check --workspace tests/fixtures/sample-crate > /tmp/locus-p2-dg001-after/sample-crate.txt 2>&1
diff /tmp/locus-p2-dg001-baseline/sample-crate.txt /tmp/locus-p2-dg001-after/sample-crate.txt
echo "diff exit: $?"
```

Expected: `diff exit: 0`.

- [ ] **Step 6: Commit cleanup**

```bash
git add -u
git commit -m "chore(#71): dogfood cleanup for DG001 migration"
```

---

## Task 11: Open PR

- [ ] **Step 1: Verify clean state + push**

```bash
git status
git log --oneline main..HEAD
git push -u origin worktree-governance-spine-p2-dg001
```

- [ ] **Step 2: Open the PR**

```bash
gh pr create --title "feat(#71): governance spine P2 — migrate DG001 to RuleDefinition" --body "$(cat <<'EOF'
## Summary

P2 of epic #71, third per-rule migration. Moves **DG001** (forbidden import) from the legacy `Paradigm::check()` path to a registered `RuleDefinition` implementation emitting typed `Evidence::Structured(json)` findings.

- **New:** `Dg001Rule` in `crates/locus-core/src/paradigms/dependency_graph/rules/dg001.rs`.
- **New:** per-rule directory `rules/` (promoted from `rules.rs`); DG002/003/004 still live in `rules/mod.rs` until they migrate (future P2 follow-up).
- **Registered:** `Dg001Rule` in `RuleRegistry::standard()`; `DgParadigmDef::rules()` now returns `&[&DG001_RULE]` (third paradigm with a non-empty `rules()` slice, after CX and OT).
- **Removed:** legacy `pub fn dg001` and `dg001_diagnostic`; the `dg001` call from `DependencyGraph::check`.
- **Migrated:** 5 DG001 unit tests in `rules_tests.rs` + DG001 integration tests in `dg_basic.rs`. Tests assert on `Evidence::Structured` JSON payload.

Spec: `docs/superpowers/specs/2026-05-11-governance-spine-design.md` §"Migration scope — rules".
Plan: `docs/superpowers/plans/2026-05-11-governance-spine-p2-dg001.md`.

## Why Evidence::Structured?

DG001 is deterministic (no inference confidence) and its evidence is a pattern-match record: `from_pattern`, `to_pattern`, `importer_module`, `import_path`, optional `reason`. No fixed enum variant fits; `Evidence::Structured(serde_json::Value)` is the catch-all for migrated rules whose schema isn't yet typed. A future PR can promote this to a typed variant once we see how DG002/003/004 use evidence.

## Severity: always Fatal

DG001 is always Fatal — a forbidden edge is the user's own declaration. `mode.elevate(Severity::Fatal)` returns `Fatal` unchanged under `--agent-strict`. Migration preserves this exactly.

## Strangler invariant verified

New integration test `crates/locus-core/tests/governance_dg001_strangler.rs` asserts DG001 findings come from `FindingSource::RegisteredRule`, not `LegacyDiagnostic`. The per-diagnostic-code filter is now exercised on **three** rule codes (CX001 + OT002 + DG001).

## Validation

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`
- [x] **Sample-crate compatibility snapshot byte-identical**
- [x] **Locus self-dogfood under `--agent-strict`: 0 errors / 104 warnings**, exactly matching post-P2-OT002 baseline
- [x] `RuleRegistry::standard().validate().is_ok()` keeps passing

## Transitional `// locus: allow` comments

Following the P2-CX001 / P2-OT002 pattern: transitional MO005 allow comments on DG002/003/004 helpers in `rules/mod.rs` (they're flagged because they live in an entrypoint module post-promotion). Come off when those rules migrate to per-file `RuleDefinition` impls.

## Out of scope (deferred)

- **DG002/003/004 migration:** Cleanup follow-up that splits remaining DG rules into per-file `RuleDefinition` impls and removes the transitional MO005 allows.
- **OT001 + OT003-OT012 migration:** Follow-up that migrates remaining OT rules.
- **CX002/CX007/CX008 migration:** Same for CX.

## Test plan

- [ ] Reviewer runs `cargo test -p locus-core --test governance_dg001_strangler` — confirms PASS.
- [ ] Reviewer runs `cargo test -p locus-cli --test governance_compat` — confirms PASS (byte-identical sample-crate).
- [ ] Reviewer runs `cargo run -p locus-cli -- check --workspace . --agent-strict` — confirms `0 error(s), 104 warning(s)`.
- [ ] Spot-check `Dg001Rule::observe` — typed `Evidence::Structured` carries the edge pattern + matched paths.
- [ ] Spot-check `default_severity` — Fatal (not Warning, unlike CX001/OT002).
EOF
)"
```

---

## Self-review checklist (performed)

- [x] **Spec coverage:** DG001 outlined as deterministic/lockfile-config-driven, `Evidence::Structured`. Plan covers all five elements: type + registration + strangler test + legacy removal + migration of tests.
- [x] **Placeholder scan:** No "TBD". The two AIR-schema-verification steps (Task 3) are explicit "confirm shape, adjust if drift" instructions, not TODOs.
- [x] **Type consistency:** `Dg001Rule.observe` produces `RuleFinding` with the same field set used by `Cx001Rule`/`Ot002Rule`. `Evidence::Structured` matches `crate::governance::evidence::Evidence::Structured(serde_json::Value)`. `DG001_RULE` static naming matches `CX001_RULE`/`OT002_RULE`.
- [x] **Severity Fatal:** explicitly called out. `default_severity` is `Severity::Fatal`. The test asserts this. The registry test asserts this.
- [x] **Strangler invariant test:** Task 5; same shape as CX001/OT002 strangler tests.
- [x] **Test migration:** Task 8 covers both unit tests (`rules_tests.rs`) AND integration tests (`dg_basic.rs`). Same patterns as prior migrations.
- [x] **Dogfood:** Task 10 includes the MO005 transitional-allow pattern and notes the OT004 + CX001 lessons from P2-OT002 (no fixture drift accepted).
- [x] **`rules.rs → rules/mod.rs` promotion:** Task 2; same as P2-CX001. Path attribute fixup for `rules_tests.rs` included.
- [x] **No fallback yet to FL003:** Plan defaults to DG001. The "If DG001 turns out heavier than expected" caveat in the header allows graceful pivot.

End of P2-DG001 plan.
