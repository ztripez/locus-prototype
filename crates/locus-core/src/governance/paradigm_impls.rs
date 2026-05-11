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

paradigm_def!(OtParadigmDef, "OT", "Canonical Domain Ownership");
paradigm_def!(DgParadigmDef, "DG", "Dependency Graph");
paradigm_def!(AbParadigmDef, "AB", "Abstraction Discipline");
paradigm_def!(BoParadigmDef, "BO", "Boundary Ownership");
paradigm_def!(CfParadigmDef, "CF", "Config Data");
paradigm_def!(CrParadigmDef, "CR", "Claim Ownership");
paradigm_def!(ClParadigmDef, "CL", "Composition Root");
paradigm_def!(CxParadigmDef, "CX", "Complexity Budget");
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
