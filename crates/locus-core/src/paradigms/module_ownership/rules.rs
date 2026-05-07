//! MO rule implementations. Stub.
//!
//! Follow the OT (`crates/locus-core/src/paradigms/one_truth/rules.rs`) and
//! DG (`crates/locus-core/src/paradigms/dependency_graph/rules.rs`) patterns
//! when adding rules: each rule is a `pub fn <prefix>001(...) -> Vec<Diagnostic>`,
//! lockfile-driven where possible, with both human and agent-strict severity
//! handling via `CheckMode::elevate`.
