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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{CheckMode, Severity};
    use crate::governance::decision::DecisionStatus;
    use crate::governance::finding::{FindingSource, FindingStore, RuleFinding};
    use crate::governance::ids::{FindingId, FindingIdMinter, ParadigmId, RuleId};
    use crate::governance::policy::PolicyContext;
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
        assert_eq!(
            out.new_findings.len(),
            2,
            "two distinct codes → two LOCUS003"
        );
    }

    #[test]
    fn mixed_registered_and_legacy_only_legacy_gets_locus003() {
        let mut store = FindingStore::new();
        store.insert(registered_finding(0, "CX001"));
        store.insert(legacy_finding(1, "CX002"));
        let out = run_policy(store);
        assert_eq!(out.new_findings.len(), 1);
        assert!(out.new_findings[0].message.contains("CX002"));
    }

    #[test]
    fn decisions_use_known_transition_debt_status() {
        let mut store = FindingStore::new();
        store.insert(legacy_finding(0, "MO002"));
        let out = run_policy(store);
        assert_eq!(out.decisions.len(), 1);
        let d = &out.decisions[0];
        assert_eq!(d.status, DecisionStatus::KnownTransitionDebt);
        assert_eq!(d.severity, Severity::Advisory);
        assert_eq!(
            d.finding_id, out.new_findings[0].id,
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
