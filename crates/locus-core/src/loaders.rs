//! Loaders enrich AIR with normalized framework-specific facts. The
//! visitor stays language-shaped but framework-neutral; loaders bridge
//! the gap.
//!
//! Each loader inspects an [`AirWorkspace`] (with its `CallSite`s +
//! `Import`s) and produces a list of [`AirFact`]s. The CLI runs all
//! default loaders during scan and appends their facts to
//! [`AirWorkspace::facts`].

use locus_air::{AirFact, AirWorkspace};

// ot: canonical
pub trait Loader: Send + Sync {
    fn name(&self) -> &'static str;
    /// Inspect the AIR and produce normalized facts. Loaders are
    /// deterministic — given the same AIR they must produce the same
    /// facts in a stable order.
    fn enrich(&self, air: &AirWorkspace) -> Vec<AirFact>;
}

/// Run each loader against the AIR and append its facts to
/// [`AirWorkspace::facts`]. Loader order is preserved so each fact's
/// position in `air.facts` is deterministic given the loader list.
pub fn apply_loaders(air: &mut AirWorkspace, loaders: &[Box<dyn Loader>]) {
    for loader in loaders {
        let facts = loader.enrich(air);
        air.facts.extend(facts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{AirFact, AirSpan, AirWorkspace, FactKind, FactTarget};

    struct StaticLoader {
        name: &'static str,
        facts: Vec<AirFact>,
    }

    impl Loader for StaticLoader {
        fn name(&self) -> &'static str {
            self.name
        }
        fn enrich(&self, _air: &AirWorkspace) -> Vec<AirFact> {
            self.facts.clone()
        }
    }

    fn fact(kind: FactKind, source: &str, sym: &str) -> AirFact {
        AirFact {
            kind,
            target: FactTarget::Function {
                symbol: sym.to_string(),
            },
            source: source.to_string(),
            confidence: 0.9,
            reasons: Vec::new(),
            evidence: None,
        }
    }

    #[test]
    fn apply_loaders_appends_facts_in_loader_order() {
        let mut air = AirWorkspace::new(Vec::new());
        let loaders: Vec<Box<dyn Loader>> = vec![
            Box::new(StaticLoader {
                name: "first",
                facts: vec![fact(FactKind::SpawnedWork, "first", "x::a")],
            }),
            Box::new(StaticLoader {
                name: "second",
                facts: vec![fact(FactKind::ConfigRead, "second", "x::b")],
            }),
        ];
        apply_loaders(&mut air, &loaders);
        assert_eq!(air.facts.len(), 2);
        assert_eq!(air.facts[0].source, "first");
        assert_eq!(air.facts[1].source, "second");
    }

    #[test]
    fn apply_loaders_preserves_existing_facts() {
        let mut air = AirWorkspace::new(Vec::new());
        air.facts.push(AirFact {
            kind: FactKind::Logging,
            target: FactTarget::Span(AirSpan::new("t.rs", 1, 1)),
            source: "preloaded".into(),
            confidence: 1.0,
            reasons: Vec::new(),
            evidence: None,
        });
        let loaders: Vec<Box<dyn Loader>> = vec![Box::new(StaticLoader {
            name: "extra",
            facts: vec![fact(FactKind::PersistenceWrite, "extra", "x::c")],
        })];
        apply_loaders(&mut air, &loaders);
        assert_eq!(air.facts.len(), 2);
        assert_eq!(air.facts[0].source, "preloaded");
        assert_eq!(air.facts[1].source, "extra");
    }

    #[test]
    fn apply_loaders_no_loaders_is_noop() {
        let mut air = AirWorkspace::new(Vec::new());
        apply_loaders(&mut air, &[]);
        assert!(air.facts.is_empty());
    }
}
