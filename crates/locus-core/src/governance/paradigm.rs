//! `ParadigmDefinition` trait — concern-pack grouping for migrated rules.
//!
//! A ParadigmDefinition declares which RuleDefinitions belong to its
//! prefix. The slice may be empty during transition — rules not in the
//! slice still run via the legacy `Paradigm::check` path and are wrapped
//! by `LegacyParadigmRuleAdapter`.

// locus: ot canonical

use crate::governance::ids::ParadigmId;
use crate::governance::rule::RuleDefinition;

pub trait ParadigmDefinition: Send + Sync {
    fn id(&self) -> ParadigmId;
    fn title(&self) -> &'static str;
    fn rules(&self) -> &'static [&'static dyn RuleDefinition];
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubParadigm;
    impl ParadigmDefinition for StubParadigm {
        fn id(&self) -> ParadigmId {
            ParadigmId::new("ZZ")
        }
        fn title(&self) -> &'static str {
            "stub"
        }
        fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
            &[]
        }
    }

    #[test]
    fn paradigm_definition_is_object_safe() {
        let p: &dyn ParadigmDefinition = &StubParadigm;
        assert_eq!(p.id().as_str(), "ZZ");
        assert!(p.rules().is_empty());
    }
}
