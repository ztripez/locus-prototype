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

// OT breaks out of the macro — all 12 rules migrated to RuleDefinition (#71 P4).
pub struct OtParadigmDef;
impl ParadigmDefinition for OtParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("OT")
    }
    fn title(&self) -> &'static str {
        "Canonical Domain Ownership"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        static RULES: [&dyn RuleDefinition; 12] = [
            &crate::paradigms::one_truth::rules::ot001::OT001_RULE,
            &crate::paradigms::one_truth::rules::ot002::OT002_RULE,
            &crate::paradigms::one_truth::rules::ot003::OT003_RULE,
            &crate::paradigms::one_truth::rules::ot004::OT004_RULE,
            &crate::paradigms::one_truth::rules::ot005::OT005_RULE,
            &crate::paradigms::one_truth::rules::ot006::OT006_RULE,
            &crate::paradigms::one_truth::rules::ot007::OT007_RULE,
            &crate::paradigms::one_truth::rules::ot008::OT008_RULE,
            &crate::paradigms::one_truth::rules::ot009::OT009_RULE,
            &crate::paradigms::one_truth::rules::ot010::OT010_RULE,
            &crate::paradigms::one_truth::rules::ot011::OT011_RULE,
            &crate::paradigms::one_truth::rules::ot012::OT012_RULE,
        ];
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
pub struct AbParadigmDef;
impl ParadigmDefinition for AbParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("AB")
    }
    fn title(&self) -> &'static str {
        "Abstraction Discipline"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        static RULES: [&dyn RuleDefinition; 2] = [
            &crate::paradigms::abstraction_discipline::rules::AB001_RULE,
            &crate::paradigms::abstraction_discipline::rules::AB002_RULE,
        ];
        &RULES
    }
}

pub struct ClParadigmDef;
impl ParadigmDefinition for ClParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("CL")
    }
    fn title(&self) -> &'static str {
        "Composition Root"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        static RULES: [&dyn RuleDefinition; 1] =
            [&crate::paradigms::claim_ownership::rules::CL001_RULE];
        &RULES
    }
}

pub struct BoParadigmDef;
impl ParadigmDefinition for BoParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("BO")
    }
    fn title(&self) -> &'static str {
        "Boundary Ownership"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        static RULES: [&dyn RuleDefinition; 4] = [
            &crate::paradigms::boundary_ownership::rules::BO001_RULE,
            &crate::paradigms::boundary_ownership::rules::BO002_RULE,
            &crate::paradigms::boundary_ownership::rules::BO004_RULE,
            &crate::paradigms::boundary_ownership::rules::BO005_RULE,
        ];
        &RULES
    }
}
paradigm_def!(CfParadigmDef, "CF", "Config Data");
pub struct CrParadigmDef;
impl ParadigmDefinition for CrParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("CR")
    }
    fn title(&self) -> &'static str {
        "Claim Ownership"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        static RULES: [&dyn RuleDefinition; 2] = [
            &crate::paradigms::composition_root::rules::CR001_RULE,
            &crate::paradigms::composition_root::rules::CR002_RULE,
        ];
        &RULES
    }
}
// CX breaks out of the macro pattern — four rules migrated (CX001–CX002–CX007–CX008 in P2/P4 #71).
pub struct CxParadigmDef;
impl ParadigmDefinition for CxParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("CX")
    }
    fn title(&self) -> &'static str {
        "Complexity Budget"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        static RULES: [&dyn RuleDefinition; 4] = [
            &crate::paradigms::complexity_budget::rules::cx001::CX001_RULE,
            &crate::paradigms::complexity_budget::rules::cx002::CX002_RULE,
            &crate::paradigms::complexity_budget::rules::cx007::CX007_RULE,
            &crate::paradigms::complexity_budget::rules::cx008::CX008_RULE,
        ];
        &RULES
    }
}
paradigm_def!(DaParadigmDef, "DA", "Demand Driven");
pub struct DcParadigmDef;
impl ParadigmDefinition for DcParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("DC")
    }
    fn title(&self) -> &'static str {
        "Documentation"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        static RULES: [&dyn RuleDefinition; 3] = [
            &crate::paradigms::documentation::rules::DC001_RULE,
            &crate::paradigms::documentation::rules::DC002_RULE,
            &crate::paradigms::documentation::rules::DC004_RULE,
        ];
        &RULES
    }
}
// ER breaks out of the macro — 5 rules migrated (ER001/002/003/005/007 in #71 P4).
pub struct ErParadigmDef;
impl ParadigmDefinition for ErParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("ER")
    }
    fn title(&self) -> &'static str {
        "Error Taxonomy"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        static RULES: [&dyn RuleDefinition; 5] = [
            &crate::paradigms::error_taxonomy::rules::ER001_RULE,
            &crate::paradigms::error_taxonomy::rules::ER002_RULE,
            &crate::paradigms::error_taxonomy::rules::ER003_RULE,
            &crate::paradigms::error_taxonomy::rules::ER005_RULE,
            &crate::paradigms::error_taxonomy::rules::ER007_RULE,
        ];
        &RULES
    }
}
// FL breaks out of the macro — 11 rules migrated (#71 P4).
pub struct FlParadigmDef;
impl ParadigmDefinition for FlParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("FL")
    }
    fn title(&self) -> &'static str {
        "Failure Lineage"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        static RULES: [&dyn RuleDefinition; 11] = [
            &crate::paradigms::failure_lineage::rules::fl001::FL001_RULE,
            &crate::paradigms::failure_lineage::rules::fl002::FL002_RULE,
            &crate::paradigms::failure_lineage::rules::fl003::FL003_RULE,
            &crate::paradigms::failure_lineage::rules::fl004::FL004_RULE,
            &crate::paradigms::failure_lineage::rules::fl005::FL005_RULE,
            &crate::paradigms::failure_lineage::rules::fl006::FL006_RULE,
            &crate::paradigms::failure_lineage::rules::fl007::FL007_RULE,
            &crate::paradigms::failure_lineage::rules::fl010::FL010_RULE,
            &crate::paradigms::failure_lineage::rules::fl011::FL011_RULE,
            &crate::paradigms::failure_lineage::rules::fl012::FL012_RULE,
            &crate::paradigms::failure_lineage::rules::fl013::FL013_RULE,
        ];
        &RULES
    }
}
paradigm_def!(FoParadigmDef, "FO", "Feature Ownership");

pub struct MoParadigmDef;
impl ParadigmDefinition for MoParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("MO")
    }
    fn title(&self) -> &'static str {
        "Module Ownership"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        static RULES: [&dyn RuleDefinition; 5] = [
            &crate::paradigms::module_ownership::rules::MO001_RULE,
            &crate::paradigms::module_ownership::rules::MO002_RULE,
            &crate::paradigms::module_ownership::rules::MO003_RULE,
            &crate::paradigms::module_ownership::rules::MO004_RULE,
            &crate::paradigms::module_ownership::rules::MO005_RULE,
        ];
        &RULES
    }
}

paradigm_def!(ObParadigmDef, "OB", "Observability");
// PA breaks out of the macro — 4 rules migrated (PA001/002/003/004 in #71 P4).
pub struct PaParadigmDef;
impl ParadigmDefinition for PaParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("PA")
    }
    fn title(&self) -> &'static str {
        "Port-Adapter"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        static RULES: [&dyn RuleDefinition; 4] = [
            &crate::paradigms::port_adapter::rules::PA001_RULE,
            &crate::paradigms::port_adapter::rules::PA002_RULE,
            &crate::paradigms::port_adapter::rules::PA003_RULE,
            &crate::paradigms::port_adapter::rules::PA004_RULE,
        ];
        &RULES
    }
}
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
