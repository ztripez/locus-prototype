//! Emission helpers for `ConceptSourceOfTruthPolicy`.
//!
//! Build a `RuleFinding`+`Decision` pair from a recognised bypass or
//! an unknown-concept signal, and route them through the per-concept
//! enforcement → (severity, status) mapping in
//! [`super::enforcement::severity_for`].

// locus: ot canonical

use std::collections::BTreeSet;

use crate::diagnostics::Severity;
use crate::governance::arch::ConceptDeclaration;
use crate::governance::decision::{Decision, DecisionStatus, SeverityChange};
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::policy::PolicyContext;

use super::enforcement::severity_for;
use super::{CONCEPT_SOT_ID, LOCUS005};

pub(super) fn push_unknown_concept(
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
    // Unknown concept ids are a config-quality issue, not a real SoT
    // bypass. Pin Advisory regardless of any per-concept `enforcement`
    // value — promoting "I typo'd an arch.json entry" to Fatal would
    // be hostile and surprising.
    push_locus005_with_severity(
        concept,
        ctx,
        message,
        observation_line,
        suggested_fix,
        rationale,
        (Severity::Advisory, DecisionStatus::Advisory),
        None,
        new_findings,
        decisions,
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_bypass(
    concept: &ConceptDeclaration,
    kind_label: &str,
    identifier: &str,
    observation_line: String,
    suggested_fix: String,
    triggering_span: Option<locus_air::AirSpan>,
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
    let (severity, status) = severity_for(concept.enforcement, ctx.mode);
    push_locus005_with_severity(
        concept,
        ctx,
        message,
        observation_line,
        suggested_fix,
        rationale,
        (severity, status),
        triggering_span,
        new_findings,
        decisions,
    );
}

/// Build the LOCUS005 finding + paired decision with explicit
/// severity/status, and push them onto the caller's output vectors.
/// `severity_change` stays `Unchanged`: LOCUS005 is the policy
/// *making* the decision, not mutating a prior one — the severity
/// itself carries any AgentStrict elevation.
#[allow(clippy::too_many_arguments)]
pub(super) fn push_locus005_with_severity(
    concept: &ConceptDeclaration,
    ctx: &PolicyContext<'_>,
    message: String,
    observation_line: String,
    suggested_fix: String,
    rationale: String,
    (severity, status): (Severity, DecisionStatus),
    triggering_span: Option<locus_air::AirSpan>,
    new_findings: &mut Vec<RuleFinding>,
    decisions: &mut Vec<Decision>,
) {
    let finding = build_locus005_finding(
        concept,
        ctx,
        message,
        observation_line,
        suggested_fix,
        severity,
        triggering_span,
    );
    let decision = Decision {
        finding_id: finding.id,
        policy: CONCEPT_SOT_ID,
        severity,
        status,
        severity_change: SeverityChange::Unchanged,
        rationale: vec![rationale],
    };
    new_findings.push(finding);
    decisions.push(decision);
}

fn build_locus005_finding(
    concept: &ConceptDeclaration,
    ctx: &PolicyContext<'_>,
    message: String,
    observation_line: String,
    suggested_fix: String,
    severity: Severity,
    triggering_span: Option<locus_air::AirSpan>,
) -> RuleFinding {
    let intent_line = format!(
        "Architecture intent declares `{}` source of truth as `{}` via `{}`.",
        concept.id, concept.source_of_truth, concept.registry
    );
    RuleFinding {
        id: ctx.finding_ids.next(),
        source: FindingSource::Policy(CONCEPT_SOT_ID),
        rule_id: None,
        paradigm_id: None,
        default_severity: severity,
        // Preserve the triggering finding's span so `locus check
        // --changed --agent-strict` doesn't silently drop enforced
        // bypasses introduced in modified files (Codex review of #101).
        // Unknown-concept-id branch has no triggering finding and
        // passes `None`; LOCUS005 there is config-quality and not
        // subject to changed-path gating anyway.
        span: triggering_span,
        concept: Some(concept.id.clone()),
        message,
        evidence: Vec::new(),
        why: vec![intent_line, observation_line],
        suggested_fix: Some(suggested_fix),
        diagnostic_code: Some(LOCUS005.into()),
    }
}

/// Quick shape check for `LOCUS\d{3}`-style governance codes. Keeps the
/// policy from flagging rule codes like `CX001` when they appear in
/// `diagnostic_code` for forwards-compat reasons.
pub(super) fn is_governance_code_shaped(code: &str) -> bool {
    code.len() == 8 && code.starts_with("LOCUS") && code[5..].chars().all(|c| c.is_ascii_digit())
}
