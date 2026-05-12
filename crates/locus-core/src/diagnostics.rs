//! Paradigm-neutral diagnostic shape.
//!
//! Every paradigm emits `Diagnostic` values; the CLI aggregates them. The
//! diagnostic message itself is built by the rule that produced it — this
//! type is the *envelope*, not the prose.

// locus: ot canonical

use locus_air::AirSpan;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Diagnostic {
    /// `OT002`, `DG001`, etc.
    pub rule_id: String,
    pub severity: Severity,
    pub span: AirSpan,
    /// The concept the rule is reasoning about, if known.
    pub concept: Option<String>,
    /// Short message; typically a single sentence.
    pub message: String,
    /// Why the rule matched — confidence-style reason list. Surfaced verbatim
    /// to the user so they can tell whether the inference is fair.
    pub why: Vec<String>,
    /// Suggested fix or CLI command, when there is a clean one.
    pub suggested_fix: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Severity {
    /// Exits the process non-zero.
    Fatal,
    /// Reported but doesn't fail CI in human mode. Becomes fatal under
    /// `--agent-strict` for new code.
    Warning,
    /// Informational; never fails CI.
    Advisory,
}

impl Severity {
    pub fn is_fatal(self) -> bool {
        matches!(self, Severity::Fatal)
    }

    /// Map an inference confidence score to a severity tier per the spec
    /// (`docs/project-jumpoff.md` §"Inference Strategy"):
    ///
    /// - `>= 0.90` — strong inference, fires as `Fatal`
    /// - `>= 0.70` — fires as `Warning` (or `Fatal` under `--agent-strict`)
    /// - `>= 0.50` — `Advisory` only
    /// - `< 0.50`  — suppressed (returns `None`)
    ///
    /// Used by inference-shaped rules (OT002, OT008–OT012) where the
    /// detector produces a confidence score; deterministic rules
    /// (`OT001` duplicate canonical, `DG001` forbidden import) skip this
    /// helper and emit a fixed severity directly.
    pub fn from_confidence(confidence: f32, mode: CheckMode) -> Option<Severity> {
        if confidence >= 0.90 {
            Some(Severity::Fatal)
        } else if confidence >= 0.70 {
            Some(match mode {
                CheckMode::AgentStrict => Severity::Fatal,
                CheckMode::Human => Severity::Warning,
            })
        } else if confidence >= 0.50 {
            Some(Severity::Advisory)
        } else {
            None
        }
    }
}

/// Rule ID for the cross-paradigm "paradigm has no definitions" diagnostic.
/// Emitted by vacant-by-definition paradigms (BO/PA/CR/RW/DA/UT/ER/FL/CF/…)
/// when their declaration lists are empty AND the prefix is not in
/// `Lockfile.acknowledged_empty`. Severity defaults to `Advisory` so it
/// surfaces but doesn't block CI; users either populate the section or
/// explicitly ack the empty state.
pub const VACANT_PARADIGM_RULE: &str = "LOCUS002";

/// Build a `LOCUS002` diagnostic for a paradigm whose declaration lists are
/// empty. `prefix` is the rule prefix (`"BO"`, `"PA"`, …); `name` is the
/// human-readable paradigm name; `missing` lists the empty declaration
/// fields the user is expected to populate (lockfile field name + a short
/// description for the `why` line).
///
/// The diagnostic is anchored at `.locus/lock.json:1` since the violation is
/// the lockfile's responsibility, not any source file.
pub fn vacant_paradigm_diagnostic(
    prefix: &str,
    name: &str,
    missing: &[(&str, &str)],
) -> Diagnostic {
    let mut why: Vec<String> = missing
        .iter()
        .map(|(field, desc)| format!("`paradigms.{prefix}.{field}` is empty — {desc}"))
        .collect();
    why.push(format!(
        "vacant-by-definition: rules under {prefix} cannot fire until at \
         least one declaration list is populated"
    ));
    Diagnostic {
        rule_id: VACANT_PARADIGM_RULE.to_string(),
        severity: Severity::Advisory,
        span: AirSpan::new(crate::lockfile::LOCKFILE_RELATIVE_PATH, 1, 1),
        concept: Some(prefix.to_string()),
        message: format!(
            "paradigm {prefix} ({name}) has no definitions; rules cannot fire \
             until you declare its concepts"
        ),
        why,
        suggested_fix: Some(format!(
            "populate `paradigms.{prefix}` in `.locus/lock.json` (use the matching `locus \
             {} ...` mutators, or hand-edit), or add `\"{prefix}\"` to \
             `acknowledged_empty` in `.locus/lock.json` to silence this paradigm",
            prefix.to_lowercase()
        )),
    }
}

/// Whether a paradigm should treat warnings as fatal. Set by the CLI's
/// `--agent-strict` flag; passed into each paradigm's `check`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckMode {
    Human,
    AgentStrict,
}

impl CheckMode {
    pub fn elevate(&self, sev: Severity) -> Severity {
        match (self, sev) {
            (CheckMode::AgentStrict, Severity::Warning) => Severity::Fatal,
            (_, s) => s,
        }
    }

    /// Like [`CheckMode::elevate`], but for *advisory-tier* rules — broad
    /// heuristics whose Fatal-under-strict economics depend on whether the
    /// user has narrowed them via lockfile config. Examples: CX001/CX002
    /// line budgets, CF002 magic-constant detection.
    ///
    /// - `narrowed = false` (using built-in fallback budget, no overrides):
    ///   stays `Warning` even under `--agent-strict`. The rule is still a
    ///   useful smoke alarm, but blocking CI on un-onboarded code is the
    ///   wrong economics — the user hasn't yet made the rule actionable.
    /// - `narrowed = true` (workspace default explicitly set, or per-module
    ///   override matched, or path exempt list non-empty): behaves like
    ///   regular [`elevate`] — Warning becomes Fatal under
    ///   `--agent-strict`.
    ///
    /// Per `docs/PARADIGMS.md` §"Severity tiers"; tracks issue #6 (epic
    /// #1 child).
    pub fn elevate_when_actionable(&self, sev: Severity, narrowed: bool) -> Severity {
        match (self, sev, narrowed) {
            (CheckMode::AgentStrict, Severity::Warning, true) => Severity::Fatal,
            (_, s, _) => s,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevate_bumps_warning_to_fatal_under_agent_strict() {
        assert_eq!(
            CheckMode::AgentStrict.elevate(Severity::Warning),
            Severity::Fatal
        );
        assert_eq!(
            CheckMode::Human.elevate(Severity::Warning),
            Severity::Warning
        );
    }

    #[test]
    fn elevate_when_actionable_blocks_strict_elevation_for_un_narrowed_advisory_rule() {
        // Un-narrowed (built-in fallback budget): even agent-strict should
        // not turn a broad heuristic into a CI blocker.
        assert_eq!(
            CheckMode::AgentStrict.elevate_when_actionable(Severity::Warning, false),
            Severity::Warning,
        );
        assert_eq!(
            CheckMode::Human.elevate_when_actionable(Severity::Warning, false),
            Severity::Warning,
        );
    }

    #[test]
    fn elevate_when_actionable_elevates_for_narrowed_advisory_rule() {
        // The user has set an explicit budget / per-module override / path
        // exempt — the rule is actionable, so agent-strict should elevate.
        assert_eq!(
            CheckMode::AgentStrict.elevate_when_actionable(Severity::Warning, true),
            Severity::Fatal,
        );
        assert_eq!(
            CheckMode::Human.elevate_when_actionable(Severity::Warning, true),
            Severity::Warning,
        );
    }

    #[test]
    fn elevate_when_actionable_passes_non_warning_severities_through() {
        // Fatal stays Fatal regardless; Advisory stays Advisory.
        assert_eq!(
            CheckMode::AgentStrict.elevate_when_actionable(Severity::Fatal, false),
            Severity::Fatal,
        );
        assert_eq!(
            CheckMode::AgentStrict.elevate_when_actionable(Severity::Advisory, true),
            Severity::Advisory,
        );
    }
}
