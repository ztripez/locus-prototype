//! `ConceptSourceOfTruthPolicy` — `PolicyDefinition` impl and per-concept
//! routing.
//!
//! Sits behind the `concept_sot` mod entrypoint. The four
//! `check_*_concept` functions walk `ctx.findings` looking for bypasses
//! of the declared source of truth and emit one `LOCUS005`-shaped
//! `RuleFinding` per unique `(concept_id, observed_identifier)`.

// locus: ot canonical

use std::collections::BTreeSet;

use crate::governance::arch::{ArchLoadOutcome, ConceptDeclaration};
use crate::governance::decision::Decision;
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::PolicyId;
use crate::governance::policy::{PolicyContext, PolicyDefinition, PolicyOutput};
use crate::governance::registry::GovernanceDiagnosticRegistry;

use super::CONCEPT_SOT_ID;
use super::emit::{emit_bypass, is_governance_code_shaped, push_unknown_concept};

pub struct ConceptSourceOfTruthPolicy;

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
            f.span.clone(),
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
            f.span.clone(),
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
            f.span.clone(),
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
            f.span.clone(),
            ctx,
            seen,
            new_findings,
            decisions,
        );
    }
}
