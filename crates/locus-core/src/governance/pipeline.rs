//! Governance pipeline: rules + legacy adapter → findings → policies →
//! decisions → diagnostics.

// locus: ot canonical

use std::path::Path;

use crate::diagnostics::{CheckMode, Diagnostic};
use crate::governance::arch::{ArchDeclaration, ArchLoadOutcome};
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

/// Run the governance pipeline without a workspace root. The architecture
/// declaration is treated as `Missing`, so `RegistryCoherencePolicy` will
/// emit a single LOCUS004 advisory pointing at the absent `.locus/arch.json`.
/// Production code paths should prefer `run_with_arch`.
pub fn run(air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> GovernanceOutput {
    run_with_arch(air, lockfile, mode, &ArchLoadOutcome::Missing)
}

/// Convenience overload: load `.locus/arch.json` from `workspace_root`
/// before running. Used by the CLI; tests typically prefer the explicit
/// `run_with_arch` form so they can inject a deterministic outcome.
pub fn run_with_workspace_root(
    air: &AirWorkspace,
    lockfile: &Lockfile,
    mode: CheckMode,
    workspace_root: &Path,
) -> GovernanceOutput {
    let arch = ArchDeclaration::load(workspace_root);
    run_with_arch(air, lockfile, mode, &arch)
}

pub fn run_with_arch(
    air: &AirWorkspace,
    lockfile: &Lockfile,
    mode: CheckMode,
    arch: &ArchLoadOutcome,
) -> GovernanceOutput {
    let rules = RuleRegistry::standard();
    let paradigms_reg = ParadigmRegistry::standard();
    let policies = PolicyRegistry::standard();
    let governance_codes = GovernanceDiagnosticRegistry::standard();
    let minter = FindingIdMinter::new();
    let mut store = FindingStore::new();

    observe_rules(
        &rules,
        &paradigms_reg,
        air,
        lockfile,
        mode,
        &minter,
        &mut store,
    );
    run_legacy_adapter(air, lockfile, mode, &rules, &minter, &mut store);
    let decisions = run_policies(
        &policies,
        &rules,
        &paradigms_reg,
        air,
        lockfile,
        mode,
        arch,
        &minter,
        &mut store,
    );

    validate_decisions(&decisions, &store).expect("policy chain produced invalid decisions");

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

/// Phase A — migrated rules observe.
fn observe_rules(
    rules: &RuleRegistry,
    paradigms_reg: &ParadigmRegistry,
    air: &AirWorkspace,
    lockfile: &Lockfile,
    mode: CheckMode,
    minter: &FindingIdMinter,
    store: &mut FindingStore,
) {
    let rule_ctx = RuleContext {
        air,
        lockfile,
        mode,
        rule_registry: rules,
        paradigm_registry: paradigms_reg,
        finding_ids: minter,
    };
    for rule in rules.iter() {
        for f in rule.observe(&rule_ctx) {
            store.insert(f);
        }
    }
}

/// Phase B — legacy adapter (per-diagnostic-code filter).
fn run_legacy_adapter(
    air: &AirWorkspace,
    lockfile: &Lockfile,
    mode: CheckMode,
    rules: &RuleRegistry,
    minter: &FindingIdMinter,
    store: &mut FindingStore,
) {
    let legacy = paradigms::registry();
    LegacyParadigmRuleAdapter::run(&legacy, air, lockfile, mode, rules, minter, store);
}

/// Phase C — policies in registry order. Single pass.
#[allow(clippy::too_many_arguments)]
fn run_policies(
    policies: &PolicyRegistry,
    rules: &RuleRegistry,
    paradigms_reg: &ParadigmRegistry,
    air: &AirWorkspace,
    lockfile: &Lockfile,
    mode: CheckMode,
    arch: &ArchLoadOutcome,
    minter: &FindingIdMinter,
    store: &mut FindingStore,
) -> Vec<Decision> {
    let mut decisions: Vec<Decision> = Vec::new();
    for policy in policies.iter() {
        let pctx = PolicyContext {
            air,
            lockfile,
            mode,
            rule_registry: rules,
            paradigm_registry: paradigms_reg,
            policy_registry: policies,
            findings: store,
            prior_decisions: &decisions,
            finding_ids: minter,
            arch,
        };
        let out = policy.decide(&pctx);
        for f in out.new_findings {
            store.insert(f);
        }
        decisions.extend(out.decisions);
    }
    decisions
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
