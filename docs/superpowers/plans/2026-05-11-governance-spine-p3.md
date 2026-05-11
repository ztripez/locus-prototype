# Governance Spine P3 — RegistryIntegrityPolicy

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `RegistryIntegrityPolicy` to the governance pipeline as the first real policy (before `DefaultPassThroughPolicy`). Its primary visible output is check 6 — one `LOCUS003` advisory finding per unique legacy rule code observed this run, surfacing migration backlog to users.

**Architecture:** `RegistryIntegrityPolicy` runs before `DefaultPassThroughPolicy` in `PolicyRegistry::standard()`. It inspects `ctx.findings` for `FindingSource::LegacyDiagnostic` entries, deduplicates by rule code, and emits one `RuleFinding` + one `Decision` per unique code. Decision status is `KnownTransitionDebt`, severity is `Advisory`. `DefaultPassThroughPolicy` handles all undecided findings as before. `LOCUS003` is already registered in `GovernanceDiagnosticRegistry` from P1.

**Tech Stack:** Rust 2024 edition, existing governance spine from P1/P2. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-05-11-governance-spine-design.md` §"RegistryIntegrityPolicy". Checks 1–5 and 7 are silent at runtime (registry construction caught them); check 6 (migration debt) is the visible output.

**Prior PRs:** P1 spine (#79), P2-CX001 (#80), P2-OT002 (#81), P2-DG001 (#82).

**Baseline (pre-PR):** `locus check --workspace .` → 0 errors / 104 warnings / 0 advisory.  
**Expected post-P3:** N unique un-migrated legacy rule codes → N new `LOCUS003` advisories added to the advisory count. This is intentional new output, not a regression.

---

## File structure

**Create:**
- `crates/locus-core/src/governance/policies/registry_integrity.rs` — `RegistryIntegrityPolicy` struct + `PolicyDefinition` impl + unit tests.
- `crates/locus-core/tests/governance_locus003_integration.rs` — integration test for `LOCUS003` emission via full `governance::run()`.

**Modify:**
- `crates/locus-core/src/governance/policies/mod.rs` — add `pub mod registry_integrity; pub use registry_integrity::RegistryIntegrityPolicy;`
- `crates/locus-core/src/governance/mod.rs` — add `pub use policies::RegistryIntegrityPolicy;`
- `crates/locus-core/src/governance/registry.rs` — add `RegistryIntegrityPolicy` to `PolicyRegistry::standard()` BEFORE `DefaultPassThroughPolicy`; add registry test asserting ordering.

---

## Task 1: Scaffold `RegistryIntegrityPolicy` (empty decide)

**Files:**
- Create: `crates/locus-core/src/governance/policies/registry_integrity.rs`
- Modify: `crates/locus-core/src/governance/policies/mod.rs`
- Modify: `crates/locus-core/src/governance/mod.rs`
- Modify: `crates/locus-core/src/governance/registry.rs`

- [ ] **Step 1: Write the failing test for ordering**

In `crates/locus-core/src/governance/registry.rs`, in the existing `tests` mod, add:

```rust
#[test]
fn registry_integrity_policy_is_before_pass_through() {
    let reg = PolicyRegistry::standard();
    let ids: Vec<&str> = reg.iter().map(|p| p.id().as_str()).collect();
    let ri_pos = ids.iter().position(|&id| id == "registry-integrity")
        .expect("RegistryIntegrityPolicy must be in PolicyRegistry::standard()");
    let pt_pos = ids.iter().position(|&id| id == "default-pass-through")
        .expect("DefaultPassThroughPolicy must be in PolicyRegistry::standard()");
    assert!(
        ri_pos < pt_pos,
        "RegistryIntegrityPolicy ({ri_pos}) must come before DefaultPassThroughPolicy ({pt_pos})"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p locus-core registry_integrity_policy_is_before_pass_through 2>&1 | tail -5`

Expected: FAIL with `RegistryIntegrityPolicy must be in PolicyRegistry::standard()` (panic) or similar.

- [ ] **Step 3: Create `registry_integrity.rs` with empty `decide()`**

```rust
//! `RegistryIntegrityPolicy` — governance health check.
//!
//! Runs BEFORE `DefaultPassThroughPolicy`. Inspects the finding store and
//! emits `LOCUS003` advisory findings for each unique legacy rule code
//! observed this run, surfacing migration backlog.

// locus: ot canonical

use std::collections::BTreeMap;

use crate::diagnostics::Severity;
use crate::governance::decision::{Decision, DecisionStatus, SeverityChange};
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::PolicyId;
use crate::governance::policy::{PolicyContext, PolicyDefinition, PolicyOutput};

pub struct RegistryIntegrityPolicy;

pub const REGISTRY_INTEGRITY_ID: PolicyId = PolicyId::new("registry-integrity");

impl PolicyDefinition for RegistryIntegrityPolicy {
    fn id(&self) -> PolicyId {
        REGISTRY_INTEGRITY_ID
    }

    fn title(&self) -> &'static str {
        "Registry Integrity"
    }

    fn decide(&self, _ctx: &PolicyContext<'_>) -> PolicyOutput {
        PolicyOutput::empty()
    }
}
```

- [ ] **Step 4: Update `policies/mod.rs`**

```rust
//! Policy implementations.
//!
//! `default` — `DefaultPassThroughPolicy` (always last in the policy
//! chain; decides every finding not already decided).
//!
//! `registry_integrity` — `RegistryIntegrityPolicy` (runs before
//! pass-through; emits LOCUS003 migration-debt advisories).

// locus: ot canonical

pub mod default;
pub mod registry_integrity;

pub use default::DefaultPassThroughPolicy;
pub use registry_integrity::RegistryIntegrityPolicy;
```

- [ ] **Step 5: Add re-export to `governance/mod.rs`**

In the `pub use policies::...` line in `governance/mod.rs`, add `RegistryIntegrityPolicy`:

```rust
pub use policies::{DefaultPassThroughPolicy, RegistryIntegrityPolicy};
```

- [ ] **Step 6: Register in `PolicyRegistry::standard()`**

In `registry.rs`, update `standard_policies()`:

```rust
fn standard_policies() -> Vec<&'static dyn PolicyDefinition> {
    // RegistryIntegrityPolicy MUST come before DefaultPassThroughPolicy.
    // Future policies (ExceptionPolicy, ...) insert between them.
    vec![
        &crate::governance::policies::registry_integrity::RegistryIntegrityPolicy,
        &crate::governance::policies::default::DefaultPassThroughPolicy,
    ]
}
```

- [ ] **Step 7: Run test to verify it passes**

Run: `cargo test -p locus-core registry_integrity_policy_is_before_pass_through 2>&1 | tail -5`

Expected: PASS.

- [ ] **Step 8: Run full test suite**

Run: `cargo test -p locus-core 2>&1 | grep -E "FAILED|^test result" | tail -5`

Expected: all pass. The empty `decide()` means no behavior change yet.

- [ ] **Step 9: Commit**

```bash
git add crates/locus-core/src/governance/policies/registry_integrity.rs \
        crates/locus-core/src/governance/policies/mod.rs \
        crates/locus-core/src/governance/mod.rs \
        crates/locus-core/src/governance/registry.rs
git commit -m "feat(#71): scaffold RegistryIntegrityPolicy (empty decide)"
```

---

## Task 2: Implement check 6 — migration debt LOCUS003

**Files:**
- Modify: `crates/locus-core/src/governance/policies/registry_integrity.rs`

- [ ] **Step 1: Write failing unit tests**

In the `#[cfg(test)]` block at the bottom of `registry_integrity.rs`, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{CheckMode, Severity};
    use crate::governance::finding::{FindingSource, FindingStore, RuleFinding};
    use crate::governance::ids::{FindingId, FindingIdMinter, ParadigmId, RuleId};
    use crate::governance::registry::{ParadigmRegistry, PolicyRegistry, RuleRegistry};
    use crate::lockfile::Lockfile;
    use locus_air::AirWorkspace;

    fn legacy_finding(id_raw: u64, rule_code: &str) -> RuleFinding {
        RuleFinding {
            id: FindingId::from_raw_for_test(id_raw),
            source: FindingSource::LegacyDiagnostic {
                rule_code: rule_code.into(),
                paradigm: Some(ParadigmId::new("CX")),
            },
            rule_id: None,
            paradigm_id: None,
            default_severity: Severity::Warning,
            span: None,
            concept: None,
            message: "msg".into(),
            evidence: Vec::new(),
            why: Vec::new(),
            suggested_fix: None,
            diagnostic_code: None,
        }
    }

    fn registered_finding(id_raw: u64, rule_id: &'static str) -> RuleFinding {
        RuleFinding {
            id: FindingId::from_raw_for_test(id_raw),
            source: FindingSource::RegisteredRule(RuleId::new(rule_id)),
            rule_id: Some(RuleId::new(rule_id)),
            paradigm_id: None,
            default_severity: Severity::Warning,
            span: None,
            concept: None,
            message: "msg".into(),
            evidence: Vec::new(),
            why: Vec::new(),
            suggested_fix: None,
            diagnostic_code: None,
        }
    }

    fn run_policy(store: FindingStore) -> PolicyOutput {
        let air = AirWorkspace::new(Vec::new());
        let lf = Lockfile::empty();
        let rules = RuleRegistry::standard();
        let paradigms = ParadigmRegistry::empty();
        let policies = PolicyRegistry::with_policies(vec![&RegistryIntegrityPolicy]);
        let minter = FindingIdMinter::new();
        let ctx = PolicyContext {
            air: &air,
            lockfile: &lf,
            mode: CheckMode::Human,
            rule_registry: &rules,
            paradigm_registry: &paradigms,
            policy_registry: &policies,
            findings: &store,
            prior_decisions: &[],
            finding_ids: &minter,
        };
        RegistryIntegrityPolicy.decide(&ctx)
    }

    #[test]
    fn silent_for_registered_rule_findings() {
        let mut store = FindingStore::new();
        store.insert(registered_finding(0, "CX001"));
        store.insert(registered_finding(1, "OT002"));
        store.insert(registered_finding(2, "DG001"));
        let out = run_policy(store);
        assert!(
            out.new_findings.is_empty(),
            "registered rules must not trigger LOCUS003; got {:?}",
            out.new_findings
        );
    }

    #[test]
    fn emits_one_locus003_per_unique_legacy_code() {
        let mut store = FindingStore::new();
        // Two CX002 findings — should produce exactly one LOCUS003
        store.insert(legacy_finding(0, "CX002"));
        store.insert(legacy_finding(1, "CX002"));
        let out = run_policy(store);
        assert_eq!(
            out.new_findings.len(),
            1,
            "two findings with same code → one LOCUS003; got {:?}",
            out.new_findings
        );
        let f = &out.new_findings[0];
        assert_eq!(f.diagnostic_code.as_deref(), Some("LOCUS003"));
        assert_eq!(f.default_severity, Severity::Advisory);
        assert!(
            f.message.contains("CX002"),
            "LOCUS003 message should name the rule code; got `{}`",
            f.message
        );
        assert!(
            f.message.contains("2 observation"),
            "LOCUS003 message should include observation count; got `{}`",
            f.message
        );
    }

    #[test]
    fn emits_one_locus003_per_distinct_legacy_code() {
        let mut store = FindingStore::new();
        store.insert(legacy_finding(0, "CX002"));
        store.insert(legacy_finding(1, "MO001"));
        store.insert(legacy_finding(2, "MO001"));
        let out = run_policy(store);
        let codes: Vec<_> = out
            .new_findings
            .iter()
            .map(|f| f.message.clone())
            .collect();
        assert_eq!(
            out.new_findings.len(),
            2,
            "two distinct codes → two LOCUS003; got {codes:?}"
        );
    }

    #[test]
    fn mixed_registered_and_legacy_only_legacy_gets_locus003() {
        let mut store = FindingStore::new();
        store.insert(registered_finding(0, "CX001"));
        store.insert(legacy_finding(1, "CX002"));
        let out = run_policy(store);
        assert_eq!(
            out.new_findings.len(),
            1,
            "only legacy findings should produce LOCUS003; got {:?}",
            out.new_findings
        );
        assert!(out.new_findings[0].message.contains("CX002"));
    }

    #[test]
    fn decisions_use_known_transition_debt_status() {
        let mut store = FindingStore::new();
        store.insert(legacy_finding(0, "MO002"));
        let out = run_policy(store);
        assert_eq!(out.decisions.len(), 1);
        let d = &out.decisions[0];
        assert_eq!(
            d.status,
            crate::governance::decision::DecisionStatus::KnownTransitionDebt
        );
        assert_eq!(d.severity, Severity::Advisory);
        assert_eq!(
            d.finding_id,
            out.new_findings[0].id,
            "decision must target the emitted LOCUS003 finding"
        );
    }

    #[test]
    fn locus003_advisory_stays_advisory_under_agent_strict() {
        let mut store = FindingStore::new();
        store.insert(legacy_finding(0, "CX002"));
        let air = AirWorkspace::new(Vec::new());
        let lf = Lockfile::empty();
        let rules = RuleRegistry::standard();
        let paradigms = ParadigmRegistry::empty();
        let policies = PolicyRegistry::with_policies(vec![&RegistryIntegrityPolicy]);
        let minter = FindingIdMinter::new();
        let ctx = PolicyContext {
            air: &air,
            lockfile: &lf,
            mode: CheckMode::AgentStrict,
            rule_registry: &rules,
            paradigm_registry: &paradigms,
            policy_registry: &policies,
            findings: &store,
            prior_decisions: &[],
            finding_ids: &minter,
        };
        let out = RegistryIntegrityPolicy.decide(&ctx);
        assert_eq!(out.new_findings[0].default_severity, Severity::Advisory);
        assert_eq!(out.decisions[0].severity, Severity::Advisory);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p locus-core registry_integrity 2>&1 | grep -E "FAILED|^test result"`

Expected: most tests FAIL (empty decide returns empty output).

- [ ] **Step 3: Implement `decide()` with check 6**

Replace the empty `decide()` in `registry_integrity.rs`:

```rust
fn decide(&self, ctx: &PolicyContext<'_>) -> PolicyOutput {
    // Check 6: migration debt — one LOCUS003 advisory per unique legacy
    // rule code observed this run. Dedup by code; count all instances.
    let mut code_counts: BTreeMap<String, usize> = BTreeMap::new();
    for f in ctx.findings.iter() {
        if let FindingSource::LegacyDiagnostic { rule_code, .. } = &f.source {
            *code_counts.entry(rule_code.clone()).or_insert(0) += 1;
        }
    }

    let mut new_findings = Vec::new();
    let mut decisions = Vec::new();

    for (code, count) in &code_counts {
        let plural = if *count == 1 { "" } else { "s" };
        let finding = RuleFinding {
            id: ctx.finding_ids.next(),
            source: FindingSource::Policy(REGISTRY_INTEGRITY_ID),
            rule_id: None,
            paradigm_id: None,
            default_severity: Severity::Advisory,
            span: None,
            concept: None,
            message: format!(
                "rule code {code} emitted via legacy paradigm runner; \
                 not yet migrated to RuleDefinition \
                 ({count} observation{plural} this run)"
            ),
            evidence: Vec::new(),
            why: Vec::new(),
            suggested_fix: Some(format!(
                "migrate {code} to a RuleDefinition implementation \
                 (governance spine epic #71)"
            )),
            diagnostic_code: Some("LOCUS003".into()),
        };
        let decision = Decision {
            finding_id: finding.id,
            policy: REGISTRY_INTEGRITY_ID,
            severity: Severity::Advisory,
            status: DecisionStatus::KnownTransitionDebt,
            severity_change: SeverityChange::Unchanged,
            rationale: vec![format!("{count} observation{plural} this run")],
        };
        new_findings.push(finding);
        decisions.push(decision);
    }

    PolicyOutput {
        new_findings,
        decisions,
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p locus-core registry_integrity 2>&1 | grep -E "FAILED|^test result"`

Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/locus-core/src/governance/policies/registry_integrity.rs
git commit -m "feat(#71): RegistryIntegrityPolicy emits LOCUS003 for legacy migration debt"
```

---

## Task 3: Integration test (full pipeline LOCUS003)

**Files:**
- Create: `crates/locus-core/tests/governance_locus003_integration.rs`

- [ ] **Step 1: Write the integration test**

```rust
//! Integration tests for LOCUS003 migration-debt emission.
//!
//! Verifies that `governance::run()` emits exactly one LOCUS003 advisory
//! per unique un-migrated legacy rule code, and that registered rules
//! (CX001 / OT002 / DG001) do not generate LOCUS003 findings.

use locus_air::{AirFile, AirImport, AirItem, AirPackage, AirSpan, AirWorkspace, Visibility};
use locus_core::governance::{self, FindingSource};
use locus_core::{CheckMode, Lockfile};

/// Build a workspace with one import that triggers DG001 (registered rule).
/// DG001 findings must NOT produce LOCUS003.
fn dg001_workspace() -> (AirWorkspace, Lockfile) {
    let air = AirWorkspace::new(vec![AirPackage {
        name: "pkg".into(),
        version: "0.0.1".into(),
        root_dir: "/tmp/pkg".into(),
        files: vec![AirFile {
            path: "src/feature_a/handler.rs".into(),
            module_path: Some("pkg::feature_a::handler".into()),
            items: vec![AirItem::Import(AirImport {
                path: "pkg::feature_b::internal".into(),
                path_segments: Vec::new(),
                visibility: Visibility::Module,
                span: AirSpan::new("src/feature_a/handler.rs", 1, 1),
            })],
            hints: Vec::new(),
            parse_error: None,
            line_count: 5,
        }],
    }]);
    let mut lf = Lockfile::default();
    let section = serde_json::json!({
        "forbidden_edges": [{"from": "pkg::feature_a::*", "to": "pkg::feature_b::*"}],
        "features": [],
        "shared_paths": []
    });
    lf.paradigms.insert("DG".to_string(), section);
    (air, lf)
}

#[test]
fn registered_rule_dg001_does_not_produce_locus003() {
    let (air, lf) = dg001_workspace();
    let out = governance::run(&air, &lf, CheckMode::Human);

    let locus003: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "LOCUS003")
        .collect();

    // DG001 is a registered rule — its findings must NOT trigger LOCUS003.
    assert!(
        locus003.is_empty(),
        "registered DG001 must not trigger LOCUS003; got: {locus003:?}"
    );

    // The DG001 diagnostic itself must still be present.
    let dg001: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "DG001")
        .collect();
    assert!(
        !dg001.is_empty(),
        "DG001 diagnostic must still be present"
    );
}

#[test]
fn locus003_advisory_never_elevates_under_agent_strict() {
    // Even in --agent-strict mode, LOCUS003 stays Advisory.
    // Use an empty workspace — if any legacy rules fire on it, each
    // unique code gets one LOCUS003. Severity must be Advisory, not Fatal.
    let air = AirWorkspace::new(Vec::new());
    let lf = Lockfile::empty();
    let out = governance::run(&air, &lf, CheckMode::AgentStrict);

    for d in out.diagnostics.iter().filter(|d| d.rule_id == "LOCUS003") {
        assert_eq!(
            d.severity,
            locus_core::Severity::Advisory,
            "LOCUS003 must stay Advisory under --agent-strict; got {:?}",
            d.severity
        );
    }
}

#[test]
fn locus003_findings_use_policy_source() {
    // LOCUS003 findings must come from FindingSource::Policy, not
    // LegacyDiagnostic or RegisteredRule.
    let air = AirWorkspace::new(Vec::new());
    let lf = Lockfile::empty();
    let out = governance::run(&air, &lf, CheckMode::Human);

    for f in out.findings.iter() {
        if f.diagnostic_code.as_deref() == Some("LOCUS003") {
            assert!(
                matches!(f.source, FindingSource::Policy(_)),
                "LOCUS003 finding must have Policy source; got {:?}",
                f.source
            );
        }
    }
}

#[test]
fn locus003_deduplicates_by_rule_code() {
    // Workspace that fires legacy DG002 multiple times (2-cycle
    // produces 2 diagnostics). There should be exactly one LOCUS003
    // for DG002, not two.
    use locus_air::AIR_SCHEMA_VERSION;
    let air = AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![
            AirPackage {
                name: "a".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "a/src/lib.rs".into(),
                    module_path: Some("a".into()),
                    items: vec![AirItem::Import(AirImport {
                        path: "b::Type1".into(),
                        path_segments: Vec::new(),
                        visibility: Visibility::Module,
                        span: AirSpan::new("a/src/lib.rs", 1, 1),
                    })],
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 2,
                }],
            },
            AirPackage {
                name: "b".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "b/src/lib.rs".into(),
                    module_path: Some("b".into()),
                    items: vec![AirItem::Import(AirImport {
                        path: "a::Type2".into(),
                        path_segments: Vec::new(),
                        visibility: Visibility::Module,
                        span: AirSpan::new("b/src/lib.rs", 1, 1),
                    })],
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 2,
                }],
            },
        ],
        facts: Vec::new(),
    };
    let lf = Lockfile::empty();
    let out = governance::run(&air, &lf, CheckMode::Human);

    // DG002 fires twice (one per edge in the cycle)
    let dg002_count = out.diagnostics.iter().filter(|d| d.rule_id == "DG002").count();
    assert!(dg002_count >= 2, "expected ≥2 DG002 diagnostics; got {dg002_count}");

    // But LOCUS003 for DG002 appears exactly once
    let locus003_for_dg002 = out
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "LOCUS003" && d.message.contains("DG002"))
        .count();
    assert_eq!(
        locus003_for_dg002,
        1,
        "DG002 should produce exactly one LOCUS003 regardless of instance count; got {locus003_for_dg002}"
    );
}
```

- [ ] **Step 2: Run the integration test**

Run: `cargo test -p locus-core --test governance_locus003_integration 2>&1 | tail -10`

Expected: all 4 tests PASS.

- [ ] **Step 3: Run full test suite**

Run: `cargo test --workspace 2>&1 | grep -E "FAILED|^test result" | head -10`

Expected: all PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/locus-core/tests/governance_locus003_integration.rs
git commit -m "test(#71): integration tests for LOCUS003 migration-debt emission"
```

---

## Task 4: Full sweep + dogfood

**Files:**
- Possibly `crates/locus-core/src/governance/policies/registry_integrity.rs` (if clippy warns)

- [ ] **Step 1: fmt + clippy**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | grep "^error" | head -10
```

Expected: clean.

- [ ] **Step 2: Run full test suite**

```bash
cargo test --workspace 2>&1 | grep -E "FAILED|^test result" | head -15
```

Expected: all PASS.

- [ ] **Step 3: Dogfood check — capture new advisory count**

```bash
cargo run -p locus-cli -- check --workspace . 2>&1 | grep "^summary:"
```

Expected shape: `0 error(s), 104 warning(s), N advisory.` where N > 0 (one per unique un-migrated legacy code firing on the Locus workspace). This is **intentional new output** per the spec — NOT a regression.

Record the N value. If N is unexpectedly large (e.g., >50), flag it. Each advisory is one unique rule code — a healthy number is ~10–30 for a codebase with many un-migrated rules.

- [ ] **Step 4: Verify no errors / warning count unchanged**

```bash
cargo run -p locus-cli -- check --workspace . 2>&1 | grep "^summary:" | grep "0 error"
```

The warning count (104) must stay unchanged. Only the advisory count goes up.

- [ ] **Step 5: Commit fmt if changed**

```bash
git add -p  # only if fmt changed anything
git commit -m "style: cargo fmt"
```

---

## Task 5: Open PR

- [ ] **Step 1: Push branch**

```bash
git push origin worktree-governance-spine-p3
```

- [ ] **Step 2: Create PR**

```bash
gh pr create \
  --title "feat(#71): P3 — RegistryIntegrityPolicy emits LOCUS003 migration-debt advisories" \
  --body "..."
```

PR body should include:
- Summary: what RegistryIntegrityPolicy does (check 6 — LOCUS003 per unique legacy code)
- Note that LOCUS003 advisories are **intentional new output** (spec §P3), not regressions
- Dogfood baseline: 0 errors / 104 warnings / N advisory (record actual N)
- Test plan checklist
