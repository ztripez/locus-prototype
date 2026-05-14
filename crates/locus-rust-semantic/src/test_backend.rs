//! In-process [`SemanticAdapter`] used for testing the consumer side
//! of the spike (#111).
//!
//! Lives in its own file because the trait + concrete impl living
//! together trips PA001 (port + adapter co-located). Once
//! `RustAnalyzerBackend` lands in phase 2, that backend ships in a
//! sibling module — `TestBackend` may stay here for downstream tests
//! or be deleted entirely.

// locus: ot canonical

use super::{AdapterError, ResolvedConversion, SemanticAdapter};

/// Returns hand-built [`ResolvedConversion`] facts. Lets consumers
/// (e.g. OT integration tests) exercise the SemanticResolved-vs-
/// Heuristic preference path before the `ra-ap-*` backend lands.
pub struct TestBackend {
    pub facts: Vec<ResolvedConversion>,
}

impl TestBackend {
    pub fn new() -> Self {
        Self { facts: Vec::new() }
    }

    pub fn with_fact(mut self, fact: ResolvedConversion) -> Self {
        self.facts.push(fact);
        self
    }
}

impl Default for TestBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl SemanticAdapter for TestBackend {
    fn name(&self) -> &'static str {
        "test-backend"
    }

    fn resolve_conversions(
        &self,
        _workspace_root: &std::path::Path,
    ) -> Result<Vec<ResolvedConversion>, AdapterError> {
        Ok(self.facts.clone())
    }
}
