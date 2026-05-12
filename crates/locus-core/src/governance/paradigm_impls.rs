//! Lifts each existing legacy `Paradigm` into a `ParadigmDefinition`
//! singleton with an initially empty `rules()` slice. Rule migration
//! (filling the slice) is P2; this file is the parity layer ensuring the
//! ParadigmRegistry has an entry for every legacy paradigm.

// locus: ot canonical

use crate::governance::ids::ParadigmId;
use crate::governance::paradigm::ParadigmDefinition;
use crate::governance::rule::RuleDefinition;

macro_rules! paradigm_def {
    ($struct_name:ident, $id:literal, $title:literal) => {
        pub struct $struct_name;
        impl ParadigmDefinition for $struct_name {
            fn id(&self) -> ParadigmId {
                ParadigmId::new($id)
            }
            fn title(&self) -> &'static str {
                $title
            }
            fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
                &[]
            }
        }
    };
}

// OT breaks out of the macro — second paradigm with a migrated rule
// (OT002 in P2 #71), so `rules()` returns a non-empty slice.
pub struct OtParadigmDef;
impl ParadigmDefinition for OtParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("OT")
    }
    fn title(&self) -> &'static str {
        "Canonical Domain Ownership"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        static RULES: [&dyn RuleDefinition; 1] =
            [&crate::paradigms::one_truth::rules::ot002::OT002_RULE];
        &RULES
    }
}
// DG breaks out of the macro — four rules migrated (DG001–DG004 in P2/P4 #71).
pub struct DgParadigmDef;
impl ParadigmDefinition for DgParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("DG")
    }
    fn title(&self) -> &'static str {
        "Dependency Graph"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        static RULES: [&dyn RuleDefinition; 4] = [
            &crate::paradigms::dependency_graph::rules::dg001::DG001_RULE,
            &crate::paradigms::dependency_graph::rules::dg002::DG002_RULE,
            &crate::paradigms::dependency_graph::rules::dg003::DG003_RULE,
            &crate::paradigms::dependency_graph::rules::dg004::DG004_RULE,
        ];
        &RULES
    }
}
paradigm_def!(AbParadigmDef, "AB", "Abstraction Discipline");
paradigm_def!(BoParadigmDef, "BO", "Boundary Ownership");
paradigm_def!(CfParadigmDef, "CF", "Config Data");
paradigm_def!(CrParadigmDef, "CR", "Claim Ownership");
paradigm_def!(ClParadigmDef, "CL", "Composition Root");
// CX breaks out of the macro pattern — first paradigm with a migrated
// rule, so `rules()` returns a non-empty slice.
pub struct CxParadigmDef;
impl ParadigmDefinition for CxParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("CX")
    }
    fn title(&self) -> &'static str {
        "Complexity Budget"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        static RULES: [&dyn RuleDefinition; 1] =
            [&crate::paradigms::complexity_budget::rules::cx001::CX001_RULE];
        &RULES
    }
}
paradigm_def!(DaParadigmDef, "DA", "Demand Driven");
paradigm_def!(DcParadigmDef, "DC", "Documentation");
paradigm_def!(ErParadigmDef, "ER", "Error Taxonomy");
paradigm_def!(FlParadigmDef, "FL", "Failure Lineage");
paradigm_def!(FoParadigmDef, "FO", "Feature Ownership");
paradigm_def!(MoParadigmDef, "MO", "Module Ownership");
paradigm_def!(ObParadigmDef, "OB", "Observability");
paradigm_def!(PaParadigmDef, "PA", "Port-Adapter");
paradigm_def!(RmParadigmDef, "RM", "Responsibility");
paradigm_def!(RwParadigmDef, "RW", "Runtime Work");
paradigm_def!(TaParadigmDef, "TA", "Test Architecture");
paradigm_def!(UtParadigmDef, "UT", "Utility Discipline");

pub static ALL_PARADIGM_DEFS: &[&dyn ParadigmDefinition] = &[
    &OtParadigmDef,
    &DgParadigmDef,
    &AbParadigmDef,
    &BoParadigmDef,
    &CfParadigmDef,
    &CrParadigmDef,
    &ClParadigmDef,
    &CxParadigmDef,
    &DaParadigmDef,
    &DcParadigmDef,
    &ErParadigmDef,
    &FlParadigmDef,
    &FoParadigmDef,
    &MoParadigmDef,
    &ObParadigmDef,
    &PaParadigmDef,
    &RmParadigmDef,
    &RwParadigmDef,
    &TaParadigmDef,
    &UtParadigmDef,
];
