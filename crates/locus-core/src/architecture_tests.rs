//! Tests for `architecture` — sibling-attached via
//! `#[path = "architecture_tests.rs"] mod tests;` to keep the schema
//! module under the CX002 line budget.

use super::*;

fn src(id: &str, kind: &str) -> SourceRef {
    SourceRef {
        id: id.to_string(),
        kind: kind.to_string(),
        path: None,
    }
}

fn concept(id: &str, src_id: &str) -> ConceptFact {
    ConceptFact {
        id: id.to_string(),
        source_of_truth: None,
        registry: None,
        source: src(src_id, "markdown"),
    }
}

fn boundary(id: &str, kind: BoundaryKind, src_id: &str) -> BoundaryFact {
    BoundaryFact {
        id: id.to_string(),
        kind,
        adapters_allowed: false,
        source: src(src_id, "markdown"),
    }
}

fn populated_facts() -> ArchitectureFacts {
    ArchitectureFacts {
        concepts: vec![concept("user", "adr:0001")],
        boundaries: vec![boundary("http-api", BoundaryKind::Http, "openapi:api.yaml")],
        contracts: vec![ContractFact {
            source_kind: "openapi".to_string(),
            operation: Some("getUser".to_string()),
            path: Some("/users/{id}".to_string()),
            schema: Some("User".to_string()),
            source: src("openapi:api.yaml", "openapi"),
        }],
        converters: vec![ConverterFact {
            from: "user".to_string(),
            to: "user-dto".to_string(),
            converter_path: Some("crate::converters::user".to_string()),
            source: src("adr:0002", "adr"),
        }],
        modules: vec![ModuleOwnershipFact {
            module: "crate::user".to_string(),
            owner: Some("team-identity".to_string()),
            concept: Some("user".to_string()),
            source: src("adr:0003", "adr"),
        }],
        debts: vec![DebtFact {
            target: DebtTarget::Concept("user".to_string()),
            reason: "legacy adapter still in place".to_string(),
            issue: Some("#42".to_string()),
            expires: Some("2026-12-31".to_string()),
            source: src("adr:0004", "adr"),
        }],
        sources: vec![src("openapi:api.yaml", "openapi"), src("adr:0001", "adr")],
    }
}

#[test]
fn default_is_empty() {
    let facts = ArchitectureFacts::default();
    assert!(facts.is_empty());
}

#[test]
fn sort_is_idempotent() {
    let mut a = populated_facts();
    a.sort();
    let snapshot = a.clone();
    a.sort();
    assert_eq!(a, snapshot);
}

#[test]
fn sort_is_deterministic_regardless_of_insertion_order() {
    let c_alpha = concept("alpha", "adr:0001");
    let c_beta = concept("beta", "adr:0002");
    let c_gamma = concept("gamma", "adr:0003");
    let b_one = boundary("alpha-bdy", BoundaryKind::Http, "src:1");
    let b_two = boundary("beta-bdy", BoundaryKind::Cli, "src:2");

    let mut forward = ArchitectureFacts {
        concepts: vec![c_alpha.clone(), c_beta.clone(), c_gamma.clone()],
        boundaries: vec![b_one.clone(), b_two.clone()],
        ..ArchitectureFacts::default()
    };
    let mut reverse = ArchitectureFacts {
        concepts: vec![c_gamma, c_beta, c_alpha],
        boundaries: vec![b_two, b_one],
        ..ArchitectureFacts::default()
    };

    forward.sort();
    reverse.sort();
    assert_eq!(forward, reverse);
}

#[test]
fn extend_unions_all_fact_vectors() {
    let mut a = ArchitectureFacts {
        concepts: vec![concept("alpha", "adr:0001")],
        boundaries: vec![boundary("a-bdy", BoundaryKind::Http, "src:1")],
        sources: vec![src("adr:0001", "adr")],
        ..ArchitectureFacts::default()
    };
    let b = ArchitectureFacts {
        concepts: vec![concept("beta", "adr:0002")],
        boundaries: vec![boundary("b-bdy", BoundaryKind::Cli, "src:2")],
        contracts: vec![ContractFact {
            source_kind: "openapi".to_string(),
            operation: Some("op".to_string()),
            path: None,
            schema: None,
            source: src("openapi:x", "openapi"),
        }],
        converters: vec![ConverterFact {
            from: "alpha".to_string(),
            to: "beta".to_string(),
            converter_path: None,
            source: src("adr:0002", "adr"),
        }],
        modules: vec![ModuleOwnershipFact {
            module: "crate::beta".to_string(),
            owner: None,
            concept: Some("beta".to_string()),
            source: src("adr:0002", "adr"),
        }],
        debts: vec![DebtFact {
            target: DebtTarget::Boundary("b-bdy".to_string()),
            reason: "TODO".to_string(),
            issue: None,
            expires: None,
            source: src("adr:0002", "adr"),
        }],
        sources: vec![src("adr:0002", "adr")],
    };

    a.extend(b);
    assert_eq!(a.concepts.len(), 2);
    assert_eq!(a.boundaries.len(), 2);
    assert_eq!(a.contracts.len(), 1);
    assert_eq!(a.converters.len(), 1);
    assert_eq!(a.modules.len(), 1);
    assert_eq!(a.debts.len(), 1);
    assert_eq!(a.sources.len(), 2);
}

#[test]
fn extend_then_sort_is_commutative() {
    let a = populated_facts();
    let b = ArchitectureFacts {
        concepts: vec![concept("zeta", "adr:0099")],
        boundaries: vec![boundary("z-bdy", BoundaryKind::Ffi, "src:99")],
        sources: vec![src("adr:0099", "adr")],
        ..ArchitectureFacts::default()
    };

    let mut a_then_b = a.clone();
    a_then_b.extend(b.clone());
    a_then_b.sort();

    let mut b_then_a = b;
    b_then_a.extend(a);
    b_then_a.sort();

    assert_eq!(a_then_b, b_then_a);
}

#[test]
fn provenance_preserved_through_sort() {
    let s1 = src("adr:0001", "adr");
    let s2 = src("adr:0002", "adr");
    let mut facts = ArchitectureFacts {
        concepts: vec![
            ConceptFact {
                id: "beta".to_string(),
                source_of_truth: None,
                registry: None,
                source: s2.clone(),
            },
            ConceptFact {
                id: "alpha".to_string(),
                source_of_truth: None,
                registry: None,
                source: s1.clone(),
            },
        ],
        ..ArchitectureFacts::default()
    };
    facts.sort();
    assert_eq!(facts.concepts[0].id, "alpha");
    assert_eq!(facts.concepts[0].source, s1);
    assert_eq!(facts.concepts[1].id, "beta");
    assert_eq!(facts.concepts[1].source, s2);
}

#[test]
fn source_ref_round_trips_through_json() {
    let original = SourceRef {
        id: "openapi:api.yaml".to_string(),
        kind: "openapi".to_string(),
        path: Some("api/openapi.yaml".to_string()),
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let parsed: SourceRef = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed, original);
}

#[test]
fn architecture_facts_round_trips_through_json() {
    let original = populated_facts();
    let json = serde_json::to_string(&original).expect("serialize");
    let parsed: ArchitectureFacts = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed, original);
}

#[test]
fn boundary_kind_serializes_as_kebab_case() {
    let json = serde_json::to_string(&BoundaryKind::Persistence).expect("serialize");
    assert_eq!(json, "\"persistence\"");
    let parsed: BoundaryKind = serde_json::from_str("\"persistence\"").expect("deserialize");
    assert_eq!(parsed, BoundaryKind::Persistence);
}

#[test]
fn debt_target_uses_tagged_kind() {
    let target = DebtTarget::Concept("user".to_string());
    let json = serde_json::to_string(&target).expect("serialize");
    assert_eq!(json, r#"{"kind":"concept","value":"user"}"#);

    let parsed: DebtTarget = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed, target);

    // Round-trip the other variants too.
    let bdy = DebtTarget::Boundary("http-api".to_string());
    let bdy_json = serde_json::to_string(&bdy).expect("serialize");
    assert_eq!(bdy_json, r#"{"kind":"boundary","value":"http-api"}"#);

    let pol = DebtTarget::Policy("registry-integrity".to_string());
    let pol_json = serde_json::to_string(&pol).expect("serialize");
    assert_eq!(
        pol_json,
        r#"{"kind":"policy","value":"registry-integrity"}"#
    );
}
