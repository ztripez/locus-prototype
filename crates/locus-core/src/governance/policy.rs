//! `PolicyDefinition` trait — the decision-maker layer.
//!
//! Policies inspect findings + prior decisions and emit `Decision`s.
//! They may also emit new findings (e.g. RegistryIntegrityPolicy emits
//! migration-debt findings). MVP is single-pass: a policy sees findings
//! from earlier phases, but later policies seeing newly-emitted findings
//! is recorded for materialization, not used for multi-pass inference.

// locus: ot canonical

use crate::diagnostics::CheckMode;
use crate::governance::decision::Decision;
use crate::governance::finding::{FindingStore, RuleFinding};
use crate::governance::ids::{FindingIdMinter, PolicyId};
use crate::governance::registry::{ParadigmRegistry, PolicyRegistry, RuleRegistry};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub trait PolicyDefinition: Send + Sync {
    fn id(&self) -> PolicyId;
    fn title(&self) -> &'static str;
    fn decide(&self, ctx: &PolicyContext<'_>) -> PolicyOutput;
}

pub struct PolicyOutput {
    pub decisions: Vec<Decision>,
    pub new_findings: Vec<RuleFinding>,
}

impl PolicyOutput {
    pub fn empty() -> Self {
        Self {
            decisions: Vec::new(),
            new_findings: Vec::new(),
        }
    }
}

pub struct PolicyContext<'a> {
    pub air: &'a AirWorkspace,
    pub lockfile: &'a Lockfile,
    pub mode: CheckMode,
    pub rule_registry: &'a RuleRegistry,
    pub paradigm_registry: &'a ParadigmRegistry,
    pub policy_registry: &'a PolicyRegistry,
    pub findings: &'a FindingStore,
    pub prior_decisions: &'a [Decision],
    pub finding_ids: &'a FindingIdMinter,
}
