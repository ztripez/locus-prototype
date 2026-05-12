//! OT — Canonical Domain Ownership.
//!
//! Spec: `project-jumpoff.md` (treat as the OT-paradigm-specific spec).
//! Umbrella: `Paradigms.md` §"Paradigm 1".
//!
//! Phase 2 ships:
//! - concept clustering ([`infer`])
//! - OT002 (undeclared concept-shaped type) ([`rules`])
//! - lockfile section + `locus init` integration ([`init`], [`lockfile_schema`])
//!
//! `check` consults the lockfile first (a symbol's role recorded there is
//! authoritative), then falls back to source hints. This makes `// locus: ot …`
//! annotations a convenience for first-time onboarding; the lockfile is the
//! source of truth from `locus init` onward.

// locus: ot canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod accept;
pub mod infer;
pub mod init;
pub mod lockfile_schema;
pub mod rules;

pub const OT_PREFIX: &str = "OT";

pub struct OneTruth;

impl Paradigm for OneTruth {
    fn name(&self) -> &'static str {
        "Canonical Domain Ownership"
    }
    fn rule_prefix(&self) -> &'static str {
        OT_PREFIX
    }

    fn init(&self, air: &AirWorkspace) -> serde_json::Value {
        let section = init::build_ot_section(air);
        serde_json::to_value(section).expect("OtSection serializes")
    }

    fn check(
        &self,
        _air: &AirWorkspace,
        _lockfile: &Lockfile,
        _mode: CheckMode,
    ) -> Vec<Diagnostic> {
        // All OT rules migrated to RuleDefinition (#71 P4). They run via
        // the governance pipeline; the legacy check path is now a no-op.
        Vec::new()
    }

    fn suggest(&self, air: &AirWorkspace, lockfile: &Lockfile) -> Vec<crate::init::Suggestion> {
        init::suggest(air, lockfile)
    }
}
