# Architecture source loader registry — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Spec:** `docs/superpowers/specs/2026-05-14-arch-source-loader-registry-design.md`

**Goal:** Land sub-issue [#108](https://github.com/ztripez/locus/issues/108) of epic #106 — a deterministic loader registry that walks `.locus/arch.json` `sources`, dispatches to registered `ArchSourceLoader`s, merges emitted facts, and surfaces load failures as LOCUS006 advisory diagnostics.

**Architecture:** New module `locus_core::architecture::loader` houses the trait, registry, and runner. `governance::pipeline::run` gains a phase that invokes the runner between the legacy adapter and the policy chain. A new `ArchSourceHealthPolicy` owns LOCUS006 and stamps Advisory decisions on findings inserted by the runner.

**Tech Stack:** Rust, serde, existing locus governance infrastructure. No new external dependencies.

---

## File structure

| File | Status | Responsibility |
|------|--------|---------------|
| `crates/locus-core/src/architecture/loader/mod.rs` | NEW | Module root + re-exports |
| `crates/locus-core/src/architecture/loader/config.rs` | NEW | `ArchSourceConfig` |
| `crates/locus-core/src/architecture/loader/context.rs` | NEW | `ArchLoadContext` |
| `crates/locus-core/src/architecture/loader/result.rs` | NEW | `ArchLoadResult`, `ArchLoadError` |
| `crates/locus-core/src/architecture/loader/trait_.rs` | NEW | `ArchSourceLoader` trait |
| `crates/locus-core/src/architecture/loader/registry.rs` | NEW | `ArchSourceRegistry` |
| `crates/locus-core/src/architecture/loader/runner.rs` | NEW | `load_all` orchestrator + tests |
| `crates/locus-core/src/architecture/mod.rs` | MODIFY | `pub mod loader;` |
| `crates/locus-core/src/governance/arch.rs` | MODIFY | Add `sources` field to `ArchDeclaration` |
| `crates/locus-core/src/governance/pipeline.rs` | MODIFY | Run loader phase + attach facts to `GovernanceOutput` |
| `crates/locus-core/src/governance/registry.rs` | MODIFY | Register LOCUS006 and `ArchSourceHealthPolicy` |
| `crates/locus-core/src/governance/policies/mod.rs` | MODIFY | Re-export `ArchSourceHealthPolicy` |
| `crates/locus-core/src/governance/policies/arch_source_health.rs` | NEW | `ArchSourceHealthPolicy` + LOCUS006 logic + tests |
| `.locus/arch.json` | MODIFY | Add `"arch-source-health"` to policies |

---

## Task 1 — `ArchSourceConfig` and arch.json `sources` field

**Files:**
- Create: `crates/locus-core/src/architecture/loader/config.rs`
- Create: `crates/locus-core/src/architecture/loader/mod.rs`
- Modify: `crates/locus-core/src/architecture/mod.rs`
- Modify: `crates/locus-core/src/governance/arch.rs`

- [ ] **Step 1: Add module skeletons**

Create `crates/locus-core/src/architecture/loader/mod.rs`:

```rust
//! Architecture source loader registry (#108).
//!
//! Walks the `sources` declared in `.locus/arch.json`, dispatches each
//! to its matching `ArchSourceLoader`, and merges emitted facts into a
//! single `ArchitectureFacts`. Load failures surface as LOCUS006
//! advisory findings via `ArchSourceHealthPolicy`.

// locus: ot canonical

pub mod config;

pub use config::ArchSourceConfig;
```

Add to `crates/locus-core/src/architecture/mod.rs` (after the existing `pub mod source;` line):

```rust
pub mod loader;
```

- [ ] **Step 2: Write the failing config-type test**

Create `crates/locus-core/src/architecture/loader/config.rs`:

```rust
//! Configured architecture source — one entry from `.locus/arch.json`'s
//! `sources` array.

// locus: ot canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct ArchSourceConfig {
    pub kind: String,
    pub path: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_kind_and_path() {
        let json = r#"{"kind":"openapi","path":"docs/openapi.yaml"}"#;
        let cfg: ArchSourceConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.kind, "openapi");
        assert_eq!(cfg.path, "docs/openapi.yaml");
    }

    #[test]
    fn serializes_back_to_original_shape() {
        let cfg = ArchSourceConfig {
            kind: "markdown".into(),
            path: "docs/architecture/overview.md".into(),
        };
        let s = serde_json::to_string(&cfg).unwrap();
        assert_eq!(s, r#"{"kind":"markdown","path":"docs/architecture/overview.md"}"#);
    }
}
```

- [ ] **Step 3: Run config tests, expect PASS**

Run: `cargo test -p locus-core architecture::loader::config -- --nocapture`
Expected: 2 tests pass.

- [ ] **Step 4: Write failing arch.json `sources` test**

Add to `crates/locus-core/src/governance/arch.rs` (inside the existing `#[cfg(test)] mod tests`):

```rust
    #[test]
    fn arch_declaration_deserializes_sources() {
        use crate::architecture::loader::config::ArchSourceConfig;
        use std::io::Write;

        let dir = tempfile::tempdir().unwrap();
        let arch_dir = dir.path().join(".locus");
        std::fs::create_dir_all(&arch_dir).unwrap();
        let mut f = std::fs::File::create(arch_dir.join("arch.json")).unwrap();
        write!(
            f,
            r#"{{
                "policies": [],
                "concepts": [],
                "sources": [
                    {{ "kind": "openapi", "path": "docs/api.yaml" }},
                    {{ "kind": "markdown", "path": "docs/arch.md" }}
                ]
            }}"#
        )
        .unwrap();

        let outcome = ArchDeclaration::load(dir.path());
        match outcome {
            ArchLoadOutcome::Present(decl) => {
                assert_eq!(decl.sources.len(), 2);
                assert_eq!(
                    decl.sources[0],
                    ArchSourceConfig { kind: "openapi".into(), path: "docs/api.yaml".into() }
                );
                assert_eq!(
                    decl.sources[1],
                    ArchSourceConfig { kind: "markdown".into(), path: "docs/arch.md".into() }
                );
            }
            other => panic!("expected Present; got {other:?}"),
        }
    }

    #[test]
    fn arch_declaration_defaults_sources_to_empty() {
        let dir = tempfile::tempdir().unwrap();
        let arch_dir = dir.path().join(".locus");
        std::fs::create_dir_all(&arch_dir).unwrap();
        std::fs::write(
            arch_dir.join("arch.json"),
            r#"{"policies":[],"concepts":[]}"#,
        )
        .unwrap();

        match ArchDeclaration::load(dir.path()) {
            ArchLoadOutcome::Present(decl) => assert!(decl.sources.is_empty()),
            other => panic!("expected Present; got {other:?}"),
        }
    }
```

- [ ] **Step 5: Run tests to verify they fail**

Run: `cargo test -p locus-core governance::arch::tests::arch_declaration_deserializes_sources`
Expected: FAIL — `sources` field unknown on `ArchDeclaration`.

- [ ] **Step 6: Add the `sources` field**

In `crates/locus-core/src/governance/arch.rs`, modify the `ArchDeclaration` struct:

```rust
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ArchDeclaration {
    /// Names of governance policies the workspace expects to be active.
    /// Match `PolicyId` literals (e.g. `"registry-integrity"`).
    #[serde(default)]
    pub policies: Vec<String>,
    /// Declared architecture concepts. Each concept names its
    /// source-of-truth path (a trait/registry pair). Bypasses are
    /// surfaced by `ConceptSourceOfTruthPolicy` as LOCUS005.
    #[serde(default)]
    pub concepts: Vec<ConceptDeclaration>,
    /// Configured architecture source artifacts (OpenAPI, ADRs, ...).
    /// Walked by `architecture::loader::runner::load_all` to populate
    /// `GovernanceOutput.architecture_facts`. Empty preserves current
    /// behavior — no facts loaded, no findings emitted.
    #[serde(default)]
    pub sources: Vec<crate::architecture::loader::config::ArchSourceConfig>,
}
```

- [ ] **Step 7: Run tests to verify pass**

Run: `cargo test -p locus-core governance::arch::tests`
Expected: all arch tests pass including the new two.

- [ ] **Step 8: Commit**

```bash
git add crates/locus-core/src/architecture/loader/mod.rs \
        crates/locus-core/src/architecture/loader/config.rs \
        crates/locus-core/src/architecture/mod.rs \
        crates/locus-core/src/governance/arch.rs
git commit -m "feat(#108): add ArchSourceConfig and arch.json sources field"
```

---

## Task 2 — `ArchLoadError` and `ArchLoadResult`

**Files:**
- Create: `crates/locus-core/src/architecture/loader/result.rs`
- Modify: `crates/locus-core/src/architecture/loader/mod.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/locus-core/src/architecture/loader/result.rs`:

```rust
//! Per-source loader output: facts produced and errors that should
//! surface as governance findings.

// locus: ot canonical

use crate::architecture::ArchitectureFacts;
use crate::architecture::source::SourceRef;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ArchLoadResult {
    pub facts: ArchitectureFacts,
    pub errors: Vec<ArchLoadError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchLoadError {
    UnknownKind { kind: String, path: String },
    FileMissing { path: String },
    MalformedSource { path: String, detail: String },
    FactConflict {
        fact_kind: &'static str,
        id: String,
        sources: Vec<SourceRef>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_result_is_empty_and_error_free() {
        let r = ArchLoadResult::default();
        assert!(r.facts.is_empty());
        assert!(r.errors.is_empty());
    }

    #[test]
    fn error_variants_round_trip_via_clone_and_eq() {
        let e = ArchLoadError::UnknownKind {
            kind: "foo".into(),
            path: "bar.yaml".into(),
        };
        assert_eq!(e.clone(), e);

        let e = ArchLoadError::FileMissing { path: "x.md".into() };
        assert_eq!(e.clone(), e);

        let e = ArchLoadError::MalformedSource {
            path: "x.yaml".into(),
            detail: "expected mapping at line 3".into(),
        };
        assert_eq!(e.clone(), e);

        let e = ArchLoadError::FactConflict {
            fact_kind: "ConceptFact",
            id: "user".into(),
            sources: vec![SourceRef {
                id: "openapi:api.yaml".into(),
                kind: "openapi".into(),
                path: Some("docs/api.yaml".into()),
            }],
        };
        assert_eq!(e.clone(), e);
    }
}
```

Add `pub mod result;` and `pub use result::{ArchLoadError, ArchLoadResult};` to `architecture/loader/mod.rs`.

- [ ] **Step 2: Run tests, expect PASS**

Run: `cargo test -p locus-core architecture::loader::result`
Expected: 2 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/locus-core/src/architecture/loader/result.rs \
        crates/locus-core/src/architecture/loader/mod.rs
git commit -m "feat(#108): add ArchLoadResult and ArchLoadError types"
```

---

## Task 3 — `ArchSourceLoader` trait and `ArchLoadContext`

**Files:**
- Create: `crates/locus-core/src/architecture/loader/context.rs`
- Create: `crates/locus-core/src/architecture/loader/trait_.rs`
- Modify: `crates/locus-core/src/architecture/loader/mod.rs`

- [ ] **Step 1: Create the context type**

Create `crates/locus-core/src/architecture/loader/context.rs`:

```rust
//! Read-only context handed to each `ArchSourceLoader::load` invocation.

// locus: ot canonical

use std::path::Path;

pub struct ArchLoadContext<'a> {
    /// Workspace root the source path is resolved against. Loaders must
    /// not assume any cwd; always join `workspace_root` and
    /// `config.path` when opening files.
    pub workspace_root: &'a Path,
}
```

- [ ] **Step 2: Create the trait with a smoke test**

Create `crates/locus-core/src/architecture/loader/trait_.rs`:

```rust
//! `ArchSourceLoader` trait — the extension point for architecture
//! source ingestion.

// locus: ot canonical

use super::config::ArchSourceConfig;
use super::context::ArchLoadContext;
use super::result::ArchLoadResult;

pub trait ArchSourceLoader: Send + Sync {
    /// Loader kind tag the registry dispatches on (e.g. `"openapi"`).
    /// Must match the `kind` field on incoming `ArchSourceConfig`s.
    fn kind(&self) -> &'static str;

    /// Read the source at `config.path` and emit facts. Implementations
    /// should not panic on filesystem or parse errors; they should
    /// return them as `ArchLoadError` entries on the result.
    fn load(&self, config: &ArchSourceConfig, ctx: &ArchLoadContext<'_>) -> ArchLoadResult;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::architecture::ArchitectureFacts;
    use std::path::Path;

    struct StubLoader;
    impl ArchSourceLoader for StubLoader {
        fn kind(&self) -> &'static str { "stub" }
        fn load(&self, _: &ArchSourceConfig, _: &ArchLoadContext<'_>) -> ArchLoadResult {
            ArchLoadResult {
                facts: ArchitectureFacts::default(),
                errors: Vec::new(),
            }
        }
    }

    #[test]
    fn trait_object_is_constructible() {
        let l: Box<dyn ArchSourceLoader> = Box::new(StubLoader);
        assert_eq!(l.kind(), "stub");
        let cfg = ArchSourceConfig { kind: "stub".into(), path: "x".into() };
        let root = Path::new(".");
        let ctx = ArchLoadContext { workspace_root: root };
        let result = l.load(&cfg, &ctx);
        assert!(result.facts.is_empty());
        assert!(result.errors.is_empty());
    }
}
```

- [ ] **Step 3: Update module exports**

Update `crates/locus-core/src/architecture/loader/mod.rs`:

```rust
//! Architecture source loader registry (#108).
//!
//! Walks the `sources` declared in `.locus/arch.json`, dispatches each
//! to its matching `ArchSourceLoader`, and merges emitted facts into a
//! single `ArchitectureFacts`. Load failures surface as LOCUS006
//! advisory findings via `ArchSourceHealthPolicy`.

// locus: ot canonical

pub mod config;
pub mod context;
pub mod result;
pub mod trait_;

pub use config::ArchSourceConfig;
pub use context::ArchLoadContext;
pub use result::{ArchLoadError, ArchLoadResult};
pub use trait_::ArchSourceLoader;
```

- [ ] **Step 4: Run tests, expect PASS**

Run: `cargo test -p locus-core architecture::loader::trait_`
Expected: 1 test passes.

- [ ] **Step 5: Commit**

```bash
git add crates/locus-core/src/architecture/loader/context.rs \
        crates/locus-core/src/architecture/loader/trait_.rs \
        crates/locus-core/src/architecture/loader/mod.rs
git commit -m "feat(#108): add ArchSourceLoader trait and ArchLoadContext"
```

---

## Task 4 — `ArchSourceRegistry`

**Files:**
- Create: `crates/locus-core/src/architecture/loader/registry.rs`
- Modify: `crates/locus-core/src/architecture/loader/mod.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/locus-core/src/architecture/loader/registry.rs`:

```rust
//! `ArchSourceRegistry` — dispatches configured architecture sources to
//! their matching `ArchSourceLoader` by `kind` tag.

// locus: ot canonical

use super::trait_::ArchSourceLoader;

pub struct ArchSourceRegistry {
    loaders: Vec<Box<dyn ArchSourceLoader>>,
}

impl ArchSourceRegistry {
    /// v1 ships empty. Concrete loaders (OpenAPI #109, markdown ADRs, ...)
    /// register themselves as they land.
    pub fn standard() -> Self {
        Self { loaders: Vec::new() }
    }

    pub fn register(&mut self, loader: Box<dyn ArchSourceLoader>) {
        self.loaders.push(loader);
    }

    pub fn find(&self, kind: &str) -> Option<&dyn ArchSourceLoader> {
        self.loaders
            .iter()
            .find(|l| l.kind() == kind)
            .map(|l| l.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::architecture::ArchitectureFacts;
    use crate::architecture::loader::config::ArchSourceConfig;
    use crate::architecture::loader::context::ArchLoadContext;
    use crate::architecture::loader::result::ArchLoadResult;

    struct K(&'static str);
    impl ArchSourceLoader for K {
        fn kind(&self) -> &'static str { self.0 }
        fn load(&self, _: &ArchSourceConfig, _: &ArchLoadContext<'_>) -> ArchLoadResult {
            ArchLoadResult { facts: ArchitectureFacts::default(), errors: Vec::new() }
        }
    }

    #[test]
    fn standard_starts_empty() {
        let r = ArchSourceRegistry::standard();
        assert!(r.find("anything").is_none());
    }

    #[test]
    fn register_then_find_returns_matching_loader() {
        let mut r = ArchSourceRegistry::standard();
        r.register(Box::new(K("openapi")));
        r.register(Box::new(K("markdown")));
        assert!(r.find("openapi").is_some());
        assert!(r.find("markdown").is_some());
        assert!(r.find("unknown").is_none());
    }

    #[test]
    fn find_returns_first_registered_for_duplicate_kinds() {
        let mut r = ArchSourceRegistry::standard();
        r.register(Box::new(K("openapi")));
        r.register(Box::new(K("openapi")));
        // First-wins; we don't yet de-dupe at register time. The runner
        // sees one loader per kind via `find` regardless.
        assert!(r.find("openapi").is_some());
    }
}
```

Add to `crates/locus-core/src/architecture/loader/mod.rs`:

```rust
pub mod registry;
pub use registry::ArchSourceRegistry;
```

- [ ] **Step 2: Run tests, expect PASS**

Run: `cargo test -p locus-core architecture::loader::registry`
Expected: 3 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/locus-core/src/architecture/loader/registry.rs \
        crates/locus-core/src/architecture/loader/mod.rs
git commit -m "feat(#108): add ArchSourceRegistry with kind-tag dispatch"
```

---

## Task 5 — `runner::load_all` — empty path and single-loader happy path

**Files:**
- Create: `crates/locus-core/src/architecture/loader/runner.rs`
- Modify: `crates/locus-core/src/architecture/loader/mod.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/locus-core/src/architecture/loader/runner.rs`:

```rust
//! Loader orchestrator. Walks every configured source through the
//! registry, merges facts, and emits errors for unknown kinds and
//! fact conflicts.

// locus: ot canonical

use super::config::ArchSourceConfig;
use super::context::ArchLoadContext;
use super::registry::ArchSourceRegistry;
use super::result::{ArchLoadError, ArchLoadResult};
use crate::architecture::ArchitectureFacts;

/// Walk `sources` in declaration order, dispatch each to its loader, and
/// fold facts + errors into a single `ArchLoadResult`. Determinism: the
/// only ordering signal is `sources`'s declared order; per-fact-type
/// vectors are sorted at the end via `ArchitectureFacts::sort`.
pub fn load_all(
    sources: &[ArchSourceConfig],
    registry: &ArchSourceRegistry,
    ctx: &ArchLoadContext<'_>,
) -> ArchLoadResult {
    let mut merged = ArchitectureFacts::default();
    let mut errors: Vec<ArchLoadError> = Vec::new();

    for cfg in sources {
        let Some(loader) = registry.find(&cfg.kind) else {
            errors.push(ArchLoadError::UnknownKind {
                kind: cfg.kind.clone(),
                path: cfg.path.clone(),
            });
            continue;
        };
        let r = loader.load(cfg, ctx);
        merged.extend(r.facts);
        errors.extend(r.errors);
    }

    merged.sort();
    ArchLoadResult { facts: merged, errors }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::architecture::concept::ConceptFact;
    use crate::architecture::loader::trait_::ArchSourceLoader;
    use crate::architecture::source::SourceRef;
    use std::path::Path;

    /// Yields canned facts on every load.
    struct MockLoader {
        kind: &'static str,
        facts: ArchitectureFacts,
    }
    impl ArchSourceLoader for MockLoader {
        fn kind(&self) -> &'static str { self.kind }
        fn load(&self, _: &ArchSourceConfig, _: &ArchLoadContext<'_>) -> ArchLoadResult {
            ArchLoadResult { facts: self.facts.clone(), errors: Vec::new() }
        }
    }

    fn concept(id: &str, source_id: &str) -> ConceptFact {
        ConceptFact {
            id: id.into(),
            source_of_truth: None,
            registry: None,
            source: SourceRef {
                id: source_id.into(),
                kind: "stub".into(),
                path: Some(format!("{source_id}.json")),
            },
        }
    }

    fn ctx<'a>() -> ArchLoadContext<'a> {
        ArchLoadContext { workspace_root: Path::new(".") }
    }

    #[test]
    fn empty_sources_returns_default_result() {
        let registry = ArchSourceRegistry::standard();
        let result = load_all(&[], &registry, &ctx());
        assert!(result.facts.is_empty());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn single_loader_returns_its_facts() {
        let mut facts = ArchitectureFacts::default();
        facts.concepts.push(concept("user", "openapi:api"));

        let mut registry = ArchSourceRegistry::standard();
        registry.register(Box::new(MockLoader { kind: "openapi", facts: facts.clone() }));

        let cfg = ArchSourceConfig { kind: "openapi".into(), path: "api.yaml".into() };
        let result = load_all(&[cfg], &registry, &ctx());

        assert_eq!(result.errors, Vec::new());
        assert_eq!(result.facts.concepts.len(), 1);
        assert_eq!(result.facts.concepts[0].id, "user");
    }
}
```

Add to `crates/locus-core/src/architecture/loader/mod.rs`:

```rust
pub mod runner;
pub use runner::load_all;
```

- [ ] **Step 2: Run tests, expect PASS**

Run: `cargo test -p locus-core architecture::loader::runner`
Expected: 2 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/locus-core/src/architecture/loader/runner.rs \
        crates/locus-core/src/architecture/loader/mod.rs
git commit -m "feat(#108): add runner::load_all empty + single-loader happy path"
```

---

## Task 6 — `runner::load_all` — unknown kind

**Files:**
- Modify: `crates/locus-core/src/architecture/loader/runner.rs`

- [ ] **Step 1: Write the failing test**

Append to the `#[cfg(test)] mod tests` block in `runner.rs`:

```rust
    #[test]
    fn unknown_kind_emits_unknown_kind_error() {
        let registry = ArchSourceRegistry::standard();
        let cfg = ArchSourceConfig { kind: "made-up".into(), path: "x.yaml".into() };
        let result = load_all(&[cfg], &registry, &ctx());

        assert!(result.facts.is_empty());
        assert_eq!(result.errors.len(), 1);
        assert_eq!(
            result.errors[0],
            ArchLoadError::UnknownKind {
                kind: "made-up".into(),
                path: "x.yaml".into(),
            }
        );
    }

    #[test]
    fn registered_and_unknown_sources_coexist() {
        let mut facts = ArchitectureFacts::default();
        facts.concepts.push(concept("user", "openapi:api"));

        let mut registry = ArchSourceRegistry::standard();
        registry.register(Box::new(MockLoader { kind: "openapi", facts }));

        let result = load_all(
            &[
                ArchSourceConfig { kind: "openapi".into(), path: "api.yaml".into() },
                ArchSourceConfig { kind: "made-up".into(), path: "x.yaml".into() },
            ],
            &registry,
            &ctx(),
        );

        assert_eq!(result.facts.concepts.len(), 1);
        assert_eq!(result.errors.len(), 1);
        assert!(matches!(
            result.errors[0],
            ArchLoadError::UnknownKind { .. }
        ));
    }
```

- [ ] **Step 2: Run, expect PASS** (the production code already handles unknown kinds)

Run: `cargo test -p locus-core architecture::loader::runner`
Expected: 4 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/locus-core/src/architecture/loader/runner.rs
git commit -m "test(#108): cover unknown-kind error in load_all"
```

---

## Task 7 — `runner::load_all` — bit-exact duplicate dedup

**Files:**
- Modify: `crates/locus-core/src/architecture/loader/runner.rs`

- [ ] **Step 1: Write the failing test**

Append to the runner tests block:

```rust
    #[test]
    fn bit_exact_duplicate_concept_is_deduped_silently() {
        let mut facts_a = ArchitectureFacts::default();
        facts_a.concepts.push(concept("user", "openapi:api"));
        let facts_b = facts_a.clone();

        let mut registry = ArchSourceRegistry::standard();
        registry.register(Box::new(MockLoader { kind: "src", facts: facts_a }));
        // Second registration would be shadowed, so use a kind alias.
        // Instead emit via a second source config that maps to a loader
        // that returns the same bit-exact concept.
        registry.register(Box::new(MockLoader { kind: "src2", facts: facts_b }));

        let result = load_all(
            &[
                ArchSourceConfig { kind: "src".into(), path: "a".into() },
                ArchSourceConfig { kind: "src2".into(), path: "b".into() },
            ],
            &registry,
            &ctx(),
        );

        assert_eq!(result.errors, Vec::new());
        assert_eq!(
            result.facts.concepts.len(),
            1,
            "bit-exact duplicate concept should collapse to one entry"
        );
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p locus-core architecture::loader::runner::tests::bit_exact_duplicate`
Expected: FAIL — the concept appears twice; current `load_all` doesn't dedup.

- [ ] **Step 3: Implement bit-exact dedup**

Add a private helper at the bottom of `runner.rs` and call it from `load_all` after `merged.sort()`:

```rust
fn dedup_bit_exact(facts: &mut ArchitectureFacts) {
    // Vectors are sorted; identical entries collapse via dedup.
    facts.concepts.dedup();
    facts.boundaries.dedup();
    facts.contracts.dedup();
    facts.converters.dedup();
    facts.modules.dedup();
    facts.debts.dedup();
    facts.sources.dedup();
}
```

Update `load_all`:

```rust
    merged.sort();
    dedup_bit_exact(&mut merged);
    ArchLoadResult { facts: merged, errors }
```

- [ ] **Step 4: Run the test to verify pass**

Run: `cargo test -p locus-core architecture::loader::runner`
Expected: 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/locus-core/src/architecture/loader/runner.rs
git commit -m "feat(#108): dedup bit-exact duplicate facts in load_all"
```

---

## Task 8 — `runner::load_all` — fact conflict detection

**Files:**
- Modify: `crates/locus-core/src/architecture/loader/runner.rs`

- [ ] **Step 1: Write the failing test**

Append to the runner tests block:

```rust
    #[test]
    fn same_concept_id_with_different_content_emits_fact_conflict() {
        // Two loaders emit a ConceptFact with the same `id` but
        // different `source_of_truth`.
        let mut a = ArchitectureFacts::default();
        a.concepts.push(ConceptFact {
            id: "user".into(),
            source_of_truth: Some("UserRecord".into()),
            registry: None,
            source: SourceRef {
                id: "openapi:api".into(),
                kind: "openapi".into(),
                path: Some("api.yaml".into()),
            },
        });
        let mut b = ArchitectureFacts::default();
        b.concepts.push(ConceptFact {
            id: "user".into(),
            source_of_truth: Some("UserModel".into()),
            registry: None,
            source: SourceRef {
                id: "markdown:arch".into(),
                kind: "markdown".into(),
                path: Some("arch.md".into()),
            },
        });

        let mut registry = ArchSourceRegistry::standard();
        registry.register(Box::new(MockLoader { kind: "openapi", facts: a }));
        registry.register(Box::new(MockLoader { kind: "markdown", facts: b }));

        let result = load_all(
            &[
                ArchSourceConfig { kind: "openapi".into(), path: "api.yaml".into() },
                ArchSourceConfig { kind: "markdown".into(), path: "arch.md".into() },
            ],
            &registry,
            &ctx(),
        );

        assert_eq!(result.errors.len(), 1, "expected one conflict finding");
        match &result.errors[0] {
            ArchLoadError::FactConflict { fact_kind, id, sources } => {
                assert_eq!(*fact_kind, "ConceptFact");
                assert_eq!(id, "user");
                assert_eq!(sources.len(), 2);
            }
            other => panic!("expected FactConflict; got {other:?}"),
        }
        // Keep-first semantics: first-declared wins.
        assert_eq!(result.facts.concepts.len(), 1);
        assert_eq!(
            result.facts.concepts[0].source_of_truth.as_deref(),
            Some("UserRecord"),
        );
    }
```

- [ ] **Step 2: Run, expect FAIL**

Run: `cargo test -p locus-core architecture::loader::runner::tests::same_concept_id_with_different_content`
Expected: FAIL — currently both concepts coexist.

- [ ] **Step 3: Implement conflict detection**

Add the conflict-detection helper to `runner.rs`. Replace the body of `load_all` so it does (a) per-loader fact accumulation, (b) per-vector conflict detection by identity, (c) sort + bit-exact dedup at the end.

```rust
/// Walk `sources` in declaration order, dispatch each to its loader, and
/// fold facts + errors into a single `ArchLoadResult`. Bit-exact
/// duplicates collapse silently; identity collisions (same fact id,
/// different content) emit `FactConflict`, keep first by source-config
/// order.
pub fn load_all(
    sources: &[ArchSourceConfig],
    registry: &ArchSourceRegistry,
    ctx: &ArchLoadContext<'_>,
) -> ArchLoadResult {
    let mut merged = ArchitectureFacts::default();
    let mut errors: Vec<ArchLoadError> = Vec::new();

    for cfg in sources {
        let Some(loader) = registry.find(&cfg.kind) else {
            errors.push(ArchLoadError::UnknownKind {
                kind: cfg.kind.clone(),
                path: cfg.path.clone(),
            });
            continue;
        };
        let r = loader.load(cfg, ctx);
        absorb_facts(&mut merged, r.facts, &mut errors);
        errors.extend(r.errors);
    }

    merged.sort();
    dedup_bit_exact(&mut merged);
    ArchLoadResult { facts: merged, errors }
}

/// Merge `incoming` into `target`, emitting `FactConflict` for any
/// incoming fact whose identity already exists in `target` with
/// different content. First-declared wins (the existing entry is kept;
/// the incoming entry is dropped).
fn absorb_facts(
    target: &mut crate::architecture::ArchitectureFacts,
    incoming: crate::architecture::ArchitectureFacts,
    errors: &mut Vec<ArchLoadError>,
) {
    use crate::architecture::{
        BoundaryFact, ConceptFact, ContractFact, ConverterFact, DebtFact, ModuleOwnershipFact,
    };

    let crate::architecture::ArchitectureFacts {
        concepts,
        boundaries,
        contracts,
        converters,
        modules,
        debts,
        sources,
    } = incoming;

    fn absorb_one<T, K>(
        target: &mut Vec<T>,
        incoming: Vec<T>,
        fact_kind: &'static str,
        identity: impl Fn(&T) -> K,
        id_display: impl Fn(&K) -> String,
        source_of: impl Fn(&T) -> crate::architecture::source::SourceRef,
        errors: &mut Vec<ArchLoadError>,
    ) where
        T: PartialEq,
        K: PartialEq,
    {
        for item in incoming {
            let new_id = identity(&item);
            if let Some(existing) = target.iter().find(|t| identity(t) == new_id) {
                if existing != &item {
                    errors.push(ArchLoadError::FactConflict {
                        fact_kind,
                        id: id_display(&new_id),
                        sources: vec![source_of(existing), source_of(&item)],
                    });
                }
                // existing wins, drop the incoming
            } else {
                target.push(item);
            }
        }
    }

    absorb_one(
        &mut target.concepts,
        concepts,
        "ConceptFact",
        |c: &ConceptFact| c.id.clone(),
        |id: &String| id.clone(),
        |c: &ConceptFact| c.source.clone(),
        errors,
    );
    absorb_one(
        &mut target.boundaries,
        boundaries,
        "BoundaryFact",
        |b: &BoundaryFact| b.id.clone(),
        |id: &String| id.clone(),
        |b: &BoundaryFact| b.source.clone(),
        errors,
    );
    absorb_one(
        &mut target.contracts,
        contracts,
        "ContractFact",
        |c: &ContractFact| (c.source_kind.clone(), c.operation.clone()),
        |id: &(String, Option<String>)| format!("{}:{:?}", id.0, id.1),
        |c: &ContractFact| c.source.clone(),
        errors,
    );
    absorb_one(
        &mut target.converters,
        converters,
        "ConverterFact",
        |c: &ConverterFact| (c.from.clone(), c.to.clone()),
        |id: &(String, String)| format!("{}→{}", id.0, id.1),
        |c: &ConverterFact| c.source.clone(),
        errors,
    );
    absorb_one(
        &mut target.modules,
        modules,
        "ModuleOwnershipFact",
        |m: &ModuleOwnershipFact| m.module.clone(),
        |id: &String| id.clone(),
        |m: &ModuleOwnershipFact| m.source.clone(),
        errors,
    );
    absorb_one(
        &mut target.debts,
        debts,
        "DebtFact",
        |d: &DebtFact| (d.target.clone(), d.reason.clone()),
        |id: &(crate::architecture::DebtTarget, String)| format!("{:?}|{}", id.0, id.1),
        |d: &DebtFact| d.source.clone(),
        errors,
    );

    // Source provenance refs: append-as-is (their own identity is the
    // SourceRef itself; bit-exact dedup at end handles collapsing).
    target.sources.extend(sources);
}
```

Important: this requires the existing fact types to expose the fields used (especially `ContractFact.operation`, `ConverterFact.from_concept`/`to_concept`, `DebtFact.reason`, etc.). Inspect the existing field names in `crates/locus-core/src/architecture/*.rs` before writing — adjust the identity closures if the actual field names differ. The compiler will catch any mismatch.

- [ ] **Step 4: Run, expect PASS**

Run: `cargo test -p locus-core architecture::loader::runner`
Expected: 6 tests pass. If any fail because of field-name mismatches in `absorb_one` identity closures, read the offending fact module and fix the closure.

- [ ] **Step 5: Commit**

```bash
git add crates/locus-core/src/architecture/loader/runner.rs
git commit -m "feat(#108): detect identity-collision fact conflicts in load_all"
```

---

## Task 9 — `runner::load_all` — determinism test

**Files:**
- Modify: `crates/locus-core/src/architecture/loader/runner.rs`

- [ ] **Step 1: Write the determinism test**

Append:

```rust
    #[test]
    fn declared_order_drives_keep_first_but_facts_sort_deterministically() {
        // Two sources that emit non-conflicting concepts. Declared in
        // opposite orders, the final fact vector should still be sorted
        // identically (ArchitectureFacts::sort runs at end).
        fn build() -> (ArchSourceRegistry, [ArchSourceConfig; 2]) {
            let mut a = ArchitectureFacts::default();
            a.concepts.push(concept("alpha", "src-a"));
            let mut b = ArchitectureFacts::default();
            b.concepts.push(concept("beta", "src-b"));
            let mut registry = ArchSourceRegistry::standard();
            registry.register(Box::new(MockLoader { kind: "ka", facts: a }));
            registry.register(Box::new(MockLoader { kind: "kb", facts: b }));
            (
                registry,
                [
                    ArchSourceConfig { kind: "ka".into(), path: "a".into() },
                    ArchSourceConfig { kind: "kb".into(), path: "b".into() },
                ],
            )
        }

        let (r1, cfgs1) = build();
        let result_fwd = load_all(&cfgs1, &r1, &ctx());

        let (r2, cfgs2) = build();
        let mut cfgs_reversed = cfgs2.to_vec();
        cfgs_reversed.reverse();
        let result_rev = load_all(&cfgs_reversed, &r2, &ctx());

        assert_eq!(
            result_fwd.facts, result_rev.facts,
            "sorted facts should be identical regardless of source declaration order"
        );
    }
```

- [ ] **Step 2: Run, expect PASS**

Run: `cargo test -p locus-core architecture::loader::runner`
Expected: 7 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/locus-core/src/architecture/loader/runner.rs
git commit -m "test(#108): verify load_all order-independence after sort"
```

---

## Task 10 — `ArchSourceHealthPolicy` (LOCUS006)

**Files:**
- Create: `crates/locus-core/src/governance/policies/arch_source_health.rs`
- Modify: `crates/locus-core/src/governance/policies/mod.rs`

- [ ] **Step 1: Inspect the existing policy export pattern**

Run: `cat crates/locus-core/src/governance/policies/mod.rs`
Look at how `RegistryCoherencePolicy` and `ConceptSourceOfTruthPolicy` are exported. The new module must follow the same `pub mod` + `pub use` pattern.

- [ ] **Step 2: Write the policy + tests**

Create `crates/locus-core/src/governance/policies/arch_source_health.rs`:

```rust
//! `ArchSourceHealthPolicy` — owner of LOCUS006.
//!
//! Decides RuleFindings produced by `architecture::loader::runner` into
//! Advisory diagnostics. Followups (#109+) will add concrete loaders;
//! this policy stays unchanged because it operates on the abstract
//! `ArchLoadError` shape via the runner-emitted findings already in the
//! store.

// locus: ot canonical

use crate::diagnostics::Severity;
use crate::governance::decision::{Decision, DecisionStatus, SeverityChange};
use crate::governance::finding::FindingSource;
use crate::governance::ids::PolicyId;
use crate::governance::policy::{PolicyContext, PolicyDefinition, PolicyOutput};

pub struct ArchSourceHealthPolicy;

pub const ARCH_SOURCE_HEALTH_ID: PolicyId = PolicyId::new("arch-source-health");

const LOCUS006: &str = "LOCUS006";

impl PolicyDefinition for ArchSourceHealthPolicy {
    fn id(&self) -> PolicyId { ARCH_SOURCE_HEALTH_ID }
    fn title(&self) -> &'static str { "Architecture Source Health" }

    fn decide(&self, ctx: &PolicyContext<'_>) -> PolicyOutput {
        let mut decisions = Vec::new();
        for f in ctx.findings.iter() {
            let owned_by_us = matches!(&f.source, FindingSource::Policy(p) if *p == ARCH_SOURCE_HEALTH_ID)
                || f.diagnostic_code.as_deref() == Some(LOCUS006);
            if !owned_by_us {
                continue;
            }
            decisions.push(Decision {
                finding_id: f.id,
                policy: ARCH_SOURCE_HEALTH_ID,
                severity: Severity::Advisory,
                status: DecisionStatus::KnownTransitionDebt,
                severity_change: SeverityChange::Unchanged,
                rationale: vec!["arch source load issue".into()],
            });
        }
        PolicyOutput { decisions, new_findings: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::architecture::loader::result::ArchLoadError;
    use crate::diagnostics::{CheckMode, Severity};
    use crate::governance::arch::ArchLoadOutcome;
    use crate::governance::finding::{FindingSource, FindingStore, RuleFinding};
    use crate::governance::ids::FindingIdMinter;
    use crate::governance::registry::{ParadigmRegistry, PolicyRegistry, RuleRegistry};
    use crate::lockfile::Lockfile;
    use locus_air::AirWorkspace;

    fn locus006_finding(minter: &FindingIdMinter, message: &str) -> RuleFinding {
        RuleFinding {
            id: minter.next(),
            source: FindingSource::Policy(ARCH_SOURCE_HEALTH_ID),
            rule_id: None,
            paradigm_id: None,
            default_severity: Severity::Advisory,
            span: None,
            concept: None,
            message: message.into(),
            evidence: Vec::new(),
            why: Vec::new(),
            suggested_fix: None,
            diagnostic_code: Some(LOCUS006.into()),
        }
    }

    fn run_policy_with(store: &FindingStore) -> PolicyOutput {
        let air = AirWorkspace::new(Vec::new());
        let lf = Lockfile::empty();
        let minter = FindingIdMinter::new();
        let prior: Vec<Decision> = Vec::new();
        let rules = RuleRegistry::standard();
        let paradigms = ParadigmRegistry::standard();
        let policies = PolicyRegistry::standard();
        let arch = ArchLoadOutcome::Missing;
        let ctx = PolicyContext {
            air: &air,
            lockfile: &lf,
            mode: CheckMode::Human,
            rule_registry: &rules,
            paradigm_registry: &paradigms,
            policy_registry: &policies,
            findings: store,
            prior_decisions: &prior,
            finding_ids: &minter,
            arch: &arch,
        };
        ArchSourceHealthPolicy.decide(&ctx)
    }

    #[test]
    fn empty_store_produces_no_decisions() {
        let store = FindingStore::new();
        let out = run_policy_with(&store);
        assert!(out.decisions.is_empty());
        assert!(out.new_findings.is_empty());
    }

    #[test]
    fn locus006_finding_gets_advisory_decision() {
        let minter = FindingIdMinter::new();
        let mut store = FindingStore::new();
        let f = locus006_finding(&minter, "arch.json source kind 'unknown' has no registered loader");
        let fid = f.id;
        store.insert(f);

        let out = run_policy_with(&store);
        assert_eq!(out.decisions.len(), 1);
        let d = &out.decisions[0];
        assert_eq!(d.finding_id, fid);
        assert_eq!(d.severity, Severity::Advisory);
        assert_eq!(d.policy, ARCH_SOURCE_HEALTH_ID);
        assert!(matches!(d.status, DecisionStatus::KnownTransitionDebt));
    }

    #[test]
    fn unrelated_findings_are_ignored() {
        let minter = FindingIdMinter::new();
        let mut store = FindingStore::new();
        store.insert(RuleFinding {
            id: minter.next(),
            source: FindingSource::Policy(PolicyId::new("registry-coherence")),
            rule_id: None,
            paradigm_id: None,
            default_severity: Severity::Advisory,
            span: None,
            concept: None,
            message: "unrelated".into(),
            evidence: Vec::new(),
            why: Vec::new(),
            suggested_fix: None,
            diagnostic_code: Some("LOCUS004".into()),
        });

        let out = run_policy_with(&store);
        assert!(out.decisions.is_empty(), "should not decide on LOCUS004 findings");
    }

    /// Message-shape sanity for each ArchLoadError variant. The policy
    /// itself doesn't render messages — the runner does — but tests pin
    /// that the canonical format strings stay stable across refactors.
    #[test]
    fn arch_load_error_message_shapes_are_stable() {
        let e = ArchLoadError::UnknownKind { kind: "openapi".into(), path: "x.yaml".into() };
        assert_eq!(
            format!("{}", arch_load_error_message(&e)),
            "arch.json source kind 'openapi' has no registered loader (path: x.yaml)"
        );
        let e = ArchLoadError::FileMissing { path: "a.md".into() };
        assert_eq!(
            arch_load_error_message(&e),
            "arch.json source path 'a.md' not found"
        );
        let e = ArchLoadError::MalformedSource { path: "x.yaml".into(), detail: "expected mapping".into() };
        assert_eq!(
            arch_load_error_message(&e),
            "arch.json source 'x.yaml' failed to parse: expected mapping"
        );
    }
}

/// Canonical message renderer for `ArchLoadError`. Pipeline phase calls
/// this when converting runner errors into `RuleFinding`s.
pub fn arch_load_error_message(e: &crate::architecture::loader::result::ArchLoadError) -> String {
    use crate::architecture::loader::result::ArchLoadError::*;
    match e {
        UnknownKind { kind, path } =>
            format!("arch.json source kind '{kind}' has no registered loader (path: {path})"),
        FileMissing { path } =>
            format!("arch.json source path '{path}' not found"),
        MalformedSource { path, detail } =>
            format!("arch.json source '{path}' failed to parse: {detail}"),
        FactConflict { fact_kind, id, sources } => {
            let src_list: Vec<String> = sources.iter().map(|s| s.id.clone()).collect();
            format!(
                "{fact_kind} '{id}' declared by multiple sources with conflicting content: {}",
                src_list.join(", ")
            )
        }
    }
}
```

Add to `crates/locus-core/src/governance/policies/mod.rs` (follow existing structure — `pub mod arch_source_health;` and re-export `ArchSourceHealthPolicy`):

```rust
pub mod arch_source_health;
pub use arch_source_health::{ArchSourceHealthPolicy, arch_load_error_message};
```

Also update `crates/locus-core/src/governance/mod.rs` re-export list to include `ArchSourceHealthPolicy` alongside the existing policies.

- [ ] **Step 3: Run policy tests**

Run: `cargo test -p locus-core governance::policies::arch_source_health`
Expected: 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/locus-core/src/governance/policies/arch_source_health.rs \
        crates/locus-core/src/governance/policies/mod.rs \
        crates/locus-core/src/governance/mod.rs
git commit -m "feat(#108): add ArchSourceHealthPolicy owning LOCUS006"
```

---

## Task 11 — Pipeline integration

**Files:**
- Modify: `crates/locus-core/src/governance/pipeline.rs`
- Modify: `crates/locus-core/src/governance/registry.rs`

- [ ] **Step 1: Register LOCUS006 and the policy**

In `crates/locus-core/src/governance/registry.rs`:

1. Find `GovernanceDiagnosticRegistry::standard()`. Add the LOCUS006 entry:

```rust
                ("LOCUS006", PolicyId::new("arch-source-health")),
```

after the existing `("LOCUS005", ...)` entry.

2. Find `PolicyRegistry::standard()`. Register `ArchSourceHealthPolicy` after `RegistryCoherencePolicy` (which mirrors run order). Use the existing pattern.

- [ ] **Step 2: Add architecture_facts to GovernanceOutput and run the loader**

In `crates/locus-core/src/governance/pipeline.rs`:

1. Add `use crate::architecture::ArchitectureFacts;` and any new imports needed.
2. Grow `GovernanceOutput`:

```rust
pub struct GovernanceOutput {
    pub diagnostics: Vec<Diagnostic>,
    pub emitted_decisions: Vec<Decision>,
    pub decisions: Vec<Decision>,
    pub findings: FindingStore,
    /// Architecture facts loaded from `.locus/arch.json` `sources`.
    /// Default-empty when no sources are configured. Populated by
    /// `architecture::loader::runner::load_all` during pipeline run.
    pub architecture_facts: ArchitectureFacts,
}
```

3. In `run_with_arch`, after `run_legacy_adapter` and before `run_policies`, run the loader:

```rust
    let architecture_facts = load_arch_sources(arch, workspace_root_hint, &mut store, &minter);
```

Add a new helper:

```rust
fn load_arch_sources(
    arch: &ArchLoadOutcome,
    workspace_root: Option<&Path>,
    store: &mut FindingStore,
    minter: &FindingIdMinter,
) -> ArchitectureFacts {
    use crate::architecture::loader::registry::ArchSourceRegistry;
    use crate::architecture::loader::runner::load_all;
    use crate::architecture::loader::context::ArchLoadContext;
    use crate::governance::policies::arch_source_health::{
        ARCH_SOURCE_HEALTH_ID, arch_load_error_message,
    };
    use crate::governance::finding::FindingSource;

    let ArchLoadOutcome::Present(decl) = arch else {
        return ArchitectureFacts::default();
    };
    if decl.sources.is_empty() {
        return ArchitectureFacts::default();
    }

    let registry = ArchSourceRegistry::standard();
    let root = workspace_root.unwrap_or_else(|| Path::new("."));
    let ctx = ArchLoadContext { workspace_root: root };
    let result = load_all(&decl.sources, &registry, &ctx);

    for err in &result.errors {
        store.insert(RuleFinding {
            id: minter.next(),
            source: FindingSource::Policy(ARCH_SOURCE_HEALTH_ID),
            rule_id: None,
            paradigm_id: None,
            default_severity: Severity::Advisory,
            span: None,
            concept: None,
            message: arch_load_error_message(err),
            evidence: Vec::new(),
            why: Vec::new(),
            suggested_fix: None,
            diagnostic_code: Some("LOCUS006".into()),
        });
    }

    result.facts
}
```

`Severity` needs `use crate::diagnostics::Severity;` if not already imported in pipeline.rs.

4. Plumb the workspace root through. The simplest cut: change `run_with_arch`'s signature does **not** change (it doesn't know the workspace root). Two-step fix:

   - In `run_with_workspace_root`, pass the workspace root through to `run_with_arch` via a new internal helper `run_inner`:

```rust
pub fn run_with_workspace_root(
    air: &AirWorkspace,
    lockfile: &Lockfile,
    mode: CheckMode,
    workspace_root: &Path,
) -> GovernanceOutput {
    let arch = ArchDeclaration::load(workspace_root);
    run_inner(air, lockfile, mode, &arch, Some(workspace_root))
}

pub fn run_with_arch(
    air: &AirWorkspace,
    lockfile: &Lockfile,
    mode: CheckMode,
    arch: &ArchLoadOutcome,
) -> GovernanceOutput {
    run_inner(air, lockfile, mode, arch, None)
}

fn run_inner(
    air: &AirWorkspace,
    lockfile: &Lockfile,
    mode: CheckMode,
    arch: &ArchLoadOutcome,
    workspace_root: Option<&Path>,
) -> GovernanceOutput {
    // existing body, but with the new `load_arch_sources(...)` call
    // wedged between `run_legacy_adapter` and `run_policies`
}
```

5. Return the new field on `GovernanceOutput`:

```rust
    GovernanceOutput {
        diagnostics,
        emitted_decisions,
        decisions,
        findings: store,
        architecture_facts,
    }
```

- [ ] **Step 3: Add a pipeline integration test**

Append to the `#[cfg(test)] mod tests` in `pipeline.rs`:

```rust
    #[test]
    fn pipeline_populates_default_architecture_facts_when_no_sources() {
        let air = AirWorkspace::new(Vec::new());
        let lf = Lockfile::empty();
        let out = run(&air, &lf, CheckMode::Human);
        assert!(out.architecture_facts.is_empty());
    }

    #[test]
    fn pipeline_emits_locus006_for_unknown_source_kind() {
        use crate::architecture::loader::config::ArchSourceConfig;
        use crate::governance::arch::{ArchDeclaration, ArchLoadOutcome};

        let air = AirWorkspace::new(Vec::new());
        let lf = Lockfile::empty();
        let decl = ArchDeclaration {
            policies: vec!["arch-source-health".into()],
            concepts: Vec::new(),
            sources: vec![ArchSourceConfig {
                kind: "definitely-not-registered".into(),
                path: "x.yaml".into(),
            }],
        };
        let out = run_with_arch(&air, &lf, CheckMode::Human, &ArchLoadOutcome::Present(decl));

        let locus006: Vec<_> = out
            .diagnostics
            .iter()
            .filter(|d| d.rule_id == "LOCUS006")
            .collect();
        assert_eq!(locus006.len(), 1, "expected exactly one LOCUS006 diagnostic");
        assert_eq!(locus006[0].severity, Severity::Advisory);
    }
```

The second test imports `Severity` — add `use crate::diagnostics::Severity;` to the test module if not already present.

- [ ] **Step 4: Run the test suite**

Run: `cargo test -p locus-core governance::pipeline`
Expected: all pipeline tests pass including the two new ones.

Run: `cargo test --workspace`
Expected: all workspace tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/locus-core/src/governance/pipeline.rs \
        crates/locus-core/src/governance/registry.rs
git commit -m "feat(#108): wire loader registry into governance pipeline"
```

---

## Task 12 — Dogfood update

**Files:**
- Modify: `.locus/arch.json`

- [ ] **Step 1: Add the new policy to the workspace declaration**

Read `.locus/arch.json`. Add `"arch-source-health"` to the `policies` array so registry coherence (LOCUS004) doesn't fire on the newly registered policy.

The file should look like:

```json
{
  "policies": [
    "registry-integrity",
    "registry-coherence",
    "concept-source-of-truth",
    "arch-source-health",
    "default-pass-through"
  ],
  "concepts": [ /* unchanged */ ]
}
```

(`sources` field stays absent — the workspace has no configured architecture sources yet.)

- [ ] **Step 2: Run self-check**

Run: `cargo run -p locus-cli -- check --workspace .`
Expected:
- Workspace check completes with exit 0.
- No new LOCUS004 finding for an unregistered policy.
- No LOCUS006 findings (since arch.json has no `sources`).

- [ ] **Step 3: Run dogfood under strict**

Run: `LOCUS_BASELINE=origin/main scripts/check-changed-strict.sh .` (if the script accepts arguments) or simply:

Run: `cargo run -p locus-cli -- check --workspace . --agent-strict --changed`
Expected: exit 0. The new policy is Advisory-only, so even under strict it should not block.

- [ ] **Step 4: Run full clippy and fmt**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add .locus/arch.json
git commit -m "feat(#108): declare arch-source-health policy in workspace arch.json"
```

---

## Task 13 — Final verification and PR

- [ ] **Step 1: Run full workspace tests**

Run: `cargo test --workspace --all-features`
Expected: all pass.

- [ ] **Step 2: Run self-check**

Run: `cargo run -p locus-cli -- check --workspace .`
Expected: same dogfood summary as the pre-PR baseline. Specifically:
- 0 active fatals
- No increase in warning count
- Locus004 advisory count unchanged
- One new entry in `GovernanceDiagnosticRegistry::standard()` (LOCUS006) but no LOCUS006 findings emitted (no sources configured)

- [ ] **Step 3: Spot-check determinism**

Run the runner determinism test once more:

Run: `cargo test -p locus-core architecture::loader::runner::tests::declared_order_drives_keep_first`
Expected: PASS.

- [ ] **Step 4: Acceptance-criteria walkthrough**

Map each AC from #108 to a passing test:

| AC | Test |
|----|------|
| Loader registry exists and can be invoked with zero or more source configs. | `runner::tests::empty_sources_returns_default_result`, `single_loader_returns_its_facts` |
| Loading is deterministic across platforms. | `runner::tests::declared_order_drives_keep_first_but_facts_sort_deterministically` |
| Missing or malformed sources are represented as structured errors/findings. | `runner::tests::unknown_kind_emits_unknown_kind_error`, `pipeline::tests::pipeline_emits_locus006_for_unknown_source_kind` |
| Multiple loaders can contribute to a single merged ArchitectureFacts value. | `runner::tests::single_loader_returns_its_facts` + `registered_and_unknown_sources_coexist` |
| Duplicate handling is explicit and tested. | `runner::tests::bit_exact_duplicate_concept_is_deduped_silently`, `same_concept_id_with_different_content_emits_fact_conflict` |
| No configured architecture sources preserves current behavior. | `pipeline::tests::pipeline_populates_default_architecture_facts_when_no_sources` |

If any row is missing a passing test, add one before opening the PR.

- [ ] **Step 5: Push and open PR**

```bash
git push -u origin <branch-name>
gh pr create --title "feat(#108): architecture source loader registry" --body "$(cat <<'EOF'
## Summary

Slice of epic #106 — sub-issue #108. Adds the deterministic loader registry that future format loaders (#109 OpenAPI, markdown ADRs, ...) plug into.

```text
.locus/arch.json [.sources] -> ArchSourceRegistry -> runner::load_all -> ArchitectureFacts + LOCUS006 findings
```

## What changed

- New module `locus_core::architecture::loader` housing `ArchSourceConfig`, `ArchSourceLoader` trait, `ArchSourceRegistry`, `ArchLoadContext`, `ArchLoadResult`/`ArchLoadError`, and `runner::load_all`.
- `ArchDeclaration` (in `governance/arch.rs`) gains a `sources: Vec<ArchSourceConfig>` field (`#[serde(default)]`).
- `GovernanceOutput` gains `architecture_facts: ArchitectureFacts` populated by the pipeline.
- New `ArchSourceHealthPolicy` (LOCUS006, Advisory by default) wired into `PolicyRegistry::standard()` and `GovernanceDiagnosticRegistry::standard()`.
- `.locus/arch.json` declares the new policy.

## Scope

Per the spec, this slice ships only the registry + pipeline hookup. Concrete loaders (#109 OpenAPI; markdown ADRs) and policy-context fact wiring are explicitly out of scope.

## Test plan

- [x] `cargo test --workspace`
- [x] `cargo run -p locus-cli -- check --workspace .` — dogfood summary unchanged
- [x] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [x] All six issue #108 ACs map to a passing test (see plan task 13).

Spec: `docs/superpowers/specs/2026-05-14-arch-source-loader-registry-design.md`
Plan: `docs/superpowers/plans/2026-05-14-arch-source-loader-registry.md`
EOF
)"
```

- [ ] **Step 6: Final task list checkbox sweep**

Walk the plan top-to-bottom and verify every `- [ ]` has been ticked. If any remain unchecked, return to that task.
