//! Paradigm-neutral diagnostic shape.
//!
//! Every paradigm emits `Diagnostic` values; the CLI aggregates them. The
//! diagnostic message itself is built by the rule that produced it — this
//! type is the *envelope*, not the prose.

// ot: canonical

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
}
