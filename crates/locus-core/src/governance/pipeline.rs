//! Governance pipeline: rules + legacy adapter → findings → policies →
//! decisions → diagnostics.

// locus: ot canonical

use crate::diagnostics::{CheckMode, Diagnostic};
use crate::governance::decision::{Decision, DecisionStatus};
use crate::governance::finding::{FindingSource, FindingStore, RuleFinding};
use crate::governance::ids::FindingIdMinter;
use crate::governance::legacy::LegacyParadigmRuleAdapter;
use crate::governance::policy::PolicyContext;
use crate::governance::registry::{
    GovernanceDiagnosticRegistry, ParadigmRegistry, PolicyRegistry, RuleRegistry,
    validate_decisions,
};
use crate::governance::rule::RuleContext;
use crate::lockfile::Lockfile;
use crate::paradigms;
use locus_air::{AirSpan, AirWorkspace};

pub struct GovernanceOutput {
    pub diagnostics: Vec<Diagnostic>,
    pub decisions: Vec<Decision>,
    pub findings: FindingStore,
}

pub fn run(air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> GovernanceOutput {
    let rules = RuleRegistry::standard();
    let paradigms_reg = ParadigmRegistry::standard();
    let policies = PolicyRegistry::standard();
    let governance_codes = GovernanceDiagnosticRegistry::standard();
    let minter = FindingIdMinter::new();
    let mut store = FindingStore::new();

    // Phase A — migrated rules observe.
    let rule_ctx = RuleContext {
        air,
        lockfile,
        mode,
        rule_registry: &rules,
        paradigm_registry: &paradigms_reg,
        finding_ids: &minter,
    };
    for rule in rules.iter() {
        for f in rule.observe(&rule_ctx) {
            store.insert(f);
        }
    }

    // Phase B — legacy adapter (per-diagnostic-code filter).
    let legacy = paradigms::registry();
    LegacyParadigmRuleAdapter::run(&legacy, air, lockfile, mode, &rules, &minter, &mut store);

    // Phase C — policies in registry order. Single pass.
    let mut decisions: Vec<Decision> = Vec::new();
    for policy in policies.iter() {
        let pctx = PolicyContext {
            air,
            lockfile,
            mode,
            rule_registry: &rules,
            paradigm_registry: &paradigms_reg,
            policy_registry: &policies,
            findings: &store,
            prior_decisions: &decisions,
            finding_ids: &minter,
        };
        let out = policy.decide(&pctx);
        for f in out.new_findings {
            store.insert(f);
        }
        decisions.extend(out.decisions);
    }

    validate_decisions(&decisions, &store).expect("policy chain produced invalid decisions");

    // Phase D — materialize.
    let diagnostics: Vec<Diagnostic> = decisions
        .iter()
        .filter_map(|d| materialize(d, &store, &governance_codes))
        .collect();

    GovernanceOutput {
        diagnostics,
        decisions,
        findings: store,
    }
}

fn materialize(
    decision: &Decision,
    store: &FindingStore,
    governance_codes: &GovernanceDiagnosticRegistry,
) -> Option<Diagnostic> {
    if matches!(
        decision.status,
        DecisionStatus::SuppressedByPolicy | DecisionStatus::AcceptedException
    ) {
        return None;
    }
    let f = store.get(decision.finding_id)?;
    let mut why = f.why.clone();
    why.extend(decision.rationale.iter().cloned());
    Some(Diagnostic {
        rule_id: emitted_rule_code(f, governance_codes),
        severity: decision.severity,
        span: f.span.clone().unwrap_or_else(synthetic_governance_span),
        concept: f.concept.clone(),
        message: f.message.clone(),
        why,
        suggested_fix: f.suggested_fix.clone(),
    })
}

fn emitted_rule_code(f: &RuleFinding, governance_codes: &GovernanceDiagnosticRegistry) -> String {
    // Resolution order — see spec §"Pipeline":
    //   1. explicit governance/policy diagnostic code (e.g. LOCUS003)
    //   2. registered rule id
    //   3. legacy diagnostic's verbatim rule_code
    //   4. RegisteredRule source as last resort
    // PolicyId is NEVER displayed as a user-facing code.
    if let Some(code) = f.diagnostic_code.as_deref() {
        debug_assert!(
            governance_codes.contains(code),
            "RuleFinding.diagnostic_code {code} is not registered in GovernanceDiagnosticRegistry"
        );
        return code.to_string();
    }
    match (&f.rule_id, &f.source) {
        (Some(r), _) => r.as_str().to_string(),
        (None, FindingSource::LegacyDiagnostic { rule_code, .. }) => rule_code.clone(),
        (None, FindingSource::RegisteredRule(r)) => r.as_str().to_string(),
        (None, FindingSource::Policy(p)) => {
            // Policy findings MUST carry diagnostic_code. Reaching this is
            // an internal error caught by RegistryIntegrityPolicy in P3.
            panic!(
                "policy finding from {} missing diagnostic_code (id={})",
                p.as_str(),
                f.id.as_u64()
            );
        }
    }
}

fn synthetic_governance_span() -> AirSpan {
    AirSpan::new("<governance>", 0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_span_uses_governance_sentinel() {
        let s = synthetic_governance_span();
        assert_eq!(s.file, "<governance>");
        assert_eq!(s.line_start, 0);
    }

    #[test]
    fn empty_workspace_round_trips_findings_to_diagnostics() {
        // Legacy paradigms may still emit diagnostics from defaults (e.g.
        // LOCUS002 vacancy nudges). Don't assert empty — assert every
        // emitted diagnostic survives Phase A→D round-trip, i.e. the count
        // matches non-suppressed decisions.
        let air = AirWorkspace::new(Vec::new());
        let lf = Lockfile::empty();
        let out = run(&air, &lf, CheckMode::Human);

        let non_suppressed = out
            .decisions
            .iter()
            .filter(|d| {
                !matches!(
                    d.status,
                    DecisionStatus::SuppressedByPolicy | DecisionStatus::AcceptedException
                )
            })
            .count();
        assert_eq!(out.diagnostics.len(), non_suppressed);
    }
}
