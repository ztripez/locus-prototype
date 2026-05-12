//! Governance registries.
//!
//! Four registries:
//! - `RuleRegistry` — migrated `RuleDefinition`s (rule codes like `CX001`).
//! - `ParadigmRegistry` — all current legacy paradigms, lifted as
//!   `ParadigmDefinition` so the new pipeline can ask each paradigm what
//!   rules it owns.
//! - `PolicyRegistry` — ordered policy chain. `DefaultPassThroughPolicy`
//!   is guaranteed last.
//! - `GovernanceDiagnosticRegistry` — governance/policy diagnostic codes
//!   (`LOCUS001`/`LOCUS002`/`LOCUS003`). Deliberately separate from
//!   `RuleRegistry`: `PolicyId` values are internal and must not surface
//!   as user-facing codes.

// locus: ot canonical

use crate::governance::decision::Decision;
use crate::governance::finding::FindingStore;
use crate::governance::ids::{ParadigmId, PolicyId, RuleId};
use crate::governance::paradigm::ParadigmDefinition;
use crate::governance::policy::PolicyDefinition;
use crate::governance::rule::RuleDefinition;

pub struct RuleRegistry {
    rules: Vec<&'static dyn RuleDefinition>,
}

impl RuleRegistry {
    /// Migrated rules. Grows as rules move from legacy `Paradigm::check`
    /// to `RuleDefinition` impls. CX001 lands in P2 (#71); others follow
    /// in subsequent PRs.
    ///
    /// Construction-time invariants (uniqueness, prefix consistency) are
    /// asserted under `debug_assert!`. The spec mandates a recoverable
    /// error path at runtime; this is the MVP form until a fallible
    /// constructor lands (see spec §"Registries → Construction-time
    /// validation").
    pub fn standard() -> Self {
        let reg = Self {
            rules: vec![
                &crate::paradigms::complexity_budget::rules::cx001::CX001_RULE,
                &crate::paradigms::dependency_graph::rules::dg001::DG001_RULE,
                &crate::paradigms::dependency_graph::rules::dg002::DG002_RULE,
                &crate::paradigms::dependency_graph::rules::dg003::DG003_RULE,
                &crate::paradigms::dependency_graph::rules::dg004::DG004_RULE,
                &crate::paradigms::one_truth::rules::ot002::OT002_RULE,
            ],
        };
        debug_assert!(
            reg.validate().is_ok(),
            "RuleRegistry::standard() violates a construction invariant: {:?}",
            reg.validate()
        );
        reg
    }

    /// Test-only constructor.
    #[cfg(test)]
    pub fn with_rules(rules: Vec<&'static dyn RuleDefinition>) -> Self {
        let r = Self { rules };
        r.validate().expect("test registry violated invariants");
        r
    }

    pub fn iter(&self) -> impl Iterator<Item = &&'static dyn RuleDefinition> {
        self.rules.iter()
    }

    pub fn find(&self, id: &RuleId) -> Option<&'static dyn RuleDefinition> {
        self.rules.iter().copied().find(|r| r.id() == *id)
    }

    /// Used by the legacy adapter to decide whether to wrap a legacy
    /// diagnostic's rule_code or skip it (because a registered rule
    /// already covers that code).
    pub fn contains_code(&self, code: &str) -> bool {
        self.rules.iter().any(|r| r.id().as_str() == code)
    }

    /// Validate construction invariants:
    /// 1. Rule IDs are distinct.
    /// 2. Every rule's id starts with its paradigm prefix.
    pub fn validate(&self) -> Result<(), RegistryError> {
        let mut seen = std::collections::HashSet::new();
        for r in &self.rules {
            if !seen.insert(r.id()) {
                return Err(RegistryError::DuplicateRuleId(r.id().as_str().to_string()));
            }
            if !r.id().as_str().starts_with(r.paradigm().as_str()) {
                return Err(RegistryError::RulePrefixMismatch {
                    rule: r.id().as_str().to_string(),
                    paradigm: r.paradigm().as_str().to_string(),
                });
            }
        }
        Ok(())
    }
}

pub struct ParadigmRegistry {
    paradigms: Vec<&'static dyn ParadigmDefinition>,
}

impl ParadigmRegistry {
    /// All current legacy paradigms with empty `rules()` slices. P2 fills
    /// the slices as rules migrate.
    pub fn standard() -> Self {
        Self {
            paradigms: standard_paradigms(),
        }
    }

    #[cfg(test)]
    pub fn empty() -> Self {
        Self {
            paradigms: Vec::new(),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &&'static dyn ParadigmDefinition> {
        self.paradigms.iter()
    }

    pub fn find(&self, id: &ParadigmId) -> Option<&'static dyn ParadigmDefinition> {
        self.paradigms.iter().copied().find(|p| p.id() == *id)
    }
}

pub struct PolicyRegistry {
    policies: Vec<&'static dyn PolicyDefinition>,
}

impl PolicyRegistry {
    /// Only `DefaultPassThroughPolicy` in P1. RegistryIntegrityPolicy lands
    /// in P3 and goes BEFORE pass-through.
    pub fn standard() -> Self {
        Self {
            policies: standard_policies(),
        }
    }

    #[cfg(test)]
    pub fn with_policies(policies: Vec<&'static dyn PolicyDefinition>) -> Self {
        Self { policies }
    }

    pub fn iter(&self) -> impl Iterator<Item = &&'static dyn PolicyDefinition> {
        self.policies.iter()
    }

    pub fn find(&self, id: &PolicyId) -> Option<&'static dyn PolicyDefinition> {
        self.policies.iter().copied().find(|p| p.id() == *id)
    }
}

pub struct GovernanceDiagnosticRegistry {
    codes: Vec<(&'static str, PolicyId)>,
}

impl GovernanceDiagnosticRegistry {
    pub fn standard() -> Self {
        Self {
            codes: vec![
                // Existing CLI-layer governance codes. Owner becomes a real
                // PolicyId when those migrate (future epic).
                ("LOCUS001", PolicyId::new("legacy-exceptions")),
                ("LOCUS002", PolicyId::new("legacy-vacancy-nudge")),
                // Reserved for P3 RegistryIntegrityPolicy.
                ("LOCUS003", PolicyId::new("registry-integrity")),
            ],
        }
    }

    pub fn contains(&self, code: &str) -> bool {
        self.codes.iter().any(|(c, _)| *c == code)
    }

    pub fn owner(&self, code: &str) -> Option<PolicyId> {
        self.codes.iter().find(|(c, _)| *c == code).map(|(_, p)| *p)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    DuplicateRuleId(String),
    RulePrefixMismatch { rule: String, paradigm: String },
    DuplicateDecision { finding_id: u64 },
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryError::DuplicateRuleId(id) => {
                write!(f, "duplicate rule id in RuleRegistry: {id}")
            }
            RegistryError::RulePrefixMismatch { rule, paradigm } => write!(
                f,
                "rule id {rule} does not start with paradigm prefix {paradigm}"
            ),
            RegistryError::DuplicateDecision { finding_id } => {
                write!(f, "multiple decisions for finding {finding_id}")
            }
        }
    }
}

impl std::error::Error for RegistryError {}

/// Asserts every finding has exactly one decision. Called after policy
/// evaluation in pipeline::run.
pub fn validate_decisions(
    decisions: &[Decision],
    store: &FindingStore,
) -> Result<(), RegistryError> {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    for d in decisions {
        if !seen.insert(d.finding_id) {
            return Err(RegistryError::DuplicateDecision {
                finding_id: d.finding_id.as_u64(),
            });
        }
    }
    // Every finding in the store must be decided.
    for f in store.iter() {
        if !seen.contains(&f.id) {
            panic!(
                "finding {} has no decision after policy chain",
                f.id.as_u64()
            );
        }
    }
    Ok(())
}

fn standard_paradigms() -> Vec<&'static dyn ParadigmDefinition> {
    crate::governance::paradigm_impls::ALL_PARADIGM_DEFS.to_vec()
}

fn standard_policies() -> Vec<&'static dyn PolicyDefinition> {
    // RegistryIntegrityPolicy MUST come before DefaultPassThroughPolicy.
    // Future policies (ExceptionPolicy, ...) insert between them.
    vec![
        &crate::governance::policies::registry_integrity::RegistryIntegrityPolicy,
        &crate::governance::policies::default::DefaultPassThroughPolicy,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Severity;
    use crate::governance::finding::{FindingSource, RuleFinding};
    use crate::governance::ids::{FindingId, RuleId};
    use crate::governance::rule::{RuleContext, RuleDefinition};

    struct StubRule(&'static str, &'static str);
    impl RuleDefinition for StubRule {
        fn id(&self) -> RuleId {
            RuleId::new(self.0)
        }
        fn paradigm(&self) -> ParadigmId {
            ParadigmId::new(self.1)
        }
        fn title(&self) -> &'static str {
            "stub"
        }
        fn default_severity(&self) -> Severity {
            Severity::Warning
        }
        fn observe(&self, _: &RuleContext<'_>) -> Vec<RuleFinding> {
            Vec::new()
        }
    }

    static R_CX001: StubRule = StubRule("CX001", "CX");
    static R_CX001_DUP: StubRule = StubRule("CX001", "CX");
    static R_BAD_PREFIX: StubRule = StubRule("OT001", "CX");

    #[test]
    fn duplicate_rule_ids_are_rejected() {
        let r = RuleRegistry {
            rules: vec![&R_CX001, &R_CX001_DUP],
        };
        assert!(matches!(
            r.validate(),
            Err(RegistryError::DuplicateRuleId(_))
        ));
    }

    #[test]
    fn rule_prefix_mismatch_is_rejected() {
        let r = RuleRegistry {
            rules: vec![&R_BAD_PREFIX],
        };
        assert!(matches!(
            r.validate(),
            Err(RegistryError::RulePrefixMismatch { .. })
        ));
    }

    #[test]
    fn well_formed_registry_validates() {
        let r = RuleRegistry {
            rules: vec![&R_CX001],
        };
        assert!(r.validate().is_ok());
        assert!(r.contains_code("CX001"));
        assert!(!r.contains_code("CX002"));
    }

    #[test]
    fn governance_diagnostic_registry_knows_locus003() {
        let g = GovernanceDiagnosticRegistry::standard();
        assert!(g.contains("LOCUS003"));
        assert_eq!(g.owner("LOCUS003").unwrap().as_str(), "registry-integrity");
    }

    #[test]
    fn validate_decisions_rejects_duplicates() {
        use crate::governance::decision::{DecisionStatus, SeverityChange};
        let fid = FindingId::from_raw_for_test(0);
        let mut store = FindingStore::new();
        store.insert(test_finding(fid));
        let d = Decision {
            finding_id: fid,
            policy: PolicyId::new("p"),
            severity: Severity::Warning,
            status: DecisionStatus::Active,
            severity_change: SeverityChange::Unchanged,
            rationale: Vec::new(),
        };
        let result = validate_decisions(&[d.clone(), d], &store);
        assert!(matches!(
            result,
            Err(RegistryError::DuplicateDecision { .. })
        ));
    }

    #[test]
    fn default_pass_through_policy_is_last_in_standard_registry() {
        let reg = PolicyRegistry::standard();
        let policies: Vec<_> = reg.iter().collect();
        assert!(!policies.is_empty(), "policy registry must not be empty");
        let last = policies.last().unwrap();
        assert_eq!(
            last.id().as_str(),
            "default-pass-through",
            "DefaultPassThroughPolicy MUST be the last entry in PolicyRegistry::standard()"
        );
    }

    #[test]
    fn registry_integrity_policy_is_before_pass_through() {
        let reg = PolicyRegistry::standard();
        let ids: Vec<&str> = reg.iter().map(|p| p.id().as_str()).collect();
        let ri_pos = ids
            .iter()
            .position(|&id| id == "registry-integrity")
            .expect("RegistryIntegrityPolicy must be in PolicyRegistry::standard()");
        let pt_pos = ids
            .iter()
            .position(|&id| id == "default-pass-through")
            .expect("DefaultPassThroughPolicy must be in PolicyRegistry::standard()");
        assert!(
            ri_pos < pt_pos,
            "RegistryIntegrityPolicy ({ri_pos}) must come before DefaultPassThroughPolicy ({pt_pos})"
        );
    }

    #[test]
    fn rule_registry_standard_satisfies_construction_invariants() {
        // P2 lands the first real registered rule. From now on, the
        // standard registry must validate clean: no duplicate IDs, every
        // rule's id starts with its paradigm prefix. Future migrations
        // (OT002, DG001, …) must keep this passing.
        let reg = RuleRegistry::standard();
        reg.validate()
            .expect("RuleRegistry::standard() must validate");
    }

    #[test]
    fn rule_registry_contains_cx001_after_p2_migration() {
        let reg = RuleRegistry::standard();
        assert!(
            reg.contains_code("CX001"),
            "CX001 must be in RuleRegistry::standard() after P2"
        );
        let rule = reg.find(&RuleId::new("CX001")).expect("CX001 missing");
        assert_eq!(rule.paradigm().as_str(), "CX");
        assert_eq!(
            rule.default_severity(),
            crate::diagnostics::Severity::Warning
        );
    }

    #[test]
    fn cx_paradigm_def_lists_cx001_rule() {
        let reg = ParadigmRegistry::standard();
        let cx = reg
            .find(&ParadigmId::new("CX"))
            .expect("CX ParadigmDefinition missing");
        let rule_ids: Vec<&str> = cx.rules().iter().map(|r| r.id().as_str()).collect();
        assert_eq!(rule_ids, vec!["CX001"]);
    }

    #[test]
    fn rule_registry_contains_ot002_after_p2_migration() {
        let reg = RuleRegistry::standard();
        assert!(
            reg.contains_code("OT002"),
            "OT002 must be in RuleRegistry::standard() after P2-OT002"
        );
        let rule = reg.find(&RuleId::new("OT002")).expect("OT002 missing");
        assert_eq!(rule.paradigm().as_str(), "OT");
        assert_eq!(
            rule.default_severity(),
            crate::diagnostics::Severity::Warning
        );
    }

    #[test]
    fn ot_paradigm_def_lists_ot002_rule() {
        let reg = ParadigmRegistry::standard();
        let ot = reg
            .find(&ParadigmId::new("OT"))
            .expect("OT ParadigmDefinition missing");
        let rule_ids: Vec<&str> = ot.rules().iter().map(|r| r.id().as_str()).collect();
        assert_eq!(rule_ids, vec!["OT002"]);
    }

    #[test]
    fn rule_registry_contains_dg001_after_p2_migration() {
        let reg = RuleRegistry::standard();
        assert!(
            reg.contains_code("DG001"),
            "DG001 must be in RuleRegistry::standard() after P2-DG001"
        );
        let rule = reg.find(&RuleId::new("DG001")).expect("DG001 missing");
        assert_eq!(rule.paradigm().as_str(), "DG");
        // DG001 is always Fatal — forbidden edge is the user's own
        // declaration, not an inferred budget.
        assert_eq!(rule.default_severity(), crate::diagnostics::Severity::Fatal);
    }

    #[test]
    fn dg_paradigm_def_lists_dg001_rule() {
        let reg = ParadigmRegistry::standard();
        let dg = reg
            .find(&ParadigmId::new("DG"))
            .expect("DG ParadigmDefinition missing");
        let rule_ids: Vec<&str> = dg.rules().iter().map(|r| r.id().as_str()).collect();
        assert_eq!(rule_ids, vec!["DG001", "DG002", "DG003", "DG004"]);
    }

    #[test]
    fn rule_registry_contains_dg002_dg003_dg004() {
        let reg = RuleRegistry::standard();
        assert!(reg.contains_code("DG002"), "DG002 must be in registry");
        assert!(reg.contains_code("DG003"), "DG003 must be in registry");
        assert!(reg.contains_code("DG004"), "DG004 must be in registry");
    }

    #[test]
    fn standard_paradigm_registry_has_every_legacy_paradigm() {
        let std_reg = ParadigmRegistry::standard();
        let legacy = crate::paradigms::registry();
        let std_ids: std::collections::HashSet<&str> =
            std_reg.iter().map(|p| p.id().as_str()).collect();
        for lp in &legacy {
            assert!(
                std_ids.contains(lp.rule_prefix()),
                "ParadigmDefinition missing for legacy paradigm prefix {}",
                lp.rule_prefix()
            );
        }
        let legacy_ids: std::collections::HashSet<&str> =
            legacy.iter().map(|p| p.rule_prefix()).collect();
        for sp in std_reg.iter() {
            assert!(
                legacy_ids.contains(sp.id().as_str()),
                "ParadigmDefinition {} has no matching legacy Paradigm",
                sp.id().as_str()
            );
        }
    }

    fn test_finding(id: FindingId) -> RuleFinding {
        RuleFinding {
            id,
            source: FindingSource::RegisteredRule(RuleId::new("CX001")),
            rule_id: None,
            paradigm_id: None,
            default_severity: Severity::Warning,
            span: None,
            concept: None,
            message: String::new(),
            evidence: Vec::new(),
            why: Vec::new(),
            suggested_fix: None,
            diagnostic_code: None,
        }
    }
}
