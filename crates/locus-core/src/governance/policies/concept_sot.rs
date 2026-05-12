//! `ConceptSourceOfTruthPolicy` — architecture-intent enforcement.
//!
//! Reads concept declarations from `.locus/arch.json`. For each declared
//! concept, checks whether observed runtime state (findings + their
//! registry references) is consistent with the declared source of truth.
//! Emits one LOCUS005 advisory per bypass.
//!
//! Coexists with `RegistryIntegrityPolicy` (LOCUS003). LOCUS003 tracks
//! migration debt for legacy diagnostics; LOCUS005 tracks declared
//! architecture-intent violations. Legacy diagnostics are LOCUS003's
//! territory only — LOCUS005 explicitly skips them.

// locus: ot canonical

use std::collections::BTreeSet;

use crate::diagnostics::Severity;
use crate::governance::arch::{ArchLoadOutcome, ConceptDeclaration};
use crate::governance::decision::{Decision, DecisionStatus, SeverityChange};
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::PolicyId;
use crate::governance::policy::{PolicyContext, PolicyDefinition, PolicyOutput};
use crate::governance::registry::GovernanceDiagnosticRegistry;

pub struct ConceptSourceOfTruthPolicy;

pub const CONCEPT_SOT_ID: PolicyId = PolicyId::new("concept-source-of-truth");

const LOCUS005: &str = "LOCUS005";

impl PolicyDefinition for ConceptSourceOfTruthPolicy {
    fn id(&self) -> PolicyId {
        CONCEPT_SOT_ID
    }

    fn title(&self) -> &'static str {
        "Concept Source-of-Truth"
    }

    fn decide(&self, ctx: &PolicyContext<'_>) -> PolicyOutput {
        // No declarations → policy silent.
        let ArchLoadOutcome::Present(decl) = ctx.arch else {
            return PolicyOutput::empty();
        };
        if decl.concepts.is_empty() {
            return PolicyOutput::empty();
        }

        let mut new_findings = Vec::new();
        let mut decisions = Vec::new();
        // (concept_id, observed_identifier) seen this run — dedupe key.
        let mut seen: BTreeSet<(String, String)> = BTreeSet::new();

        for concept in &decl.concepts {
            match concept.id.as_str() {
                "rule" => {
                    check_rule_concept(concept, ctx, &mut seen, &mut new_findings, &mut decisions)
                }
                "paradigm" => check_paradigm_concept(
                    concept,
                    ctx,
                    &mut seen,
                    &mut new_findings,
                    &mut decisions,
                ),
                "policy" => {
                    check_policy_concept(concept, ctx, &mut seen, &mut new_findings, &mut decisions)
                }
                "governance-code" => check_governance_code_concept(
                    concept,
                    ctx,
                    &mut seen,
                    &mut new_findings,
                    &mut decisions,
                ),
                _ => {
                    push_unknown_concept(concept, ctx, &mut seen, &mut new_findings, &mut decisions)
                }
            }
        }

        PolicyOutput {
            new_findings,
            decisions,
        }
    }
}

fn check_rule_concept(
    concept: &ConceptDeclaration,
    ctx: &PolicyContext<'_>,
    seen: &mut BTreeSet<(String, String)>,
    new_findings: &mut Vec<RuleFinding>,
    decisions: &mut Vec<Decision>,
) {
    for f in ctx.findings.iter() {
        // CRITICAL: legacy diagnostics are LOCUS003's territory. LOCUS005
        // skips them by construction so the new policy never duplicates
        // migration-debt signal.
        if matches!(f.source, FindingSource::LegacyDiagnostic { .. }) {
            continue;
        }
        let FindingSource::RegisteredRule(rule_id) = &f.source else {
            continue;
        };
        if ctx.rule_registry.find(rule_id).is_some() {
            continue;
        }
        let identifier = rule_id.as_str().to_string();
        emit_bypass(
            concept,
            "rule",
            &identifier,
            format!(
                "Observed rule `{identifier}` was emitted without a registered \
                 `RuleDefinition`."
            ),
            format!(
                "Create a RuleDefinition for `{identifier}`, register it in \
                 `RuleRegistry::standard()`, and add it to the owning \
                 `ParadigmDefinition::rules()` slice."
            ),
            ctx,
            seen,
            new_findings,
            decisions,
        );
    }
}

fn check_paradigm_concept(
    concept: &ConceptDeclaration,
    ctx: &PolicyContext<'_>,
    seen: &mut BTreeSet<(String, String)>,
    new_findings: &mut Vec<RuleFinding>,
    decisions: &mut Vec<Decision>,
) {
    for f in ctx.findings.iter() {
        if matches!(f.source, FindingSource::LegacyDiagnostic { .. }) {
            continue;
        }
        let Some(pid) = f.paradigm_id else { continue };
        if ctx.paradigm_registry.find(&pid).is_some() {
            continue;
        }
        let identifier = pid.as_str().to_string();
        emit_bypass(
            concept,
            "paradigm",
            &identifier,
            format!(
                "Observed paradigm `{identifier}` was referenced without a \
                 registered `ParadigmDefinition`."
            ),
            format!(
                "Create a `ParadigmDefinition` for `{identifier}` and \
                 register it in `ParadigmRegistry::standard()`."
            ),
            ctx,
            seen,
            new_findings,
            decisions,
        );
    }
}

fn check_policy_concept(
    concept: &ConceptDeclaration,
    ctx: &PolicyContext<'_>,
    seen: &mut BTreeSet<(String, String)>,
    new_findings: &mut Vec<RuleFinding>,
    decisions: &mut Vec<Decision>,
) {
    for f in ctx.findings.iter() {
        if matches!(f.source, FindingSource::LegacyDiagnostic { .. }) {
            continue;
        }
        let FindingSource::Policy(policy_id) = &f.source else {
            continue;
        };
        if ctx.policy_registry.find(policy_id).is_some() {
            continue;
        }
        let identifier = policy_id.as_str().to_string();
        emit_bypass(
            concept,
            "policy",
            &identifier,
            format!(
                "Observed policy `{identifier}` emitted a finding but is not \
                 registered in `PolicyRegistry::standard()`."
            ),
            format!(
                "Register a `PolicyDefinition` for `{identifier}` in \
                 `PolicyRegistry::standard()`."
            ),
            ctx,
            seen,
            new_findings,
            decisions,
        );
    }
}

fn check_governance_code_concept(
    concept: &ConceptDeclaration,
    ctx: &PolicyContext<'_>,
    seen: &mut BTreeSet<(String, String)>,
    new_findings: &mut Vec<RuleFinding>,
    decisions: &mut Vec<Decision>,
) {
    let governance_codes = GovernanceDiagnosticRegistry::standard();
    for f in ctx.findings.iter() {
        if matches!(f.source, FindingSource::LegacyDiagnostic { .. }) {
            continue;
        }
        let Some(code) = f.diagnostic_code.as_deref() else {
            continue;
        };
        if !is_governance_code_shaped(code) {
            continue;
        }
        if governance_codes.contains(code) {
            continue;
        }
        let identifier = code.to_string();
        emit_bypass(
            concept,
            "governance-code",
            &identifier,
            format!(
                "Observed governance code `{identifier}` is not registered in \
                 `GovernanceDiagnosticRegistry::standard()`."
            ),
            format!(
                "Register `{identifier}` in \
                 `GovernanceDiagnosticRegistry::standard()` with the owning \
                 `PolicyId`, or change the emitter to use a registered code."
            ),
            ctx,
            seen,
            new_findings,
            decisions,
        );
    }
}

fn push_unknown_concept(
    concept: &ConceptDeclaration,
    ctx: &PolicyContext<'_>,
    seen: &mut BTreeSet<(String, String)>,
    new_findings: &mut Vec<RuleFinding>,
    decisions: &mut Vec<Decision>,
) {
    let key = (concept.id.clone(), String::from("<declaration>"));
    if !seen.insert(key) {
        return;
    }
    let observation_line = format!(
        "Concept id `{}` is not recognised by `ConceptSourceOfTruthPolicy`; \
         declaration will not be enforced.",
        concept.id
    );
    let suggested_fix = format!(
        "Use one of the supported concept ids (`rule`, `paradigm`, `policy`, \
         `governance-code`), or remove the `{}` entry from `.locus/arch.json`.",
        concept.id
    );
    let message = format!(
        "concept `{}` is declared but not understood by the source-of-truth policy",
        concept.id
    );
    let rationale = format!("unknown concept id `{}`", concept.id);
    push_locus005(
        concept,
        ctx,
        message,
        observation_line,
        suggested_fix,
        rationale,
        new_findings,
        decisions,
    );
}

#[allow(clippy::too_many_arguments)]
fn emit_bypass(
    concept: &ConceptDeclaration,
    kind_label: &str,
    identifier: &str,
    observation_line: String,
    suggested_fix: String,
    ctx: &PolicyContext<'_>,
    seen: &mut BTreeSet<(String, String)>,
    new_findings: &mut Vec<RuleFinding>,
    decisions: &mut Vec<Decision>,
) {
    let key = (concept.id.clone(), identifier.to_string());
    if !seen.insert(key) {
        return;
    }
    let message = format!("concept `{}` bypasses declared source of truth", concept.id);
    let rationale = format!(
        "{kind_label} `{identifier}` bypasses `{}`",
        concept.registry
    );
    push_locus005(
        concept,
        ctx,
        message,
        observation_line,
        suggested_fix,
        rationale,
        new_findings,
        decisions,
    );
}

/// Build the LOCUS005 finding + paired decision and push them onto the
/// caller's output vectors. Both kinds of LOCUS005 (recognised bypass +
/// unknown concept id) share this shape — they only differ in the
/// observation line, message, and rationale.
#[allow(clippy::too_many_arguments)]
fn push_locus005(
    concept: &ConceptDeclaration,
    ctx: &PolicyContext<'_>,
    message: String,
    observation_line: String,
    suggested_fix: String,
    rationale: String,
    new_findings: &mut Vec<RuleFinding>,
    decisions: &mut Vec<Decision>,
) {
    let intent_line = format!(
        "Architecture intent declares `{}` source of truth as `{}` via `{}`.",
        concept.id, concept.source_of_truth, concept.registry
    );
    let finding = RuleFinding {
        id: ctx.finding_ids.next(),
        source: FindingSource::Policy(CONCEPT_SOT_ID),
        rule_id: None,
        paradigm_id: None,
        default_severity: Severity::Advisory,
        span: None,
        concept: Some(concept.id.clone()),
        message,
        evidence: Vec::new(),
        why: vec![intent_line, observation_line],
        suggested_fix: Some(suggested_fix),
        diagnostic_code: Some(LOCUS005.into()),
    };
    let decision = Decision {
        finding_id: finding.id,
        policy: CONCEPT_SOT_ID,
        severity: Severity::Advisory,
        status: DecisionStatus::Active,
        severity_change: SeverityChange::Unchanged,
        rationale: vec![rationale],
    };
    new_findings.push(finding);
    decisions.push(decision);
}

/// Quick shape check for `LOCUS\d{3}`-style governance codes. Keeps the
/// policy from flagging rule codes like `CX001` when they appear in
/// `diagnostic_code` for forwards-compat reasons.
fn is_governance_code_shaped(code: &str) -> bool {
    code.len() == 8 && code.starts_with("LOCUS") && code[5..].chars().all(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::CheckMode;
    use crate::governance::arch::{ArchDeclaration, ArchLoadOutcome, ConceptDeclaration};
    use crate::governance::finding::{FindingSource, FindingStore};
    use crate::governance::ids::{FindingId, FindingIdMinter, ParadigmId, PolicyId, RuleId};
    use crate::governance::registry::{ParadigmRegistry, PolicyRegistry, RuleRegistry};
    use crate::lockfile::Lockfile;
    use locus_air::AirWorkspace;

    fn registered_rule_finding(id_raw: u64, rule_code: &'static str) -> RuleFinding {
        RuleFinding {
            id: FindingId::from_raw_for_test(id_raw),
            source: FindingSource::RegisteredRule(RuleId::new(rule_code)),
            rule_id: Some(RuleId::new(rule_code)),
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

    fn legacy_finding(id_raw: u64, rule_code: &str) -> RuleFinding {
        RuleFinding {
            id: FindingId::from_raw_for_test(id_raw),
            source: FindingSource::LegacyDiagnostic {
                rule_code: rule_code.into(),
                paradigm: None,
            },
            rule_id: None,
            paradigm_id: None,
            default_severity: Severity::Warning,
            span: None,
            concept: None,
            message: "legacy".into(),
            evidence: Vec::new(),
            why: Vec::new(),
            suggested_fix: None,
            diagnostic_code: None,
        }
    }

    fn run_with(
        arch: &ArchLoadOutcome,
        store: FindingStore,
        rules: &RuleRegistry,
        paradigms: &ParadigmRegistry,
        policies: &PolicyRegistry,
    ) -> PolicyOutput {
        let air = AirWorkspace::new(Vec::new());
        let lf = Lockfile::empty();
        let minter = FindingIdMinter::new();
        let ctx = PolicyContext {
            air: &air,
            lockfile: &lf,
            mode: CheckMode::Human,
            rule_registry: rules,
            paradigm_registry: paradigms,
            policy_registry: policies,
            findings: &store,
            prior_decisions: &[],
            finding_ids: &minter,
            arch,
        };
        ConceptSourceOfTruthPolicy.decide(&ctx)
    }

    fn rule_concept() -> ConceptDeclaration {
        ConceptDeclaration {
            id: "rule".into(),
            source_of_truth: "RuleDefinition".into(),
            registry: "RuleRegistry".into(),
        }
    }

    fn policy_concept() -> ConceptDeclaration {
        ConceptDeclaration {
            id: "policy".into(),
            source_of_truth: "PolicyDefinition".into(),
            registry: "PolicyRegistry".into(),
        }
    }

    fn governance_code_concept() -> ConceptDeclaration {
        ConceptDeclaration {
            id: "governance-code".into(),
            source_of_truth: "GovernanceDiagnosticRegistry".into(),
            registry: "GovernanceDiagnosticRegistry".into(),
        }
    }

    #[test]
    fn silent_when_arch_missing() {
        let out = run_with(
            &ArchLoadOutcome::Missing,
            FindingStore::new(),
            &RuleRegistry::standard(),
            &ParadigmRegistry::standard(),
            &PolicyRegistry::standard(),
        );
        assert!(out.new_findings.is_empty());
        assert!(out.decisions.is_empty());
    }

    #[test]
    fn silent_when_concepts_empty() {
        let arch = ArchLoadOutcome::Present(ArchDeclaration {
            policies: vec!["registry-integrity".into()],
            concepts: Vec::new(),
        });
        let out = run_with(
            &arch,
            FindingStore::new(),
            &RuleRegistry::standard(),
            &ParadigmRegistry::standard(),
            &PolicyRegistry::standard(),
        );
        assert!(out.new_findings.is_empty());
    }

    #[test]
    fn legacy_diagnostic_skipped_for_rule_concept() {
        // CRITICAL: legacy diagnostics belong to LOCUS003 alone. LOCUS005
        // must not fire for them, even if their rule_code happens to be
        // unregistered (which is the whole point of LOCUS003 -- migration
        // debt is already tracked).
        let arch = ArchLoadOutcome::Present(ArchDeclaration {
            policies: Vec::new(),
            concepts: vec![rule_concept()],
        });
        let mut store = FindingStore::new();
        store.insert(legacy_finding(0, "ZZ999"));
        let out = run_with(
            &arch,
            store,
            &RuleRegistry::standard(),
            &ParadigmRegistry::standard(),
            &PolicyRegistry::standard(),
        );
        assert!(
            out.new_findings.is_empty(),
            "legacy diagnostics must not trigger LOCUS005; got {:?}",
            out.new_findings
                .iter()
                .map(|f| f.message.as_str())
                .collect::<Vec<_>>()
        );
    }

    fn policy_finding_with_code(id: u64, owner: &'static str, code: &str) -> RuleFinding {
        RuleFinding {
            id: FindingId::from_raw_for_test(id),
            source: FindingSource::Policy(PolicyId::new(owner)),
            rule_id: None,
            paradigm_id: None,
            default_severity: Severity::Advisory,
            span: None,
            concept: None,
            message: "m".into(),
            evidence: Vec::new(),
            why: Vec::new(),
            suggested_fix: None,
            diagnostic_code: Some(code.into()),
        }
    }

    #[test]
    fn fires_once_per_unique_unregistered_governance_code() {
        let arch = ArchLoadOutcome::Present(ArchDeclaration {
            policies: Vec::new(),
            concepts: vec![governance_code_concept()],
        });
        let mut store = FindingStore::new();
        // Two findings, same unregistered code → one LOCUS005 (dedupe).
        store.insert(policy_finding_with_code(
            0,
            "registry-integrity",
            "LOCUS999",
        ));
        store.insert(policy_finding_with_code(
            1,
            "registry-integrity",
            "LOCUS999",
        ));
        let out = run_with(
            &arch,
            store,
            &RuleRegistry::standard(),
            &ParadigmRegistry::standard(),
            &PolicyRegistry::standard(),
        );
        let locus005: Vec<&RuleFinding> = out
            .new_findings
            .iter()
            .filter(|f| f.diagnostic_code.as_deref() == Some("LOCUS005"))
            .collect();
        assert_eq!(
            locus005.len(),
            1,
            "two findings with same unregistered code → one dedupe'd LOCUS005; got {:?}",
            locus005
                .iter()
                .map(|f| f.message.as_str())
                .collect::<Vec<_>>()
        );
        let f = locus005[0];
        assert!(
            f.why
                .iter()
                .any(|w| w.contains("GovernanceDiagnosticRegistry")),
            "why[] should reference the declared SoT; got {:?}",
            f.why
        );
        assert_eq!(f.concept.as_deref(), Some("governance-code"));
    }

    #[test]
    fn governance_code_concept_skips_locus_codes_that_are_registered() {
        let arch = ArchLoadOutcome::Present(ArchDeclaration {
            policies: Vec::new(),
            concepts: vec![governance_code_concept()],
        });
        let mut store = FindingStore::new();
        // LOCUS003 IS in the registry — must not trigger LOCUS005.
        store.insert(policy_finding_with_code(
            0,
            "registry-integrity",
            "LOCUS003",
        ));
        let out = run_with(
            &arch,
            store,
            &RuleRegistry::standard(),
            &ParadigmRegistry::standard(),
            &PolicyRegistry::standard(),
        );
        assert!(
            out.new_findings.is_empty(),
            "registered LOCUS003 must not trigger LOCUS005"
        );
    }

    #[test]
    fn fires_for_finding_from_unregistered_policy() {
        let arch = ArchLoadOutcome::Present(ArchDeclaration {
            policies: Vec::new(),
            concepts: vec![policy_concept()],
        });
        let mut store = FindingStore::new();
        // Synthetic finding from a policy that's NOT in the registry. The
        // diagnostic_code is registered (LOCUS003), so the only bypass is
        // the policy id itself.
        store.insert(policy_finding_with_code(
            0,
            "nonexistent-policy",
            "LOCUS003",
        ));
        let out = run_with(
            &arch,
            store,
            &RuleRegistry::standard(),
            &ParadigmRegistry::standard(),
            &PolicyRegistry::standard(),
        );
        let locus005: Vec<&RuleFinding> = out
            .new_findings
            .iter()
            .filter(|f| f.diagnostic_code.as_deref() == Some("LOCUS005"))
            .collect();
        assert_eq!(locus005.len(), 1);
        assert!(locus005[0].message.contains("policy"));
        assert!(
            locus005[0]
                .why
                .iter()
                .any(|w| w.contains("nonexistent-policy"))
                || locus005[0].why.iter().any(|w| w.contains("policy"))
        );
    }

    #[test]
    fn rule_concept_silent_for_registered_rule() {
        let arch = ArchLoadOutcome::Present(ArchDeclaration {
            policies: Vec::new(),
            concepts: vec![rule_concept()],
        });
        let mut store = FindingStore::new();
        // CX001 IS in standard rule registry.
        store.insert(registered_rule_finding(0, "CX001"));
        let out = run_with(
            &arch,
            store,
            &RuleRegistry::standard(),
            &ParadigmRegistry::standard(),
            &PolicyRegistry::standard(),
        );
        assert!(
            out.new_findings.is_empty(),
            "registered rule must not trigger LOCUS005"
        );
    }

    #[test]
    fn unknown_concept_id_emits_one_advisory() {
        let arch = ArchLoadOutcome::Present(ArchDeclaration {
            policies: Vec::new(),
            concepts: vec![ConceptDeclaration {
                id: "unknown-thing".into(),
                source_of_truth: "X".into(),
                registry: "Y".into(),
            }],
        });
        let out = run_with(
            &arch,
            FindingStore::new(),
            &RuleRegistry::standard(),
            &ParadigmRegistry::standard(),
            &PolicyRegistry::standard(),
        );
        let locus005: Vec<&RuleFinding> = out
            .new_findings
            .iter()
            .filter(|f| f.diagnostic_code.as_deref() == Some("LOCUS005"))
            .collect();
        assert_eq!(locus005.len(), 1);
        assert!(locus005[0].message.contains("unknown-thing"));
        assert_eq!(
            locus005[0].default_severity,
            Severity::Advisory,
            "LOCUS005 is always Advisory"
        );
    }

    #[test]
    fn paradigm_concept_silent_for_registered_paradigm() {
        let arch = ArchLoadOutcome::Present(ArchDeclaration {
            policies: Vec::new(),
            concepts: vec![ConceptDeclaration {
                id: "paradigm".into(),
                source_of_truth: "ParadigmDefinition".into(),
                registry: "ParadigmRegistry".into(),
            }],
        });
        // CX paradigm IS in the standard registry — must not trigger LOCUS005.
        let mut store = FindingStore::new();
        let mut f = registered_rule_finding(0, "CX001");
        f.paradigm_id = Some(ParadigmId::new("CX"));
        store.insert(f);
        let out = run_with(
            &arch,
            store,
            &RuleRegistry::standard(),
            &ParadigmRegistry::standard(),
            &PolicyRegistry::standard(),
        );
        assert!(out.new_findings.is_empty());
    }

    #[test]
    fn is_governance_code_shaped_helper() {
        assert!(is_governance_code_shaped("LOCUS001"));
        assert!(is_governance_code_shaped("LOCUS999"));
        assert!(!is_governance_code_shaped("CX001"));
        assert!(!is_governance_code_shaped("LOCUS"));
        assert!(!is_governance_code_shaped("LOCUS1"));
        assert!(!is_governance_code_shaped("LOCUS0001"));
    }

    #[test]
    fn locus005_severity_always_advisory() {
        // Even on a non-trivial finding-set, default_severity must stay Advisory.
        let arch = ArchLoadOutcome::Present(ArchDeclaration {
            policies: Vec::new(),
            concepts: vec![rule_concept(), policy_concept(), governance_code_concept()],
        });
        let mut store = FindingStore::new();
        store.insert(policy_finding_with_code(0, "ghost-policy", "LOCUS999"));
        let out = run_with(
            &arch,
            store,
            &RuleRegistry::standard(),
            &ParadigmRegistry::standard(),
            &PolicyRegistry::standard(),
        );
        for f in &out.new_findings {
            assert_eq!(f.default_severity, Severity::Advisory);
        }
        for d in &out.decisions {
            assert_eq!(d.severity, Severity::Advisory);
            assert_eq!(d.status, DecisionStatus::Active);
        }
    }
}
