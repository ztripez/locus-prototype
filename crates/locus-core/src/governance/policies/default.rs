//! `DefaultPassThroughPolicy` — the strangler's compatibility bridge.
//!
//! Runs LAST in `PolicyRegistry::standard()`. Emits exactly one decision
//! for every finding not already decided by an earlier policy. Severity
//! is the finding's `default_severity`; status is `Active` (or `Advisory`
//! if the default is `Severity::Advisory`); no rationale is appended so
//! legacy findings materialize byte-identically to their original
//! `Diagnostic`s.

// locus: ot canonical

use crate::diagnostics::Severity;
use crate::governance::decision::{Decision, DecisionStatus, SeverityChange};
use crate::governance::ids::{FindingId, PolicyId};
use crate::governance::policy::{PolicyContext, PolicyDefinition, PolicyOutput};
use std::collections::HashSet;

pub struct DefaultPassThroughPolicy;

pub const DEFAULT_PASS_THROUGH: PolicyId = PolicyId::new("default-pass-through");

impl PolicyDefinition for DefaultPassThroughPolicy {
    fn id(&self) -> PolicyId {
        DEFAULT_PASS_THROUGH
    }

    fn title(&self) -> &'static str {
        "Default Pass-Through"
    }

    fn decide(&self, ctx: &PolicyContext<'_>) -> PolicyOutput {
        let decided: HashSet<FindingId> =
            ctx.prior_decisions.iter().map(|d| d.finding_id).collect();

        let decisions: Vec<Decision> = ctx
            .findings
            .iter()
            .filter(|f| !decided.contains(&f.id))
            .map(|f| Decision {
                finding_id: f.id,
                policy: DEFAULT_PASS_THROUGH,
                severity: f.default_severity,
                status: match f.default_severity {
                    Severity::Advisory => DecisionStatus::Advisory,
                    _ => DecisionStatus::Active,
                },
                severity_change: SeverityChange::Unchanged,
                rationale: Vec::new(),
            })
            .collect();

        PolicyOutput {
            decisions,
            new_findings: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::CheckMode;
    use crate::governance::finding::{FindingSource, FindingStore, RuleFinding};
    use crate::governance::ids::{FindingIdMinter, ParadigmId, RuleId};
    use crate::governance::registry::{ParadigmRegistry, PolicyRegistry, RuleRegistry};
    use crate::lockfile::Lockfile;
    use locus_air::AirWorkspace;

    fn make_finding(id_raw: u64, sev: Severity) -> RuleFinding {
        RuleFinding {
            id: FindingId::from_raw_for_test(id_raw),
            source: FindingSource::LegacyDiagnostic {
                rule_code: "CX001".into(),
                paradigm: Some(ParadigmId::new("CX")),
            },
            rule_id: None,
            paradigm_id: Some(ParadigmId::new("CX")),
            default_severity: sev,
            span: None,
            concept: None,
            message: "msg".into(),
            evidence: Vec::new(),
            why: Vec::new(),
            suggested_fix: None,
            diagnostic_code: None,
        }
    }

    #[test]
    fn decides_every_undecided_finding() {
        let air = AirWorkspace::new(Vec::new());
        let lf = Lockfile::empty();
        let rules = RuleRegistry::standard();
        let paradigms = ParadigmRegistry::empty();
        let policies = PolicyRegistry::with_policies(Vec::new());
        let minter = FindingIdMinter::new();

        let mut store = FindingStore::new();
        store.insert(make_finding(0, Severity::Warning));
        store.insert(make_finding(1, Severity::Advisory));
        store.insert(make_finding(2, Severity::Fatal));

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

        let out = DefaultPassThroughPolicy.decide(&ctx);
        assert_eq!(out.decisions.len(), 3);
        assert!(out.new_findings.is_empty());

        let by_id: std::collections::HashMap<_, _> =
            out.decisions.iter().map(|d| (d.finding_id, d)).collect();

        assert_eq!(
            by_id[&FindingId::from_raw_for_test(0)].status,
            DecisionStatus::Active
        );
        assert_eq!(
            by_id[&FindingId::from_raw_for_test(1)].status,
            DecisionStatus::Advisory
        );
        assert_eq!(
            by_id[&FindingId::from_raw_for_test(2)].status,
            DecisionStatus::Active
        );

        for d in &out.decisions {
            assert_eq!(d.severity_change, SeverityChange::Unchanged);
            assert!(
                d.rationale.is_empty(),
                "legacy findings must not get rationale lines"
            );
        }
    }

    #[test]
    fn skips_already_decided_findings() {
        let air = AirWorkspace::new(Vec::new());
        let lf = Lockfile::empty();
        let rules = RuleRegistry::standard();
        let paradigms = ParadigmRegistry::empty();
        let policies = PolicyRegistry::with_policies(Vec::new());
        let minter = FindingIdMinter::new();

        let mut store = FindingStore::new();
        store.insert(make_finding(0, Severity::Warning));
        store.insert(make_finding(1, Severity::Warning));

        let prior = vec![Decision {
            finding_id: FindingId::from_raw_for_test(0),
            policy: PolicyId::new("some-earlier-policy"),
            severity: Severity::Advisory,
            status: DecisionStatus::Advisory,
            severity_change: SeverityChange::Downgraded { from: Severity::Warning },
            rationale: vec!["downgraded".into()],
        }];

        let ctx = PolicyContext {
            air: &air,
            lockfile: &lf,
            mode: CheckMode::Human,
            rule_registry: &rules,
            paradigm_registry: &paradigms,
            policy_registry: &policies,
            findings: &store,
            prior_decisions: &prior,
            finding_ids: &minter,
        };

        let out = DefaultPassThroughPolicy.decide(&ctx);
        assert_eq!(out.decisions.len(), 1);
        assert_eq!(out.decisions[0].finding_id, FindingId::from_raw_for_test(1));
    }

    // Used in pipeline tests too — keep _ silenced.
    #[allow(dead_code)]
    fn _suppress_unused() {
        let _ = RuleId::new("CX001");
    }
}
