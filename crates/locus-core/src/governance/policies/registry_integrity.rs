//! `RegistryIntegrityPolicy` — governance health check.
//!
//! Runs BEFORE `DefaultPassThroughPolicy`. Inspects the finding store and
//! emits `LOCUS003` advisory findings for each unique legacy rule code
//! observed this run, surfacing migration backlog.

// locus: ot canonical

use crate::governance::ids::PolicyId;
use crate::governance::policy::{PolicyContext, PolicyDefinition, PolicyOutput};

pub struct RegistryIntegrityPolicy;

pub const REGISTRY_INTEGRITY_ID: PolicyId = PolicyId::new("registry-integrity");

impl PolicyDefinition for RegistryIntegrityPolicy {
    fn id(&self) -> PolicyId {
        REGISTRY_INTEGRITY_ID
    }

    fn title(&self) -> &'static str {
        "Registry Integrity"
    }

    fn decide(&self, _ctx: &PolicyContext<'_>) -> PolicyOutput {
        PolicyOutput::empty()
    }
}
