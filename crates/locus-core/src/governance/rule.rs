//! `RuleDefinition` trait — the new evidence-producer abstraction.
//!
//! Per-rule-id structs implement this trait; the engine drives them
//! (not paradigms). Each rule sees a read-only `RuleContext` carrying the
//! AIR, the lockfile, the run mode, and read-only references to the
//! rule/paradigm registries.

// locus: ot canonical

use crate::diagnostics::{CheckMode, Severity};
use crate::governance::finding::RuleFinding;
use crate::governance::ids::{FindingIdMinter, ParadigmId, RuleId};
use crate::governance::registry::{ParadigmRegistry, RuleRegistry};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub trait RuleDefinition: Send + Sync {
    fn id(&self) -> RuleId;
    fn paradigm(&self) -> ParadigmId;
    fn title(&self) -> &'static str;
    fn default_severity(&self) -> Severity;
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding>;
}

pub struct RuleContext<'a> {
    pub air: &'a AirWorkspace,
    pub lockfile: &'a Lockfile,
    pub mode: CheckMode,
    pub rule_registry: &'a RuleRegistry,
    pub paradigm_registry: &'a ParadigmRegistry,
    pub finding_ids: &'a FindingIdMinter,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubRule;
    impl RuleDefinition for StubRule {
        fn id(&self) -> RuleId {
            RuleId::new("ZZ001")
        }
        fn paradigm(&self) -> ParadigmId {
            ParadigmId::new("ZZ")
        }
        fn title(&self) -> &'static str {
            "stub"
        }
        fn default_severity(&self) -> Severity {
            Severity::Advisory
        }
        fn observe(&self, _: &RuleContext<'_>) -> Vec<RuleFinding> {
            Vec::new()
        }
    }

    #[test]
    fn rule_definition_is_object_safe() {
        let r: &dyn RuleDefinition = &StubRule;
        assert_eq!(r.id().as_str(), "ZZ001");
        assert_eq!(r.paradigm().as_str(), "ZZ");
        assert_eq!(r.title(), "stub");
        assert_eq!(r.default_severity(), Severity::Advisory);
    }
}
