//! `RegistryCoherencePolicy` — governance health check.
//!
//! Verifies that the registered rule/paradigm/policy graph stays self-
//! consistent at runtime, and that the workspace's `.locus/arch.json`
//! declaration matches what's actually registered. Mirrors the
//! construction-time invariants in `RuleRegistry::validate()` but surfaces
//! drift as findings instead of startup panics.
//!
//! Emits one LOCUS004 advisory per drift:
//! - declaration file missing (`.locus/arch.json` not found)
//! - declaration parse error
//! - policy declared in arch.json but not registered in `PolicyRegistry`
//! - policy registered but not declared in arch.json (drift in either direction)
//! - registered rule's `paradigm()` doesn't resolve in `ParadigmRegistry`
//! - registered paradigm's `rules()` entry doesn't resolve in `RuleRegistry`
//!
//! Severity: always `Advisory`. The MVP runs advisory-only; future iterations
//! may opt into Warning/Fatal once arch drift is well-understood.

// locus: ot canonical

use std::collections::BTreeSet;

use crate::diagnostics::Severity;
use crate::governance::arch::{ARCH_RELATIVE_PATH, ArchLoadOutcome};
use crate::governance::decision::{Decision, DecisionStatus, SeverityChange};
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::PolicyId;
use crate::governance::policy::{PolicyContext, PolicyDefinition, PolicyOutput};

pub struct RegistryCoherencePolicy;

pub const REGISTRY_COHERENCE_ID: PolicyId = PolicyId::new("registry-coherence");

const LOCUS004: &str = "LOCUS004";

impl PolicyDefinition for RegistryCoherencePolicy {
    fn id(&self) -> PolicyId {
        REGISTRY_COHERENCE_ID
    }

    fn title(&self) -> &'static str {
        "Registry Coherence"
    }

    fn decide(&self, ctx: &PolicyContext<'_>) -> PolicyOutput {
        let mut new_findings = Vec::new();
        let mut decisions = Vec::new();

        // (1) Arch declaration availability.
        let registered_policies: BTreeSet<&'static str> = ctx
            .policy_registry
            .iter()
            .map(|p| p.id().as_str())
            .collect();

        match ctx.arch {
            ArchLoadOutcome::Missing => {
                let f = self.build_finding(
                    ctx,
                    format!(
                        "no architecture declaration found at `{ARCH_RELATIVE_PATH}` — \
                         create one to enable governance drift detection"
                    ),
                    Vec::new(),
                    Some(format!(
                        "create `{ARCH_RELATIVE_PATH}` with a `policies` array \
                         listing the governance policies you expect to be registered"
                    )),
                );
                let d = self.build_decision(f.id, vec!["arch declaration missing".into()]);
                new_findings.push(f);
                decisions.push(d);
            }
            ArchLoadOutcome::Invalid(err) => {
                let f = self.build_finding(
                    ctx,
                    format!("architecture declaration at `{ARCH_RELATIVE_PATH}` failed to parse"),
                    vec![format!("parse error: {err}")],
                    Some(format!(
                        "fix the JSON in `{ARCH_RELATIVE_PATH}` so it conforms to \
                         the `ArchDeclaration` schema (`{{\"policies\": [...]}}`)"
                    )),
                );
                let d = self.build_decision(f.id, vec!["arch declaration unparseable".into()]);
                new_findings.push(f);
                decisions.push(d);
            }
            ArchLoadOutcome::Present(decl) => {
                let declared: BTreeSet<&str> = decl.policies.iter().map(String::as_str).collect();

                // (2) Declared but not registered.
                for name in declared.difference(&registered_policies) {
                    let f = self.build_finding(
                        ctx,
                        format!(
                            "policy `{name}` declared in `{ARCH_RELATIVE_PATH}` is not \
                             registered in `PolicyRegistry::standard()`"
                        ),
                        Vec::new(),
                        Some(format!(
                            "either register the `{name}` PolicyDefinition in \
                             `PolicyRegistry::standard()`, or remove `{name}` from \
                             `{ARCH_RELATIVE_PATH}`"
                        )),
                    );
                    let d = self.build_decision(
                        f.id,
                        vec![format!("declared policy `{name}` not registered")],
                    );
                    new_findings.push(f);
                    decisions.push(d);
                }

                // (3) Registered but not declared.
                for name in registered_policies.difference(&declared) {
                    let f = self.build_finding(
                        ctx,
                        format!(
                            "policy `{name}` is registered but not declared in \
                             `{ARCH_RELATIVE_PATH}`"
                        ),
                        Vec::new(),
                        Some(format!(
                            "add `\"{name}\"` to the `policies` array in \
                             `{ARCH_RELATIVE_PATH}` to acknowledge it"
                        )),
                    );
                    let d = self.build_decision(
                        f.id,
                        vec![format!("registered policy `{name}` not declared")],
                    );
                    new_findings.push(f);
                    decisions.push(d);
                }
            }
        }

        // (4) Rule -> paradigm dangling references.
        let paradigm_ids: BTreeSet<&'static str> = ctx
            .paradigm_registry
            .iter()
            .map(|p| p.id().as_str())
            .collect();
        for rule in ctx.rule_registry.iter() {
            let paradigm = rule.paradigm().as_str();
            if !paradigm_ids.contains(paradigm) {
                let rule_id = rule.id().as_str();
                let f = self.build_finding(
                    ctx,
                    format!(
                        "registered rule `{rule_id}` declares paradigm `{paradigm}`, \
                         which is not in `ParadigmRegistry::standard()`"
                    ),
                    Vec::new(),
                    Some(format!(
                        "register a `ParadigmDefinition` for `{paradigm}`, or change \
                         `{rule_id}.paradigm()` to a registered paradigm id"
                    )),
                );
                let d = self.build_decision(
                    f.id,
                    vec![format!("rule `{rule_id}` -> unknown paradigm `{paradigm}`")],
                );
                new_findings.push(f);
                decisions.push(d);
            }
        }

        // (5) Paradigm -> rule dangling references.
        let rule_ids: BTreeSet<&'static str> =
            ctx.rule_registry.iter().map(|r| r.id().as_str()).collect();
        for paradigm in ctx.paradigm_registry.iter() {
            let paradigm_id = paradigm.id().as_str();
            for rule in paradigm.rules() {
                let rule_id = rule.id().as_str();
                if !rule_ids.contains(rule_id) {
                    let f = self.build_finding(
                        ctx,
                        format!(
                            "paradigm `{paradigm_id}` lists rule `{rule_id}` in its \
                             `rules()` slice, but `{rule_id}` is not in \
                             `RuleRegistry::standard()`"
                        ),
                        Vec::new(),
                        Some(format!(
                            "register `{rule_id}` in `RuleRegistry::standard()`, or \
                             remove it from `{paradigm_id}.rules()`"
                        )),
                    );
                    let d = self.build_decision(
                        f.id,
                        vec![format!(
                            "paradigm `{paradigm_id}` -> unregistered rule `{rule_id}`"
                        )],
                    );
                    new_findings.push(f);
                    decisions.push(d);
                }
            }
        }

        PolicyOutput {
            new_findings,
            decisions,
        }
    }
}

impl RegistryCoherencePolicy {
    fn build_finding(
        &self,
        ctx: &PolicyContext<'_>,
        message: String,
        why: Vec<String>,
        suggested_fix: Option<String>,
    ) -> RuleFinding {
        RuleFinding {
            id: ctx.finding_ids.next(),
            source: FindingSource::Policy(REGISTRY_COHERENCE_ID),
            rule_id: None,
            paradigm_id: None,
            default_severity: Severity::Advisory,
            span: None,
            concept: None,
            message,
            evidence: Vec::new(),
            why,
            suggested_fix,
            diagnostic_code: Some(LOCUS004.into()),
        }
    }

    fn build_decision(
        &self,
        finding_id: crate::governance::ids::FindingId,
        rationale: Vec<String>,
    ) -> Decision {
        Decision {
            finding_id,
            policy: REGISTRY_COHERENCE_ID,
            severity: Severity::Advisory,
            status: DecisionStatus::KnownTransitionDebt,
            severity_change: SeverityChange::Unchanged,
            rationale,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{CheckMode, Severity};
    use crate::governance::arch::{ArchDeclaration, ArchLoadOutcome};
    use crate::governance::decision::DecisionStatus;
    use crate::governance::finding::FindingStore;
    use crate::governance::ids::{FindingIdMinter, ParadigmId, RuleId};
    use crate::governance::paradigm::ParadigmDefinition;
    use crate::governance::policy::PolicyContext;
    use crate::governance::registry::{ParadigmRegistry, PolicyRegistry, RuleRegistry};
    use crate::governance::rule::{RuleContext, RuleDefinition};
    use crate::lockfile::Lockfile;
    use locus_air::AirWorkspace;

    struct StubRule(&'static str, &'static str);
    impl RuleDefinition for StubRule {
        fn id(&self) -> RuleId {
            RuleId::new(self.0)
        }
        fn paradigm(&self) -> ParadigmId {
            ParadigmId::new(self.1)
        }
        fn title(&self) -> &'static str {
            "stub"
        }
        fn default_severity(&self) -> Severity {
            Severity::Warning
        }
        fn observe(&self, _: &RuleContext<'_>) -> Vec<RuleFinding> {
            Vec::new()
        }
    }

    struct StubParadigm {
        id: &'static str,
        rules: &'static [&'static dyn RuleDefinition],
    }
    impl ParadigmDefinition for StubParadigm {
        fn id(&self) -> ParadigmId {
            ParadigmId::new(self.id)
        }
        fn title(&self) -> &'static str {
            "stub"
        }
        fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
            self.rules
        }
    }

    fn run_policy_with(
        arch: &ArchLoadOutcome,
        rules: &RuleRegistry,
        paradigms: &ParadigmRegistry,
        policies: &PolicyRegistry,
        mode: CheckMode,
    ) -> PolicyOutput {
        let air = AirWorkspace::new(Vec::new());
        let lf = Lockfile::empty();
        let store = FindingStore::new();
        let minter = FindingIdMinter::new();
        let ctx = PolicyContext {
            air: &air,
            lockfile: &lf,
            mode,
            rule_registry: rules,
            paradigm_registry: paradigms,
            policy_registry: policies,
            findings: &store,
            prior_decisions: &[],
            finding_ids: &minter,
            arch,
        };
        RegistryCoherencePolicy.decide(&ctx)
    }

    #[test]
    fn silent_when_arch_declares_exactly_registered_policies() {
        let arch = ArchLoadOutcome::Present(ArchDeclaration {
            policies: vec![
                "registry-integrity".into(),
                "registry-coherence".into(),
                "concept-source-of-truth".into(),
                "default-pass-through".into(),
            ],
            concepts: Vec::new(),
        });
        let rules = RuleRegistry::standard();
        let paradigms = ParadigmRegistry::standard();
        let policies = PolicyRegistry::standard();
        let out = run_policy_with(&arch, &rules, &paradigms, &policies, CheckMode::Human);
        assert!(
            out.new_findings.is_empty(),
            "expected silence; got findings: {:?}",
            out.new_findings
                .iter()
                .map(|f| f.message.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn emits_locus004_when_arch_missing() {
        let arch = ArchLoadOutcome::Missing;
        let rules = RuleRegistry::with_rules(Vec::new());
        let paradigms = ParadigmRegistry::empty();
        let policies = PolicyRegistry::with_policies(Vec::new());
        let out = run_policy_with(&arch, &rules, &paradigms, &policies, CheckMode::Human);
        assert_eq!(out.new_findings.len(), 1);
        let f = &out.new_findings[0];
        assert_eq!(f.diagnostic_code.as_deref(), Some("LOCUS004"));
        assert_eq!(f.default_severity, Severity::Advisory);
        assert!(f.message.contains(".locus/arch.json"));
    }

    #[test]
    fn emits_locus004_when_arch_invalid_with_error_in_why() {
        let arch = ArchLoadOutcome::Invalid("expected `,` at line 1".into());
        let rules = RuleRegistry::with_rules(Vec::new());
        let paradigms = ParadigmRegistry::empty();
        let policies = PolicyRegistry::with_policies(Vec::new());
        let out = run_policy_with(&arch, &rules, &paradigms, &policies, CheckMode::Human);
        assert_eq!(out.new_findings.len(), 1);
        let f = &out.new_findings[0];
        assert_eq!(f.diagnostic_code.as_deref(), Some("LOCUS004"));
        assert!(
            f.why.iter().any(|w| w.contains("expected `,` at line 1")),
            "parse error should appear in why[]; got {:?}",
            f.why
        );
    }

    #[test]
    fn emits_one_locus004_per_declared_but_unregistered_policy() {
        let arch = ArchLoadOutcome::Present(ArchDeclaration {
            policies: vec!["nonexistent-policy".into(), "another-ghost".into()],
            concepts: Vec::new(),
        });
        let rules = RuleRegistry::with_rules(Vec::new());
        let paradigms = ParadigmRegistry::empty();
        let policies = PolicyRegistry::with_policies(Vec::new());
        let out = run_policy_with(&arch, &rules, &paradigms, &policies, CheckMode::Human);
        let drift: Vec<&str> = out
            .new_findings
            .iter()
            .map(|f| f.message.as_str())
            .collect();
        assert!(
            drift
                .iter()
                .any(|m| m.contains("nonexistent-policy") && m.contains("not registered")),
            "missing finding for nonexistent-policy; got {drift:?}"
        );
        assert!(
            drift
                .iter()
                .any(|m| m.contains("another-ghost") && m.contains("not registered")),
            "missing finding for another-ghost; got {drift:?}"
        );
    }

    #[test]
    fn emits_one_locus004_per_registered_but_undeclared_policy() {
        let arch = ArchLoadOutcome::Present(ArchDeclaration {
            policies: vec![],
            concepts: Vec::new(),
        });
        let rules = RuleRegistry::with_rules(Vec::new());
        let paradigms = ParadigmRegistry::empty();
        let policies = PolicyRegistry::standard();
        let out = run_policy_with(&arch, &rules, &paradigms, &policies, CheckMode::Human);
        let drift: Vec<&str> = out
            .new_findings
            .iter()
            .map(|f| f.message.as_str())
            .collect();
        for expected in [
            "registry-integrity",
            "registry-coherence",
            "concept-source-of-truth",
            "default-pass-through",
        ] {
            assert!(
                drift
                    .iter()
                    .any(|m| m.contains(expected) && m.contains("not declared")),
                "missing finding for registered `{expected}`; got {drift:?}"
            );
        }
    }

    static R_GHOST: StubRule = StubRule("ZZ001", "ZZ");

    #[test]
    fn emits_locus004_when_rule_paradigm_not_resolvable() {
        // RuleRegistry::with_rules validates that rule.id starts with
        // paradigm prefix — ZZ001 / ZZ satisfies that, so the registry
        // accepts the rule. But ParadigmRegistry::empty() has no ZZ
        // ParadigmDefinition, so the coherence policy must flag drift.
        let arch = ArchLoadOutcome::Present(ArchDeclaration {
            policies: vec![],
            concepts: Vec::new(),
        });
        let rules = RuleRegistry::with_rules(vec![&R_GHOST]);
        let paradigms = ParadigmRegistry::empty();
        let policies = PolicyRegistry::with_policies(Vec::new());
        let out = run_policy_with(&arch, &rules, &paradigms, &policies, CheckMode::Human);
        assert!(
            out.new_findings
                .iter()
                .any(|f| f.message.contains("ZZ001") && f.message.contains("ZZ")),
            "expected finding mentioning rule ZZ001 / paradigm ZZ; got {:?}",
            out.new_findings
                .iter()
                .map(|f| f.message.as_str())
                .collect::<Vec<_>>()
        );
    }

    static R_DANGLING_FROM_PARADIGM: StubRule = StubRule("YY001", "YY");
    static PARADIGM_WITH_DANGLING_RULE: StubParadigm = StubParadigm {
        id: "YY",
        rules: &[&R_DANGLING_FROM_PARADIGM],
    };

    #[test]
    fn emits_locus004_when_paradigm_references_unregistered_rule() {
        // The YY ParadigmDefinition references rule YY001, but YY001 is NOT
        // in the rule registry — this is the inverse drift direction.
        let arch = ArchLoadOutcome::Present(ArchDeclaration {
            policies: vec![],
            concepts: Vec::new(),
        });
        let rules = RuleRegistry::with_rules(Vec::new());
        let paradigms = ParadigmRegistry::with_paradigms(vec![&PARADIGM_WITH_DANGLING_RULE]);
        let policies = PolicyRegistry::with_policies(Vec::new());
        let out = run_policy_with(&arch, &rules, &paradigms, &policies, CheckMode::Human);

        let drift: Vec<&str> = out
            .new_findings
            .iter()
            .filter(|f| {
                f.message.contains("YY001")
                    && f.message.contains("YY")
                    && f.message.contains("not in `RuleRegistry::standard()`")
            })
            .map(|f| f.message.as_str())
            .collect();
        assert_eq!(
            drift.len(),
            1,
            "expected exactly one paradigm->rule drift finding for YY/YY001; got {:?}",
            out.new_findings
                .iter()
                .map(|f| f.message.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn locus004_advisory_stays_advisory_under_agent_strict() {
        let arch = ArchLoadOutcome::Missing;
        let rules = RuleRegistry::with_rules(Vec::new());
        let paradigms = ParadigmRegistry::empty();
        let policies = PolicyRegistry::with_policies(Vec::new());
        let out = run_policy_with(&arch, &rules, &paradigms, &policies, CheckMode::AgentStrict);
        for f in &out.new_findings {
            assert_eq!(f.default_severity, Severity::Advisory);
        }
        for d in &out.decisions {
            assert_eq!(d.severity, Severity::Advisory);
            assert_eq!(d.status, DecisionStatus::KnownTransitionDebt);
        }
    }

    #[test]
    fn decisions_use_known_transition_debt_status() {
        let arch = ArchLoadOutcome::Missing;
        let rules = RuleRegistry::with_rules(Vec::new());
        let paradigms = ParadigmRegistry::empty();
        let policies = PolicyRegistry::with_policies(Vec::new());
        let out = run_policy_with(&arch, &rules, &paradigms, &policies, CheckMode::Human);
        for d in &out.decisions {
            assert_eq!(d.status, DecisionStatus::KnownTransitionDebt);
        }
    }
}
