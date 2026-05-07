//! PA — Port/Adapter Ownership.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 6: Port/Adapter Ownership".
//!
//! Phase-2 scope:
//! - PA001: trait declared and immediately implemented in the same file
//!   (port and adapter co-located — physical separation never happened).
//!
//! `init` returns an empty section: there's nothing to infer up front. The
//! lockfile starts empty so PA001 fires on every co-located trait+impl pair;
//! the user reviews each one and either splits the port from its adapter
//! (the canonical fix) or accepts the trait as a non-port utility helper via
//! `accepted_colocated_traits`.

// ot: canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
pub mod lockfile_schema;
pub mod rules;

pub const PA_PREFIX: &str = "PA";

pub struct PortAdapter;

impl Paradigm for PortAdapter {
    fn name(&self) -> &'static str {
        "Port/Adapter Ownership"
    }
    fn rule_prefix(&self) -> &'static str {
        PA_PREFIX
    }
    fn init(&self, _air: &AirWorkspace) -> serde_json::Value {
        // No automatic inference — port/adapter exemptions come from review.
        serde_json::Value::Null
    }
    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic> {
        let section: lockfile_schema::PaSection =
            lockfile.paradigm_section(PA_PREFIX).unwrap_or_default();
        rules::pa001(air, &section, mode)
    }
}
