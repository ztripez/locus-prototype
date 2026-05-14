//! `ArchitectureFacts` aggregator — the runtime model populated by
//! source loaders and consumed by governance policies.

// locus: ot canonical

use serde::{Deserialize, Serialize};

use super::boundary::BoundaryFact;
use super::concept::ConceptFact;
use super::contract::ContractFact;
use super::converter::ConverterFact;
use super::debt::DebtFact;
use super::module_ownership::ModuleOwnershipFact;
use super::source::SourceRef;

/// The aggregator. Populated by source loaders (future #108/#109),
/// consumed by governance policies. The default constructor allocates
/// nothing, so "no architecture sources loaded" is the cheap zero
/// state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchitectureFacts {
    pub concepts: Vec<ConceptFact>,
    pub boundaries: Vec<BoundaryFact>,
    pub contracts: Vec<ContractFact>,
    pub converters: Vec<ConverterFact>,
    pub modules: Vec<ModuleOwnershipFact>,
    pub debts: Vec<DebtFact>,
    pub sources: Vec<SourceRef>,
}

impl ArchitectureFacts {
    /// True when no facts of any kind have been loaded.
    pub fn is_empty(&self) -> bool {
        self.concepts.is_empty()
            && self.boundaries.is_empty()
            && self.contracts.is_empty()
            && self.converters.is_empty()
            && self.modules.is_empty()
            && self.debts.is_empty()
            && self.sources.is_empty()
    }

    /// Sort every Vec in place using each fact type's derived `Ord`.
    /// After `sort`, equality compares against another sorted instance
    /// deterministically regardless of insertion order.
    pub fn sort(&mut self) {
        self.concepts.sort();
        self.boundaries.sort();
        self.contracts.sort();
        self.converters.sort();
        self.modules.sort();
        self.debts.sort();
        self.sources.sort();
    }

    /// Merge `other` into `self`. Useful for the future loader registry
    /// (#108) to compose facts from multiple sources without
    /// re-implementing the union logic. Destructured so adding a new
    /// fact vector to `ArchitectureFacts` becomes a compile error here
    /// — prevents silent fact-loss in future PRs.
    pub fn extend(&mut self, other: ArchitectureFacts) {
        let ArchitectureFacts {
            concepts,
            boundaries,
            contracts,
            converters,
            modules,
            debts,
            sources,
        } = other;
        self.concepts.extend(concepts);
        self.boundaries.extend(boundaries);
        self.contracts.extend(contracts);
        self.converters.extend(converters);
        self.modules.extend(modules);
        self.debts.extend(debts);
        self.sources.extend(sources);
    }
}
