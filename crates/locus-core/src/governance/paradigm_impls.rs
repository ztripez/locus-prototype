//! Lifts each existing legacy `Paradigm` into a `ParadigmDefinition`
//! singleton with an initially empty `rules()` slice. Rule migration
//! (filling the slice) is P2; this file is the parity layer ensuring the
//! ParadigmRegistry has an entry for every legacy paradigm.

// locus: ot canonical

use crate::governance::ids::ParadigmId;
use crate::governance::paradigm::ParadigmDefinition;
use crate::governance::rule::RuleDefinition;

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
// CF breaks out of the macro — 3 rules migrated (CF001/002/003 in #71 P4).
pub struct CfParadigmDef;
impl ParadigmDefinition for CfParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("CF")
    }
    fn title(&self) -> &'static str {
        "Config Data"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        static RULES: [&dyn RuleDefinition; 3] = [
            &crate::paradigms::config_data::rules::CF001_RULE,
            &crate::paradigms::config_data::rules::CF002_RULE,
            &crate::paradigms::config_data::rules::CF003_RULE,
        ];
        &RULES
    }
}
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
// DA breaks out of the macro — 3 rules migrated (DA001/002/007 in #71 P4).
pub struct DaParadigmDef;
impl ParadigmDefinition for DaParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("DA")
    }
    fn title(&self) -> &'static str {
        "Demand Driven"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        static RULES: [&dyn RuleDefinition; 3] = [
            &crate::paradigms::demand_driven::rules::DA001_RULE,
            &crate::paradigms::demand_driven::rules::DA002_RULE,
            &crate::paradigms::demand_driven::rules::DA007_RULE,
        ];
        &RULES
    }
}
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
// FO breaks out of the macro — 2 rules migrated (FO001/004 in #71 P4).
pub struct FoParadigmDef;
impl ParadigmDefinition for FoParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("FO")
    }
    fn title(&self) -> &'static str {
        "Feature Ownership"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        static RULES: [&dyn RuleDefinition; 2] = [
            &crate::paradigms::feature_ownership::rules::FO001_RULE,
            &crate::paradigms::feature_ownership::rules::FO004_RULE,
        ];
        &RULES
    }
}

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

// OB breaks out of the macro — 4 rules migrated (OB001/002/003/004 in #71 P4).
pub struct ObParadigmDef;
impl ParadigmDefinition for ObParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("OB")
    }
    fn title(&self) -> &'static str {
        "Observability"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        static RULES: [&dyn RuleDefinition; 4] = [
            &crate::paradigms::observability::rules::OB001_RULE,
            &crate::paradigms::observability::rules::OB002_RULE,
            &crate::paradigms::observability::rules::OB003_RULE,
            &crate::paradigms::observability::rules::OB004_RULE,
        ];
        &RULES
    }
}
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
// RM breaks out of the macro — 6 rules migrated (RM001–RM006 in #71 P4).
pub struct RmParadigmDef;
impl ParadigmDefinition for RmParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("RM")
    }
    fn title(&self) -> &'static str {
        "Responsibility"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        static RULES: [&dyn RuleDefinition; 6] = [
            &crate::paradigms::responsibility::rules::RM001_RULE,
            &crate::paradigms::responsibility::rules::RM002_RULE,
            &crate::paradigms::responsibility::rules::RM003_RULE,
            &crate::paradigms::responsibility::rules::RM004_RULE,
            &crate::paradigms::responsibility::rules::RM005_RULE,
            &crate::paradigms::responsibility::rules::RM006_RULE,
        ];
        &RULES
    }
}
// RW breaks out of the macro — 6 rules migrated (RW001–RW006 in #71 P4).
pub struct RwParadigmDef;
impl ParadigmDefinition for RwParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("RW")
    }
    fn title(&self) -> &'static str {
        "Runtime Work"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        static RULES: [&dyn RuleDefinition; 6] = [
            &crate::paradigms::runtime_work::rules::RW001_RULE,
            &crate::paradigms::runtime_work::rules::RW002_RULE,
            &crate::paradigms::runtime_work::rules::RW003_RULE,
            &crate::paradigms::runtime_work::rules::RW004_RULE,
            &crate::paradigms::runtime_work::rules::RW005_RULE,
            &crate::paradigms::runtime_work::rules::RW006_RULE,
        ];
        &RULES
    }
}
// TA breaks out of the macro — 4 rules migrated (TA001/002/003/004 in #71 P4).
pub struct TaParadigmDef;
impl ParadigmDefinition for TaParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("TA")
    }
    fn title(&self) -> &'static str {
        "Test Architecture"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        static RULES: [&dyn RuleDefinition; 4] = [
            &crate::paradigms::test_architecture::rules::TA001_RULE,
            &crate::paradigms::test_architecture::rules::TA002_RULE,
            &crate::paradigms::test_architecture::rules::TA003_RULE,
            &crate::paradigms::test_architecture::rules::TA004_RULE,
        ];
        &RULES
    }
}
// UT breaks out of the macro — 5 rules migrated (UT001/002/003/004/005 in #71 P4).
pub struct UtParadigmDef;
impl ParadigmDefinition for UtParadigmDef {
    fn id(&self) -> ParadigmId {
        ParadigmId::new("UT")
    }
    fn title(&self) -> &'static str {
        "Utility Discipline"
    }
    fn rules(&self) -> &'static [&'static dyn RuleDefinition] {
        static RULES: [&dyn RuleDefinition; 5] = [
            &crate::paradigms::utility_discipline::rules::UT001_RULE,
            &crate::paradigms::utility_discipline::rules::UT002_RULE,
            &crate::paradigms::utility_discipline::rules::UT003_RULE,
            &crate::paradigms::utility_discipline::rules::UT004_RULE,
            &crate::paradigms::utility_discipline::rules::UT005_RULE,
        ];
        &RULES
    }
}

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
