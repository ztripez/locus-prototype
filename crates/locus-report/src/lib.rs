//! Output formatters for `locus check`.
//!
//! The governance pipeline (`locus-core::governance`) is the source of
//! truth: it produces `(Diagnostic, Decision)` pairs after the policy
//! chain has run. This crate owns the *rendering* of those pairs into
//! text, stable JSON, and SARIF v2.1.0 — formats that downstream tools
//! (humans, CI, code-scanning ingest) consume.
//!
//! Inputs are wrapped in [`DecisionRecord`] so callers can mix
//! governance-produced records (with full decision metadata) with
//! post-pipeline diagnostics (Policy Guard, expired-exception warnings)
//! that don't yet flow through a policy. Writers must accept both
//! shapes — `decision`-free records still render correctly.
//!
//! Issue #29 / spec: SARIF results map to final decisions, not raw rule
//! findings. The text writer is the legacy human format moved out of
//! the CLI verbatim.

pub mod json;
pub mod sarif;
pub mod text;

use locus_core::Diagnostic;
use locus_core::governance::{DecisionStatus, SeverityChange};

/// Bundle of "what was emitted" plus optional "why the policy chain
/// chose to emit it." Stable input shape for every writer in this crate.
///
/// `decision` is `None` for diagnostics that bypass the governance
/// pipeline today — currently Policy Guard (PG###) and the legacy
/// expired-exception warning (LOCUS001). When those eventually flow
/// through policies, callers fill in the field and writers will
/// surface the metadata automatically.
#[derive(Debug, Clone)]
pub struct DecisionRecord {
    pub diagnostic: Diagnostic,
    pub decision: Option<DecisionMetadata>,
}

/// Decision-level metadata copied off the governance pipeline's
/// `Decision` so writers don't need to reach back into the
/// `FindingStore` to render JSON/SARIF properties.
#[derive(Debug, Clone)]
pub struct DecisionMetadata {
    pub policy_id: String,
    pub status: DecisionStatus,
    pub severity_change: SeverityChange,
    pub rationale: Vec<String>,
}

impl DecisionRecord {
    pub fn from_diagnostic(diagnostic: Diagnostic) -> Self {
        Self {
            diagnostic,
            decision: None,
        }
    }

    pub fn with_decision(diagnostic: Diagnostic, decision: DecisionMetadata) -> Self {
        Self {
            diagnostic,
            decision: Some(decision),
        }
    }
}

/// Version of the `Locus` SARIF tool driver. Surfaced in the SARIF
/// `tool.driver.version` field. Pinned via the cargo workspace package
/// version so SARIF consumers can correlate output with a Locus build.
pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Tool name in the SARIF `tool.driver.name` field. Capitalized form
/// per SARIF convention.
pub const TOOL_NAME: &str = "Locus";

/// SARIF tool information URI. Stable across runs.
pub const TOOL_INFORMATION_URI: &str = "https://github.com/ztripez/locus";
