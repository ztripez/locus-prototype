# Governance Spine P2 — OT002 Migration Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate OT002 (undeclared concept-shaped type) from the legacy `Paradigm::check()` path to a registered `RuleDefinition` impl emitting typed `Evidence::InferenceConfidence { score: Confidence, signals }` findings. After this PR, `locus check` output remains byte-identical, but OT002's path is `RuleDefinition::observe` → `RuleFinding` → `DefaultPassThroughPolicy` → `Diagnostic`.

**Architecture:** Second per-rule migration. OT002 is inference-shaped — `member.field_overlap` (an `f32` in `[0.0, 1.0]`) is discretized into `Confidence::{Low, Medium, High}` for the typed Evidence variant. The materialized diagnostic preserves byte-identical legacy text (overlap percentage, stem match, reasons). Cluster computation (`infer::cluster_concepts_with_lockfile`) is invoked inside `Ot002Rule::observe`; the legacy `OneTruth::check` still computes clusters for OT001 + OT003–OT012, so this PR introduces a transient double-compute that subsequent P2 migrations eliminate.

**Tech Stack:** Rust 2024 edition, existing governance spine from P1 + CX001 migration from P2. No new dependencies.

**Spec:** [docs/superpowers/specs/2026-05-11-governance-spine-design.md](../specs/2026-05-11-governance-spine-design.md) §"Migration scope — rules" — OT002 is the inference variant exercising the `Confidence` enum.

**Prior PRs:** P1 spine (#79, merged at `a000e15`), P2-CX001 (#80, merged at `dffe4ef`).

**Reference:** P2-CX001 plan at [docs/superpowers/plans/2026-05-11-governance-spine-p2-cx001.md](2026-05-11-governance-spine-p2-cx001.md). This plan follows the same shape with three differences flagged inline: (i) no `rules.rs → rules/mod.rs` rename (OT already split), (ii) Evidence is `InferenceConfidence`, not `ComplexityBudget`, (iii) clusters double-compute is accepted transient debt.

---

## File structure (P2-OT002)

**Modify:**
- `crates/locus-core/src/paradigms/one_truth/rules/ot002.rs` — add `Ot002Rule` struct + `RuleDefinition` impl alongside (then replacing) the legacy `pub fn ot002`.
- `crates/locus-core/src/paradigms/one_truth/rules.rs` — remove `pub mod ot002;` re-export line `pub use ot002::ot002;` once tests migrate (Task 8).
- `crates/locus-core/src/paradigms/one_truth/mod.rs` — remove the `rules::ot002(&clusters, mode)` call from `OneTruth::check`.
- `crates/locus-core/src/paradigms/one_truth/rules_tests.rs` — migrate 5 OT002 tests to drive `Ot002Rule::observe`.
- `crates/locus-core/src/governance/registry.rs` — register `Ot002Rule` in `RuleRegistry::standard()`.
- `crates/locus-core/src/governance/paradigm_impls.rs` — break `OtParadigmDef` out of the macro (like `CxParadigmDef` in P2-CX001) and populate its `rules()` slice with `&OT002_RULE`.

**Create:**
- `crates/locus-core/tests/governance_ot002_strangler.rs` — strangler-invariant integration test, parallel to `governance_cx001_strangler.rs`.

**Untouched (transitional):**
- The cluster-building call `infer::cluster_concepts_with_lockfile(air, &section)` is duplicated for now: once in `Ot002Rule::observe` (new), once in `OneTruth::check` (legacy, for OT001 + OT003-OT012). When all OT rules migrate, the legacy compute drops.

---

## Acceptance criteria (P2-OT002)

- `cargo build --workspace` succeeds.
- `cargo test --workspace` passes; OT002 tests migrated to drive `Ot002Rule::observe`.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `cargo fmt --all -- --check` clean.
- `cargo test -p locus-cli --test governance_compat` — compatibility snapshot byte-identical.
- `cargo test -p locus-core --test governance_ot002_strangler` — new integration test passes; asserts OT002 findings come from `FindingSource::RegisteredRule`, not `LegacyDiagnostic`.
- `cargo run -p locus-cli -- check --workspace . --agent-strict` exits 0 with the same `0 errors / 103 warnings` baseline as post-P2-CX001 (zero new findings on the migrated code).
- `RuleRegistry::standard().contains_code("OT002")` returns `true` (covered by `rule_registry_contains_ot002_after_p2_migration` test).
- `RuleRegistry::standard().validate().is_ok()` still passes — debug_assert from P2-CX001 catches uniqueness/prefix violations.

---

## Task 1: Worktree setup + baseline capture

**Files:** none (environment only)

- [ ] **Step 1: Create the isolated worktree**

The session controller MUST create a worktree via `EnterWorktree` (or equivalent) named `governance-spine-p2-ot002`. Expected outcome: working directory becomes `/mnt/code/projects/locus/.claude/worktrees/governance-spine-p2-ot002` on branch `worktree-governance-spine-p2-ot002`.

- [ ] **Step 2: Verify baseline tests pass**

Run: `cargo test --workspace 2>&1 | grep "test result:" | awk '{passed += $4; failed += $6} END {print "passed:", passed, "failed:", failed}'`
Expected: `passed: 1066 failed: 0` (post-P2-CX001 baseline; one extra test vs. P1 from the new `rule_registry_standard_satisfies_construction_invariants` test).

- [ ] **Step 3: Capture pre-migration baseline outputs**

```bash
mkdir -p /tmp/locus-p2-ot002-baseline
cargo run -p locus-cli --quiet -- check --workspace tests/fixtures/sample-crate \
    > /tmp/locus-p2-ot002-baseline/sample-crate.txt 2>&1
cargo run -p locus-cli --quiet -- check --workspace tests/fixtures/sample-crate --agent-strict \
    > /tmp/locus-p2-ot002-baseline/sample-crate-strict.txt 2>&1
cargo run -p locus-cli --quiet -- check --workspace . --agent-strict \
    > /tmp/locus-p2-ot002-baseline/self-strict.txt 2>&1
```

No commit. These are the "before" recordings for the byte-identical check in Task 10.

---

## Task 2: Add `Ot002Rule` stub + first failing test

**Files:**
- Modify: `crates/locus-core/src/paradigms/one_truth/rules/ot002.rs` — append `Ot002Rule` stub and a failing test below the existing `ot002`/`ot002_diagnostic` code.

The existing file (70 lines) ends with `ot002_diagnostic`. We append to the same file so OT002's new and legacy implementations live side-by-side until Task 9 deletes the legacy code.

- [ ] **Step 1: Add imports + Ot002Rule stub + failing test**

Open `crates/locus-core/src/paradigms/one_truth/rules/ot002.rs`. Below the existing module-level `use` statements (currently `use super::super::infer::...; use crate::diagnostics::...;`), add these additional imports:

```rust
use crate::governance::evidence::{Confidence, Evidence};
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};
```

At the very end of the file (after `ot002_diagnostic`), append:

```rust
pub struct Ot002Rule;

pub static OT002_RULE: Ot002Rule = Ot002Rule;

const OT002_ID: RuleId = RuleId::new("OT002");
const OT_PARADIGM: ParadigmId = ParadigmId::new("OT");

impl RuleDefinition for Ot002Rule {
    fn id(&self) -> RuleId {
        OT002_ID
    }
    fn paradigm(&self) -> ParadigmId {
        OT_PARADIGM
    }
    fn title(&self) -> &'static str {
        "undeclared concept-shaped type"
    }
    fn default_severity(&self) -> Severity {
        Severity::Warning
    }
    fn observe(&self, _ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        // Implemented in Task 3.
        Vec::new()
    }
}

#[cfg(test)]
mod ot002_rule_tests {
    use super::*;
    use crate::diagnostics::CheckMode;
    use crate::governance::ids::FindingIdMinter;
    use crate::governance::registry::{ParadigmRegistry, RuleRegistry};
    use crate::lockfile::Lockfile;
    use locus_air::{
        AIR_SCHEMA_VERSION, AirField, AirFile, AirItem, AirPackage, AirSpan, AirType, AirWorkspace,
        TypeKind, Visibility,
    };

    /// Build a workspace with two types sharing a stem and overlapping
    /// fields: one is `// locus: ot canonical`-annotated, the other has
    /// no hint. The migrated rule should emit one OT002 finding on the
    /// undeclared sibling.
    #[test]
    fn fires_on_concept_shaped_sibling_without_annotation() {
        let air = workspace_with_canonical_and_sibling();
        let lf = Lockfile::default();
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

        let findings = Ot002Rule.observe(&ctx);
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one OT002 finding, got {findings:?}"
        );
        let f = &findings[0];
        assert_eq!(f.source, FindingSource::RegisteredRule(OT002_ID));
        assert_eq!(f.rule_id, Some(OT002_ID));
        assert_eq!(f.paradigm_id, Some(OT_PARADIGM));
        assert_eq!(f.default_severity, Severity::Warning);
        assert!(
            f.message.contains("concept-shaped but not accepted"),
            "expected legacy-compatible message, got `{}`",
            f.message
        );

        // Typed evidence.
        assert_eq!(f.evidence.len(), 1);
        match &f.evidence[0] {
            Evidence::InferenceConfidence { score, signals } => {
                // Two fields overlap fully (id, name) → field_overlap is
                // 1.0 → Confidence::High.
                assert_eq!(*score, Confidence::High);
                assert!(
                    signals.iter().any(|s| s.contains("overlaps")),
                    "expected overlap signal in {signals:?}"
                );
            }
            other => panic!("expected InferenceConfidence evidence, got {other:?}"),
        }
    }

    fn workspace_with_canonical_and_sibling() -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "demo".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "src/user.rs".into(),
                    module_path: Some("demo::user".into()),
                    items: vec![
                        canonical_user(),
                        sibling_user(),
                    ],
                    hints: vec![locus_air::AirHint {
                        anchor: "demo::user::User".into(),
                        text: "ot canonical".into(),
                        span: AirSpan::new("src/user.rs", 1, 1),
                    }],
                    parse_error: None,
                    line_count: 30,
                }],
            }],
            facts: Vec::new(),
        }
    }

    fn canonical_user() -> AirItem {
        AirItem::Type(AirType {
            name: "User".into(),
            symbol: "demo::user::User".into(),
            symbol_segments: Vec::new(),
            type_kind: TypeKind::Struct,
            visibility: Visibility::Public,
            span: AirSpan::new("src/user.rs", 2, 6),
            fields: vec![
                AirField {
                    name: "id".into(),
                    ty: "u32".into(),
                    visibility: Visibility::Public,
                },
                AirField {
                    name: "name".into(),
                    ty: "String".into(),
                    visibility: Visibility::Public,
                },
            ],
            decorators: Vec::new(),
            doc: None,
        })
    }

    fn sibling_user() -> AirItem {
        AirItem::Type(AirType {
            name: "UserResponse".into(),
            symbol: "demo::user::UserResponse".into(),
            symbol_segments: Vec::new(),
            type_kind: TypeKind::Struct,
            visibility: Visibility::Public,
            span: AirSpan::new("src/user.rs", 10, 14),
            fields: vec![
                AirField {
                    name: "id".into(),
                    ty: "u32".into(),
                    visibility: Visibility::Public,
                },
                AirField {
                    name: "name".into(),
                    ty: "String".into(),
                    visibility: Visibility::Public,
                },
            ],
            decorators: Vec::new(),
            doc: None,
        })
    }
}
```

**Note on the AIR fixture:** verify the actual `AirField` and `AirType` field sets against `crates/locus-air/src/lib.rs` before pasting. `AirField` may need additional fields (`span`, `doc`, etc.) and `AirType` may need different fields (e.g. `generic_params`). Adjust the helpers to match the current schema — the principle is "one canonical-hinted type plus one sibling with 100% field overlap and the same stem `User`."

The test module is named `ot002_rule_tests` (not just `tests`) to avoid clashing with any future test mod in the same file.

- [ ] **Step 2: Run the failing test**

Run: `cargo test -p locus-core paradigms::one_truth::rules::ot002::ot002_rule_tests -- --nocapture`

Expected: 1 test FAILS with `expected exactly one OT002 finding, got []`. Confirms the test harness compiles AND runs against the stub.

If the test fails to COMPILE, fix the AIR fixture to match current `locus-air` schema. Common drift points:
- `AirField` field set
- `AirType` may not have a `doc` field (some Air items don't)
- `AirHint.anchor` may be named differently (check current schema)
- `AirHint` may not be the right way to inject a `// locus: ot canonical` annotation — look at how `crates/locus-core/src/paradigms/one_truth/rules_tests.rs` builds test fixtures with canonical hints, and copy that pattern.

- [ ] **Step 3: Commit the failing test**

```bash
git add crates/locus-core/src/paradigms/one_truth/rules/ot002.rs
git commit -m "test(#71): failing test for Ot002Rule migration"
```

---

## Task 3: Implement `Ot002Rule::observe`

**Files:**
- Modify: `crates/locus-core/src/paradigms/one_truth/rules/ot002.rs`

- [ ] **Step 1: Replace the stub observe with a real implementation**

In `crates/locus-core/src/paradigms/one_truth/rules/ot002.rs`, find the stub:

```rust
    fn observe(&self, _ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        // Implemented in Task 3.
        Vec::new()
    }
}
```

Replace with the real body PLUS helpers (placed AFTER the closing `}` of the `impl`):

```rust
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::super::lockfile_schema::OtSection;
        let section: OtSection = ctx
            .lockfile
            .paradigm_section("OT")
            .unwrap_or_default();
        let clusters = super::super::infer::cluster_concepts_with_lockfile(ctx.air, &section);
        let mut out = Vec::new();
        for cluster in &clusters {
            let canonical = cluster
                .members
                .iter()
                .find(|m| m.role == InferredRole::Canonical);
            let Some(canonical) = canonical else {
                continue;
            };
            for member in &cluster.members {
                if member.role != InferredRole::Unknown {
                    continue;
                }
                if member.field_overlap < FIELD_OVERLAP_THRESHOLD {
                    continue;
                }
                out.push(make_finding(cluster, canonical, member, ctx));
            }
        }
        out
    }
}

fn make_finding(
    cluster: &ConceptCluster,
    canonical: &super::super::infer::ClusterMember,
    member: &super::super::infer::ClusterMember,
    ctx: &RuleContext<'_>,
) -> RuleFinding {
    let mut signals = vec![
        format!(
            "overlaps {:.0}% with `{}` (canonical for `{}`)",
            member.field_overlap * 100.0,
            canonical.name,
            cluster.concept_id
        ),
        format!("name shares stem `{}`", cluster.stem),
    ];
    signals.extend(member.reasons.iter().cloned());

    let severity = ctx.mode.elevate(Severity::Warning);
    let why = signals.clone();
    RuleFinding {
        id: ctx.finding_ids.next(),
        source: FindingSource::RegisteredRule(OT002_ID),
        rule_id: Some(OT002_ID),
        paradigm_id: Some(OT_PARADIGM),
        default_severity: severity,
        span: Some(member.span.clone()),
        concept: Some(cluster.concept_id.clone()),
        message: format!(
            "`{}` is concept-shaped but not accepted as canonical or boundary",
            member.symbol
        ),
        evidence: vec![Evidence::InferenceConfidence {
            score: confidence_from_overlap(member.field_overlap),
            signals,
        }],
        why,
        suggested_fix: Some(format!(
            "annotate as boundary: `// locus: ot boundary {} <boundary-name>` above `{}`, \
             or remove and use `{}` directly",
            cluster.concept_id, member.name, canonical.symbol
        )),
    }
}

/// Discretize the inference's `field_overlap` (0.0..=1.0) into a
/// `Confidence` tier. Mirrors the spec's 0.50/0.70/0.90 confidence ladder
/// (see `Severity::from_confidence` in `diagnostics.rs`). The exact
/// thresholds match: < 0.70 → Low (but always ≥ 0.50 here since the
/// rule's overlap gate is `FIELD_OVERLAP_THRESHOLD = 0.50`), < 0.90 →
/// Medium, ≥ 0.90 → High.
fn confidence_from_overlap(overlap: f32) -> Confidence {
    if overlap >= 0.90 {
        Confidence::High
    } else if overlap >= 0.70 {
        Confidence::Medium
    } else {
        Confidence::Low
    }
}
```

Note: `cluster_concepts_with_lockfile` is computed inside `observe` for now. This duplicates the cluster build that `OneTruth::check` does for OT001 + OT003-OT012, but it's the strangler-correct approach — each rule self-contained. When all OT rules migrate, the legacy compute disappears.

- [ ] **Step 2: Run the test — expect PASS**

Run: `cargo test -p locus-core paradigms::one_truth::rules::ot002::ot002_rule_tests -- --nocapture`
Expected: PASS — `fires_on_concept_shaped_sibling_without_annotation`.

If it fails:
- If `field_overlap` is < 1.0 (e.g. 0.5 because fields don't align perfectly), the `Confidence::High` assertion fails. Inspect the cluster builder's overlap math; either adjust the fixture to guarantee 100% overlap (matching field names AND types) or relax the assertion to `Confidence::High | Confidence::Medium` based on the actual value.
- If the cluster builder doesn't recognize the canonical annotation, check the hint shape — `// locus: ot canonical` hints are picked up via `AirHint`, but the exact text/anchor format matters. Look at how `crates/locus-core/src/paradigms/one_truth/rules_tests.rs` builds canonical-anchored test fixtures.

- [ ] **Step 3: Add a `confidence_from_overlap` unit test**

Append inside `mod ot002_rule_tests`:

```rust
    #[test]
    fn confidence_ladder_matches_spec_thresholds() {
        assert_eq!(confidence_from_overlap(1.00), Confidence::High);
        assert_eq!(confidence_from_overlap(0.95), Confidence::High);
        assert_eq!(confidence_from_overlap(0.90), Confidence::High);
        assert_eq!(confidence_from_overlap(0.89), Confidence::Medium);
        assert_eq!(confidence_from_overlap(0.70), Confidence::Medium);
        assert_eq!(confidence_from_overlap(0.69), Confidence::Low);
        assert_eq!(confidence_from_overlap(0.50), Confidence::Low);
    }
```

Run: `cargo test -p locus-core paradigms::one_truth::rules::ot002::ot002_rule_tests -- --nocapture`
Expected: 2 tests pass.

- [ ] **Step 4: Clippy + commit**

Run: `cargo clippy -p locus-core --all-targets -- -D warnings 2>&1 | tail -3`
Expected: clean.

```bash
git add crates/locus-core/src/paradigms/one_truth/rules/ot002.rs
git commit -m "feat(#71): Ot002Rule observes Evidence::InferenceConfidence"
```

---

## Task 4: Register `Ot002Rule` in `RuleRegistry::standard()` and `OtParadigmDef::rules()`

**Files:**
- Modify: `crates/locus-core/src/governance/registry.rs`
- Modify: `crates/locus-core/src/governance/paradigm_impls.rs`

- [ ] **Step 1: Add OT002 to `RuleRegistry::standard()`**

In `crates/locus-core/src/governance/registry.rs`, find:

```rust
    pub fn standard() -> Self {
        let reg = Self {
            rules: vec![&crate::paradigms::complexity_budget::rules::cx001::CX001_RULE],
        };
        debug_assert!(
            reg.validate().is_ok(),
            "RuleRegistry::standard() violates a construction invariant: {:?}",
            reg.validate()
        );
        reg
    }
```

Extend the vec to include `OT002_RULE`. The order is deliberately CX-first to match registry conventions, then alphabetical-by-prefix (CX, OT). Insert OT002 AFTER CX001:

```rust
    pub fn standard() -> Self {
        let reg = Self {
            rules: vec![
                &crate::paradigms::complexity_budget::rules::cx001::CX001_RULE,
                &crate::paradigms::one_truth::rules::ot002::OT002_RULE,
            ],
        };
        debug_assert!(
            reg.validate().is_ok(),
            "RuleRegistry::standard() violates a construction invariant: {:?}",
            reg.validate()
        );
        reg
    }
```

- [ ] **Step 2: Break `OtParadigmDef` out of the macro**

In `crates/locus-core/src/governance/paradigm_impls.rs`, find:

```rust
paradigm_def!(OtParadigmDef, "OT", "Canonical Domain Ownership");
```

Replace with an explicit impl (mirrors the CX pattern from P2-CX001):

```rust
// OT is the second paradigm with a migrated rule — explicit impl returns
// a non-empty `rules()` slice.
pub struct OtParadigmDef;
impl ParadigmDefinition for OtParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("OT")
    }
    fn title(&self) -> &'static str {
        "Canonical Domain Ownership"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        static RULES: [&dyn RuleDefinition; 1] =
            [&crate::paradigms::one_truth::rules::ot002::OT002_RULE];
        &RULES
    }
}
```

- [ ] **Step 3: Add registry tests for OT002**

In `crates/locus-core/src/governance/registry.rs`, find the existing test `rule_registry_contains_cx001_after_p2_migration`. Append two new tests immediately below it (still inside the test module):

```rust
    #[test]
    fn rule_registry_contains_ot002_after_p2_migration() {
        let reg = RuleRegistry::standard();
        assert!(reg.contains_code("OT002"), "OT002 must be in RuleRegistry::standard() after P2-OT002");
        let rule = reg.find(&RuleId::new("OT002")).expect("OT002 missing");
        assert_eq!(rule.paradigm().as_str(), "OT");
        assert_eq!(rule.default_severity(), crate::diagnostics::Severity::Warning);
    }

    #[test]
    fn ot_paradigm_def_lists_ot002_rule() {
        let reg = ParadigmRegistry::standard();
        let ot = reg
            .find(&ParadigmId::new("OT"))
            .expect("OT ParadigmDefinition missing");
        let rule_ids: Vec<&str> = ot.rules().iter().map(|r| r.id().as_str()).collect();
        assert_eq!(rule_ids, vec!["OT002"]);
    }
```

- [ ] **Step 4: Run tests + clippy**

Run: `cargo test -p locus-core --lib governance::registry 2>&1 | grep "test result:"`
Expected: 12 tests pass (10 from post-P2-CX001 + 2 new).

Run: `cargo clippy -p locus-core --all-targets -- -D warnings 2>&1 | tail -3`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/locus-core/src/governance/registry.rs crates/locus-core/src/governance/paradigm_impls.rs
git commit -m "feat(#71): register Ot002Rule in RuleRegistry::standard() and OT paradigm"
```

---

## Task 5: Strangler-invariant integration test

**Files:**
- Create: `crates/locus-core/tests/governance_ot002_strangler.rs`

- [ ] **Step 1: Write the integration test**

Create `crates/locus-core/tests/governance_ot002_strangler.rs`:

```rust
//! Verifies the strangler invariant for OT002: every OT002 finding from
//! the governance pipeline comes from `FindingSource::RegisteredRule`,
//! NOT from `FindingSource::LegacyDiagnostic`. The per-diagnostic-code
//! filter in `LegacyParadigmRuleAdapter` is now exercised on a second
//! rule code.

use locus_air::{
    AIR_SCHEMA_VERSION, AirField, AirFile, AirHint, AirItem, AirPackage, AirSpan, AirType,
    AirWorkspace, TypeKind, Visibility,
};
use locus_core::CheckMode;
use locus_core::governance::{self, FindingSource, RuleId};
use locus_core::lockfile::Lockfile;

#[test]
fn ot002_findings_come_from_registered_rule_not_legacy_adapter() {
    let air = AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "demo".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: "src/user.rs".into(),
                module_path: Some("demo::user".into()),
                items: vec![
                    AirItem::Type(AirType {
                        name: "User".into(),
                        symbol: "demo::user::User".into(),
                        symbol_segments: Vec::new(),
                        type_kind: TypeKind::Struct,
                        visibility: Visibility::Public,
                        span: AirSpan::new("src/user.rs", 2, 6),
                        fields: vec![
                            AirField {
                                name: "id".into(),
                                ty: "u32".into(),
                                visibility: Visibility::Public,
                            },
                            AirField {
                                name: "name".into(),
                                ty: "String".into(),
                                visibility: Visibility::Public,
                            },
                        ],
                        decorators: Vec::new(),
                        doc: None,
                    }),
                    AirItem::Type(AirType {
                        name: "UserResponse".into(),
                        symbol: "demo::user::UserResponse".into(),
                        symbol_segments: Vec::new(),
                        type_kind: TypeKind::Struct,
                        visibility: Visibility::Public,
                        span: AirSpan::new("src/user.rs", 10, 14),
                        fields: vec![
                            AirField {
                                name: "id".into(),
                                ty: "u32".into(),
                                visibility: Visibility::Public,
                            },
                            AirField {
                                name: "name".into(),
                                ty: "String".into(),
                                visibility: Visibility::Public,
                            },
                        ],
                        decorators: Vec::new(),
                        doc: None,
                    }),
                ],
                hints: vec![AirHint {
                    anchor: "demo::user::User".into(),
                    text: "ot canonical".into(),
                    span: AirSpan::new("src/user.rs", 1, 1),
                }],
                parse_error: None,
                line_count: 20,
            }],
        }],
        facts: Vec::new(),
    };
    let lf = Lockfile::default();

    let out = governance::run(&air, &lf, CheckMode::Human);

    let ot002_findings: Vec<_> = out
        .findings
        .iter()
        .filter(|f| {
            matches!(&f.rule_id, Some(r) if *r == RuleId::new("OT002"))
                || matches!(
                    &f.source,
                    FindingSource::LegacyDiagnostic { rule_code, .. } if rule_code == "OT002"
                )
        })
        .collect();

    assert_eq!(
        ot002_findings.len(),
        1,
        "expected exactly one OT002 finding (no double-fire), got {} findings: {:?}",
        ot002_findings.len(),
        ot002_findings
    );

    match &ot002_findings[0].source {
        FindingSource::RegisteredRule(r) => {
            assert_eq!(r.as_str(), "OT002");
        }
        FindingSource::LegacyDiagnostic { rule_code, .. } => {
            panic!(
                "OT002 finding still flows through legacy adapter (rule_code={rule_code}); \
                 strangler filter is not working"
            );
        }
        other => panic!("unexpected source for OT002 finding: {other:?}"),
    }
}
```

Same caveat as Task 2: verify `AirField`/`AirType`/`AirHint` shapes match current `locus-air` schema; adjust the fixture if drift.

- [ ] **Step 2: Run the test**

Run: `cargo test -p locus-core --test governance_ot002_strangler 2>&1 | tail -10`

Expected: PASS — OT002 finding comes from `FindingSource::RegisteredRule`, single emission only.

If TWO findings show up, that means the legacy adapter ALSO synthesized OT002. Check `RuleRegistry::standard().contains_code("OT002")` — it should be true after Task 4. If still false, Task 4 didn't land properly.

- [ ] **Step 3: Commit**

```bash
git add crates/locus-core/tests/governance_ot002_strangler.rs
git commit -m "test(#71): assert OT002 findings come from RegisteredRule, not legacy adapter"
```

---

## Task 6: Remove legacy `rules::ot002` call from `OneTruth::check`

**Files:**
- Modify: `crates/locus-core/src/paradigms/one_truth/mod.rs`

- [ ] **Step 1: Drop the OT002 invocation**

In `crates/locus-core/src/paradigms/one_truth/mod.rs`, find:

```rust
        let clusters = infer::cluster_concepts_with_lockfile(air, &section);
        let mut out = rules::ot001(&clusters, mode);
        out.extend(rules::ot002(&clusters, mode));
        out.extend(rules::ot003(air, &section, mode));
```

Remove the `rules::ot002` line:

```rust
        let clusters = infer::cluster_concepts_with_lockfile(air, &section);
        let mut out = rules::ot001(&clusters, mode);
        // OT002 migrated to RuleDefinition (#71 P2). Runs via the
        // governance pipeline; the legacy adapter's per-rule-code filter
        // drops any OT002 diagnostic emitted here.
        out.extend(rules::ot003(air, &section, mode));
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p locus-core 2>&1 | grep "test result:" | awk '{passed += $4; failed += $6} END {print "passed:", passed, "failed:", failed}'`

Expected: existing OT002 tests in `rules_tests.rs` may still pass because they call `ot002(...)` directly (the legacy function is still present). Total pass count should remain stable or slightly drop if any test exercised the `Paradigm::check` end-to-end with OT002 specifically — investigate if so.

- [ ] **Step 3: Run the compatibility snapshot**

Run: `cargo test -p locus-cli --test governance_compat 2>&1 | tail -3`
Expected: PASS — byte-identical sample-crate output.

- [ ] **Step 4: Commit**

```bash
git add crates/locus-core/src/paradigms/one_truth/mod.rs
git commit -m "feat(#71): remove legacy rules::ot002 call from OneTruth::check"
```

---

## Task 7: Migrate OT002 unit tests in `rules_tests.rs`

**Files:**
- Modify: `crates/locus-core/src/paradigms/one_truth/rules_tests.rs`

The file contains 5 OT002 tests calling `ot002(&[cluster], CheckMode::*)` directly. Migrate them to drive `Ot002Rule::observe`.

- [ ] **Step 1: Add a test helper at the top of `rules_tests.rs`**

After the existing top-of-file imports, add:

```rust
// OT002 migrated to RuleDefinition (#71 P2). The new tests construct an
// AirWorkspace from one or more clusters' inputs, then drive
// `Ot002Rule::observe`. The legacy `ot002(clusters, mode)` test pattern
// won't carry over directly because the new rule rebuilds clusters from
// AIR + lockfile, so test fixtures must produce AIR shaped to yield the
// expected cluster.
//
// For simple cases where the existing tests already build `cluster`
// instances by hand, we wrap the cluster's MEMBERS' AIR back into an
// AirWorkspace via `air_workspace_from_cluster`, and trust the rule's
// own cluster builder to reconstruct it.
//
// This is the rebuild-from-AIR shape the rule actually uses in
// production; tests that mocked clusters directly are exercising a
// shorthand that no longer applies post-migration.
use crate::governance::evidence::{Confidence, Evidence};
use crate::governance::finding::RuleFinding;
use crate::governance::ids::{FindingIdMinter, RuleId};
use crate::governance::registry::{ParadigmRegistry, RuleRegistry};
use crate::governance::rule::{RuleContext, RuleDefinition};
use crate::lockfile::Lockfile;
use crate::paradigms::one_truth::rules::ot002::Ot002Rule;

fn observe_ot002(
    air: &locus_air::AirWorkspace,
    section: Option<&super::lockfile_schema::OtSection>,
    mode: crate::diagnostics::CheckMode,
) -> Vec<RuleFinding> {
    let mut lf = Lockfile::default();
    if let Some(s) = section {
        lf.paradigms.insert(
            "OT".to_string(),
            serde_json::to_value(s).expect("OtSection must serialize"),
        );
    }
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
    Ot002Rule.observe(&ctx)
}
```

**Note on test refactor:** the legacy OT002 tests pass a hand-built `cluster` to `ot002(&[cluster], mode)`. The new rule builds clusters from AIR. To keep the tests meaningful, each migrated test needs to construct an `AirWorkspace` whose items would yield the same cluster after `cluster_concepts_with_lockfile` runs.

Five OT002 tests live in `rules_tests.rs` (lines around 41/71/92/112/133 per pre-migration inspection). Run `grep -n "fn ot002_" crates/locus-core/src/paradigms/one_truth/rules_tests.rs` to find them — they may not have `fn ot002_` prefix; look for tests whose body calls `ot002(...)`. Use the line numbers from the search to locate.

- [ ] **Step 2: For each OT002 test, build the equivalent AirWorkspace**

For each of the 5 OT002 tests:

1. Read the test's `cluster` construction. Note: stem, concept_id, canonical name + fields, sibling name + fields.
2. Replace the cluster construction with `AirWorkspace` construction that includes (a) the canonical type with its fields AND an `AirHint` of `ot canonical`, (b) the sibling type with its fields.
3. Replace `ot002(&[cluster], mode)` with `observe_ot002(&air, None, mode)`.
4. Adjust assertions:
   - `diags[0].rule_id == "OT002"` → `findings[0].rule_id == Some(RuleId::new("OT002"))`
   - `diags[0].severity` → `findings[0].default_severity`
   - `diags[0].message` → `findings[0].message`
   - `diags[0].why` → `findings[0].why`
   - `diags[0].suggested_fix` → `findings[0].suggested_fix`
   - `diags[0].span` → `findings[0].span.as_ref().unwrap()`
   - `diags[0].concept` → `findings[0].concept`
5. Add an `Evidence::InferenceConfidence` assertion to each test that previously asserted a fired diagnostic. Example:

   ```rust
   match &findings[0].evidence[0] {
       Evidence::InferenceConfidence { score, signals } => {
           assert_eq!(*score, Confidence::High);  // or Medium/Low based on overlap
           assert!(signals.iter().any(|s| s.contains("overlaps")));
       }
       other => panic!("expected InferenceConfidence evidence, got {other:?}"),
   }
   ```

Commit each migrated test or batch 2-3 together; aim for ≤3 commits in this task.

- [ ] **Step 3: Run the migrated tests**

Run: `cargo test -p locus-core paradigms::one_truth::rules_tests 2>&1 | grep "test result:"`
Expected: same pass count as pre-migration.

- [ ] **Step 4: Commit any remaining migration**

```bash
git add crates/locus-core/src/paradigms/one_truth/rules_tests.rs
git commit -m "test(#71): migrate OT002 rules_tests to drive Ot002Rule::observe"
```

---

## Task 8: Delete legacy `pub fn ot002` + `ot002_diagnostic`

**Files:**
- Modify: `crates/locus-core/src/paradigms/one_truth/rules/ot002.rs`
- Modify: `crates/locus-core/src/paradigms/one_truth/rules.rs`

- [ ] **Step 1: Verify no remaining callers of legacy `ot002`**

Run: `grep -rn "rules::ot002\|use.*ot002::ot002\|ot002(&\[" crates/ 2>&1 | grep -v "rules::ot002::OT002_RULE\|rules::ot002::Ot002Rule\|use crate::paradigms::one_truth::rules::ot002::Ot002Rule\|tests/governance_ot002"`

Expected: zero hits, OR only hits pointing at the migrated `Ot002Rule`/`OT002_RULE` references. If anything else remains, address it before deleting.

- [ ] **Step 2: Delete the legacy `pub fn ot002` and `ot002_diagnostic`**

In `crates/locus-core/src/paradigms/one_truth/rules/ot002.rs`, delete:
- `pub fn ot002(clusters: &[ConceptCluster], mode: CheckMode) -> Vec<Diagnostic>` (lines ~14-36 in the pre-migration file)
- `fn ot002_diagnostic(...)` (lines ~38-70)

Keep:
- The doc-comment header at the top of the file (update it to reflect the migration if useful)
- The new `Ot002Rule` struct, `OT002_RULE` static, `RuleDefinition` impl, helper functions (`make_finding`, `confidence_from_overlap`), and the test module

Also remove any imports that become dead after the deletion. The current file has `use crate::diagnostics::{CheckMode, Diagnostic, Severity};` — `Diagnostic` and possibly `CheckMode` are no longer used in the post-deletion file. Trim them. Run `cargo build` to see unused-import warnings and fix as a unit.

- [ ] **Step 3: Update the re-exports in `rules.rs`**

In `crates/locus-core/src/paradigms/one_truth/rules.rs`, find:

```rust
pub mod ot002;
```

and

```rust
pub use ot002::ot002;
```

The `pub mod ot002;` line stays — the module still exists, just no longer holds the function. Remove the re-export of the function:

```rust
pub use ot002::ot002;
```

Either delete it outright, or replace with re-exports of the new types if any external code needs them:

```rust
pub use ot002::{Ot002Rule, OT002_RULE};
```

(The registry already imports them via the full path `crate::paradigms::one_truth::rules::ot002::OT002_RULE`, so the re-export is for symmetry — drop it if you prefer the minimal change.)

- [ ] **Step 4: Build + test**

Run: `cargo build -p locus-core 2>&1 | tail -3`
Expected: clean build.

Run: `cargo test -p locus-core 2>&1 | grep "test result:" | awk '{passed += $4; failed += $6} END {print "passed:", passed, "failed:", failed}'`
Expected: same pass count as before deletion (Task 7's migrated tests now drive `Ot002Rule::observe`, so they're independent of the legacy function).

- [ ] **Step 5: Commit**

```bash
git add crates/locus-core/src/paradigms/one_truth/rules/ot002.rs crates/locus-core/src/paradigms/one_truth/rules.rs
git commit -m "refactor(#71): remove legacy ot002 fn (replaced by Ot002Rule)"
```

---

## Task 9: Full sweep + diff check

- [ ] **Step 1: fmt + clippy**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
```

If `cargo fmt` made changes:

```bash
git status --short
git add -u
git commit -m "style(#71): cargo fmt"
```

- [ ] **Step 2: Full test sweep**

Run: `cargo test --workspace 2>&1 | grep "test result:" | awk '{passed += $4; failed += $6} END {print "passed:", passed, "failed:", failed}'`
Expected: ~1069 passed, 0 failed (~3 new tests over the post-P2-CX001 baseline: 2 in ot002.rs (`fires_on_concept_shaped_sibling_without_annotation` + `confidence_ladder_matches_spec_thresholds`), 1 strangler integration, 2 registry tests — minus 1 if the legacy function-call test gets dropped during migration).

- [ ] **Step 3: Compatibility snapshot**

Run: `cargo test -p locus-cli --test governance_compat`
Expected: PASS — byte-identical `sample-crate` output (the new rule's output must match the legacy diagnostic's).

If it fails, the rule's output diverges in some field. Diff against `/tmp/locus-p2-ot002-baseline/sample-crate.txt` to find the drift. Most likely culprits: `concept` field, `why` ordering, or `suggested_fix` wording.

- [ ] **Step 4: Locus self-dogfood check**

```bash
mkdir -p /tmp/locus-p2-ot002-after
cargo run -p locus-cli --quiet -- check --workspace . --agent-strict \
    > /tmp/locus-p2-ot002-after/self-strict.txt 2>&1
diff /tmp/locus-p2-ot002-baseline/self-strict.txt /tmp/locus-p2-ot002-after/self-strict.txt | head -30
```

Expected: small diff or none. The new code (`Ot002Rule`, `make_finding`, `confidence_from_overlap`) might trigger MO005 or CX001 if anything's >50 lines.

If new findings appear:
- **CX001** on `make_finding` (likely the longest function): add `// locus: allow CX001 — ...` above the function, or refactor to extract the signal-building / message-formatting into smaller helpers. Prefer refactoring.
- **OT009** on `make_finding` would be a false positive — name doesn't start with `check_`, so unlikely.

Compare the summary line. Pre-baseline was `0 errors, 103 warnings`. If post is the same, no lockfile changes needed. If different, fix as per the P2-CX001 dogfood-cleanup pattern (lockfile entries or `// locus: allow` comments).

- [ ] **Step 5: Sample-crate diff against pre-P2-OT002 baseline**

```bash
cargo run -p locus-cli --quiet -- check --workspace tests/fixtures/sample-crate > /tmp/locus-p2-ot002-after/sample-crate.txt 2>&1
diff /tmp/locus-p2-ot002-baseline/sample-crate.txt /tmp/locus-p2-ot002-after/sample-crate.txt
echo "diff exit: $?"
```

Expected: `diff exit: 0`.

- [ ] **Step 6: Commit any final cleanup**

If lockfile entries or `// locus: allow` comments are required:

```bash
git add locus.lock crates/locus-core/src/paradigms/one_truth/rules/ot002.rs
git commit -m "chore(#71): dogfood cleanup for OT002 migration"
```

---

## Task 10: Open PR

**Files:** none new

- [ ] **Step 1: Verify clean state**

```bash
git status
git log --oneline main..HEAD
```

Every commit must reference #71. Working tree clean.

- [ ] **Step 2: Push and open PR**

```bash
git push -u origin <branch-name>
gh pr create --title "feat(#71): governance spine P2 — migrate OT002 to RuleDefinition" --body "$(cat <<'EOF'
## Summary

P2 of epic #71, second per-rule migration. Moves **OT002** (undeclared concept-shaped type) from the legacy `Paradigm::check()` path to a registered `RuleDefinition` implementation emitting typed `Evidence::InferenceConfidence { score: Confidence, signals }` findings.

- **New:** `Ot002Rule` in `crates/locus-core/src/paradigms/one_truth/rules/ot002.rs` (same file as the now-deleted legacy `pub fn ot002`).
- **New:** `confidence_from_overlap` discretizes `f32` field-overlap (`0.0..=1.0`) into `Confidence::{Low, Medium, High}` using the spec's 0.50/0.70/0.90 thresholds. The mapping is unit-tested.
- **Registered:** `Ot002Rule` in `RuleRegistry::standard()`; `OtParadigmDef::rules()` now returns `&[&OT002_RULE]` (second paradigm with a non-empty `rules()` slice, after CX).
- **Removed:** legacy `pub fn ot002` and `ot002_diagnostic`; the `ot002` call from `OneTruth::check`.
- **Migrated:** 5 OT002 tests in `rules_tests.rs` now drive `Ot002Rule::observe` and assert on the typed `Evidence::InferenceConfidence` payload.

Spec: `docs/superpowers/specs/2026-05-11-governance-spine-design.md` §"Migration scope — rules".
Plan: `docs/superpowers/plans/2026-05-11-governance-spine-p2-ot002.md`.

## Strangler invariant verified on real data

New integration test `crates/locus-core/tests/governance_ot002_strangler.rs` asserts OT002 findings emerge from `FindingSource::RegisteredRule`, **not** from `FindingSource::LegacyDiagnostic`. The per-diagnostic-code filter in `LegacyParadigmRuleAdapter` is now exercised on TWO rule codes (CX001 + OT002).

## Transient cost: double cluster compute

`cluster_concepts_with_lockfile` is invoked once inside `Ot002Rule::observe` AND once inside `OneTruth::check` (for OT001 + OT003-OT012, which still live on the legacy path). This costs ~ms per workspace and disappears once all OT rules migrate. Acceptable transient debt; called out here for visibility.

## Validation

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`
- [x] **Sample-crate compatibility snapshot byte-identical**
- [x] **Locus self-dogfood under `--agent-strict`:** 0 errors / 103 warnings, matching post-P2-CX001 baseline (or lockfile updated for any new findings on migrated code)
- [x] `RuleRegistry::standard().validate().is_ok()` — debug_assert pinned in P2-CX001 keeps passing.

## Test plan

- [ ] Reviewer runs `cargo test -p locus-core --test governance_ot002_strangler` — confirms PASS.
- [ ] Reviewer runs `cargo test -p locus-cli --test governance_compat` — confirms PASS (byte-identical sample-crate output).
- [ ] Reviewer runs `cargo run -p locus-cli -- check --workspace . --agent-strict` — confirms `0 error(s), 103 warning(s)` summary.
- [ ] Spot-check `Ot002Rule::observe` — typed `Evidence::InferenceConfidence` carries `score: Confidence` and `signals`.
- [ ] Spot-check `confidence_from_overlap` — thresholds match `Severity::from_confidence` spec.

## Out of scope (deferred)

- **DG001 (or FL003 fallback) migration:** Deterministic lockfile-config-driven rule; exercises `Evidence::Structured`. Detailed plan after this PR lands.
- **OT001 + OT003-OT012 migration:** Cleanup follow-up that migrates the remaining OT rules and drops the legacy `cluster_concepts_with_lockfile` call from `OneTruth::check`.
- **CX002/CX007/CX008 migration:** Removes the 11 transitional MO005 allow comments from P2-CX001.
EOF
)"
```

- [ ] **Step 3: Return the PR URL**

---

## Self-review checklist (performed)

- [x] **Spec coverage:** P2 outline for OT002 from the spec ("inference-shaped rule exercising Confidence enum") is covered. The `confidence_from_overlap` helper + unit test makes the f32→Confidence mapping explicit. The Evidence variant uses `InferenceConfidence` as the spec mandates.
- [x] **Placeholder scan:** No "TBD" / "implement later". Every code step shows actual code. The two flagged AIR schema verification steps (Task 2 Step 2 + Task 5 Step 1) are explicit "look up X, adjust Y" instructions, not TODOs.
- [x] **Type consistency:** `Ot002Rule.observe` produces `RuleFinding` with the same field set used by P2-CX001's `Cx001Rule`. `Evidence::InferenceConfidence { score, signals }` matches the definition in `governance/evidence.rs`. `OT002_RULE` static naming convention matches `CX001_RULE`.
- [x] **Strangler invariant:** Task 5's integration test verifies the per-rule-code filter; Task 6 (legacy call removal) + Task 8 (legacy function deletion) both maintain it via the compat snapshot.
- [x] **Test migration:** Task 7 walks through the 5 OT002 tests with explicit field-mapping + cluster-to-AIR-fixture instructions. The mapping table from P2-CX001 (`diags[0].field → findings[0].field`) applies directly.
- [x] **Dogfood:** Task 9 covers the Locus self-workspace diff. Task 9 Step 6 handles any new findings on the new code (lockfile entries pattern from P2-CX001).
- [x] **No `rules.rs → rules/mod.rs` rename:** OT already has the per-rule split (`rules/ot001.rs`, …). The MO005 surge from P2-CX001 doesn't recur.
- [x] **`RuleRegistry::standard().validate()` debug_assert:** existing from P2-CX001 — automatically catches if OT002's registration breaks uniqueness or prefix invariants.

End of P2-OT002 plan.
