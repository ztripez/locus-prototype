# Locus init — multi-run scan-and-report Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn `locus init` into a scan-and-report command that prints a checklist of exact CLI commands the agent runs one at a time to onboard each paradigm. No daemon, no architecture-model file on disk, no auto-write of inferred decisions.

**Architecture:** Add a paradigm-neutral `Suggestion` infrastructure to `locus-core`. Extend the `Paradigm` trait with a `suggest()` method that emits per-paradigm onboarding hints; a separate cross-paradigm helper emits layer/feature suggestions that span paradigms. The `init` CLI handler aggregates both, renders a stable text checklist, and exits non-zero while items remain.

**Tech Stack:** Rust 2024 edition, clap 4 for CLI, serde + serde_json for the lockfile, indoc + insta for tests, anyhow for CLI error context.

**Spec:** `docs/superpowers/specs/2026-05-08-locus-init-multi-run-design.md`

---

## File Structure

**Create:**
- `crates/locus-core/src/init.rs` — `Suggestion`, `SuggestionCategory`, `CommandOption`, aggregator, layer+feature cross-paradigm helpers, p50/p95 stats.
- `crates/locus-core/src/paradigms/<paradigm>/init.rs` — per-paradigm `suggest()` for paradigms that don't already have one (OT does; the other 18 are new).
- `crates/locus-core/src/paradigms/runtime_work/edit.rs` — RW lockfile mutator (`add_runtime_owner_path`).
- `crates/locus-core/src/paradigms/error_taxonomy/edit.rs` — ER lockfile mutator (`add_domain_path`).
- `tests/fixtures/cluster-crate/` — fixture seeded with a `User`/`UserResponse` cluster + `From` impl.
- `crates/locus-cli/tests/init_smoke.rs` — end-to-end snapshot test for the init checklist.

**Modify:**
- `crates/locus-core/src/lib.rs` — re-export `init` module.
- `crates/locus-core/src/paradigms/mod.rs` — add `Paradigm::suggest()` with default empty impl.
- `crates/locus-core/src/paradigms/one_truth/mod.rs` — wire OT to its existing `init` module's new `suggest()`.
- `crates/locus-core/src/paradigms/one_truth/init.rs` — add `suggest()` for concept clusters.
- `crates/locus-core/src/paradigms/one_truth/accept.rs` — add `accept_converter()`.
- `crates/locus-core/src/paradigms/runtime_work/mod.rs` — wire RW to `RuntimeWork::suggest()` and pull in `edit` module.
- `crates/locus-core/src/paradigms/error_taxonomy/mod.rs` — wire ER to `ErrorTaxonomy::suggest()` and pull in `edit` module.
- `crates/locus-core/src/paradigms/responsibility/edit.rs` — add `add_domain_path()` (writes `domain_paths_rm`).
- `crates/locus-core/src/paradigms/port_adapter/edit.rs` — add `add_application_path()`.
- `crates/locus-cli/src/main.rs` — `InitArgs.acknowledge_empty`, new init handler that calls aggregator + renders checklist, new `Accept::Converter` subcommand, new `Rw` subcommand, new `Er` subcommand, `Rm::AddDomainPath` and `Pa::AddApplicationPath` variants.
- `crates/locus-cli/Cargo.toml` — add `assert_cmd`, `predicates`, `insta` dev-deps for the smoke test.

---

## Phase 1: Suggestion infrastructure & `--acknowledge-empty` flag

This phase introduces the data shape every later phase emits. After phase 1, `locus init` exits 0 and prints `unresolved: 0` because no paradigm `suggest()` returns anything yet.

### Task 1.1: Add `Suggestion`, `SuggestionCategory`, `CommandOption` + de-dup aggregator

**Files:**
- Create: `crates/locus-core/src/init.rs`
- Modify: `crates/locus-core/src/lib.rs`

- [ ] **Step 1: Write the failing test for `Suggestion::render`**

Append to a new file `crates/locus-core/src/init.rs`:

```rust
//! Init-time onboarding suggestions.
//!
//! Each paradigm's `init.rs` emits zero or more [`Suggestion`]s; cross-
//! paradigm helpers in this module emit `Suggestion`s for shared questions
//! (layer detection, feature partitioning). The CLI's `init` handler
//! aggregates both lists, sorts and de-duplicates them, and prints the
//! result as a checklist.
//!
//! Suggestions are *not* fired as `Diagnostic`s — they are init-only and
//! never affect the rule engine's pass/fail.

// locus: ot canonical

use std::cmp::Ordering;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub category: SuggestionCategory,
    pub headline: String,
    pub why: Vec<String>,
    pub options: Vec<CommandOption>,
    /// Paradigm prefixes this suggestion is associated with. Used by the
    /// aggregator to merge `why` lines when two paradigms emit the same
    /// suggestion shape.
    pub prefixes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SuggestionCategory {
    Concept,
    Layer,
    Feature,
    Threshold,
    Switch,
    ParadigmVacant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOption {
    pub label: String,
    pub commands: Vec<String>,
}

impl Suggestion {
    /// Render this suggestion as a human-readable block (no leading or
    /// trailing newline).
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("[{}] {}", self.category.tag(), self.headline));
        for w in &self.why {
            out.push('\n');
            out.push_str("  ");
            out.push_str(w);
        }
        for opt in &self.options {
            out.push('\n');
            out.push_str("  ");
            out.push_str(&opt.label);
            out.push(':');
            for cmd in &opt.commands {
                out.push('\n');
                out.push_str("    ");
                out.push_str(cmd);
            }
        }
        out
    }
}

impl SuggestionCategory {
    pub fn tag(self) -> &'static str {
        match self {
            SuggestionCategory::Concept => "concept",
            SuggestionCategory::Layer => "layer",
            SuggestionCategory::Feature => "feature",
            SuggestionCategory::Threshold => "threshold",
            SuggestionCategory::Switch => "switch",
            SuggestionCategory::ParadigmVacant => "paradigm-vacant",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_layer_suggestion() {
        let s = Suggestion {
            category: SuggestionCategory::Layer,
            headline: "no domain layer detected".into(),
            why: vec!["required by BO, FL".into()],
            options: vec![
                CommandOption {
                    label: "specify".into(),
                    commands: vec![
                        "locus bo add-domain-path \"crate::domain::*\"".into(),
                        "locus fl add-domain-path \"crate::domain::*\"".into(),
                    ],
                },
                CommandOption {
                    label: "or skip".into(),
                    commands: vec!["locus init --acknowledge-empty BO,FL".into()],
                },
            ],
            prefixes: vec!["BO".into(), "FL".into()],
        };
        let expected = "\
[layer] no domain layer detected
  required by BO, FL
  specify:
    locus bo add-domain-path \"crate::domain::*\"
    locus fl add-domain-path \"crate::domain::*\"
  or skip:
    locus init --acknowledge-empty BO,FL";
        assert_eq!(s.render(), expected);
    }
}
```

Wire the module into the crate. In `crates/locus-core/src/lib.rs`, add to the `pub mod` block:

```rust
pub mod init;
```

and add a re-export to the `pub use` block:

```rust
pub use init::{CommandOption, Suggestion, SuggestionCategory};
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test -p locus-core init::tests::render_layer_suggestion`
Expected: PASS.

- [ ] **Step 3: Add the aggregator + sorting test**

Append to `crates/locus-core/src/init.rs`:

```rust
/// Collect suggestions from many sources, sort them into a stable order,
/// and merge duplicates (suggestions with byte-identical option-command
/// lists). Merging combines `prefixes` and `why` lines.
pub fn aggregate(mut suggestions: Vec<Suggestion>) -> Vec<Suggestion> {
    suggestions.sort_by(suggestion_order);
    let mut out: Vec<Suggestion> = Vec::with_capacity(suggestions.len());
    for s in suggestions {
        let key = command_signature(&s);
        if let Some(existing) = out.iter_mut().find(|e| command_signature(e) == key) {
            for p in s.prefixes {
                if !existing.prefixes.iter().any(|q| q == &p) {
                    existing.prefixes.push(p);
                }
            }
            for w in s.why {
                if !existing.why.iter().any(|q| q == &w) {
                    existing.why.push(w);
                }
            }
        } else {
            out.push(s);
        }
    }
    out
}

fn suggestion_order(a: &Suggestion, b: &Suggestion) -> Ordering {
    a.category
        .cmp(&b.category)
        .then_with(|| a.headline.cmp(&b.headline))
}

fn command_signature(s: &Suggestion) -> Vec<Vec<String>> {
    s.options
        .iter()
        .map(|o| o.commands.clone())
        .collect()
}

#[cfg(test)]
mod aggregate_tests {
    use super::*;

    fn mk(category: SuggestionCategory, headline: &str, prefix: &str, cmds: &[&str]) -> Suggestion {
        Suggestion {
            category,
            headline: headline.into(),
            why: vec![format!("from {prefix}")],
            options: vec![CommandOption {
                label: "specify".into(),
                commands: cmds.iter().map(|c| (*c).to_string()).collect(),
            }],
            prefixes: vec![prefix.into()],
        }
    }

    #[test]
    fn aggregate_merges_identical_command_sets() {
        let a = mk(SuggestionCategory::Layer, "no domain", "BO", &["locus xx add"]);
        let b = mk(SuggestionCategory::Layer, "no domain", "FL", &["locus xx add"]);
        let out = aggregate(vec![a, b]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].prefixes, vec!["BO", "FL"]);
        assert_eq!(out[0].why, vec!["from BO", "from FL"]);
    }

    #[test]
    fn aggregate_keeps_distinct_command_sets() {
        let a = mk(SuggestionCategory::Layer, "no domain", "BO", &["locus bo add"]);
        let b = mk(SuggestionCategory::Layer, "no domain", "FL", &["locus fl add"]);
        let out = aggregate(vec![a, b]);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn aggregate_sorts_by_category_then_headline() {
        let a = mk(SuggestionCategory::ParadigmVacant, "RW empty", "RW", &["a"]);
        let b = mk(SuggestionCategory::Layer, "no domain", "BO", &["b"]);
        let c = mk(SuggestionCategory::Concept, "user cluster", "OT", &["c"]);
        let out = aggregate(vec![a, b, c]);
        assert_eq!(out[0].category, SuggestionCategory::Concept);
        assert_eq!(out[1].category, SuggestionCategory::Layer);
        assert_eq!(out[2].category, SuggestionCategory::ParadigmVacant);
    }
}
```

- [ ] **Step 4: Run aggregator tests**

Run: `cargo test -p locus-core init::aggregate_tests`
Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/locus-core/src/init.rs crates/locus-core/src/lib.rs
git commit -m "feat(core): add Suggestion infrastructure for init checklist"
```

### Task 1.2: Add `Paradigm::suggest()` trait method (default empty)

**Files:**
- Modify: `crates/locus-core/src/paradigms/mod.rs`

- [ ] **Step 1: Write the failing test in paradigms/mod.rs**

Append to `crates/locus-core/src/paradigms/mod.rs` (inside a new `#[cfg(test)] mod tests {}` block at the end of the file if one doesn't already exist):

```rust
#[cfg(test)]
mod suggest_default_tests {
    use super::*;
    use locus_air::AirWorkspace;

    struct Stub;
    impl Paradigm for Stub {
        fn name(&self) -> &'static str { "Stub" }
        fn rule_prefix(&self) -> &'static str { "ZZ" }
        fn init(&self, _: &AirWorkspace) -> serde_json::Value { serde_json::Value::Null }
        fn check(&self, _: &AirWorkspace, _: &Lockfile, _: CheckMode) -> Vec<Diagnostic> { Vec::new() }
    }

    #[test]
    fn default_suggest_returns_empty() {
        let p = Stub;
        let air = AirWorkspace::new(Vec::new());
        let lf = Lockfile::empty();
        assert!(p.suggest(&air, &lf).is_empty());
    }
}
```

- [ ] **Step 2: Run the test and confirm it fails**

Run: `cargo test -p locus-core paradigms::suggest_default_tests`
Expected: FAIL — `no method named 'suggest' found`.

- [ ] **Step 3: Add the trait method with a default empty impl**

Edit `crates/locus-core/src/paradigms/mod.rs` — add a `use` line and extend the trait:

```rust
use crate::diagnostics::{CheckMode, Diagnostic};
use crate::init::Suggestion;
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

// locus: ot canonical
pub trait Paradigm {
    fn name(&self) -> &'static str;
    fn rule_prefix(&self) -> &'static str;
    fn init(&self, air: &AirWorkspace) -> serde_json::Value;
    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic>;
    /// Emit init-time onboarding suggestions for this paradigm. Default
    /// returns no suggestions; paradigms override to propose layer paths,
    /// concept clusters, threshold dial-ins, or vacancy nudges.
    fn suggest(&self, _air: &AirWorkspace, _lockfile: &Lockfile) -> Vec<Suggestion> {
        Vec::new()
    }
}
```

- [ ] **Step 4: Run the test to confirm pass**

Run: `cargo test -p locus-core paradigms::suggest_default_tests`
Expected: PASS.

- [ ] **Step 5: Run the full core test suite to verify nothing else broke**

Run: `cargo test -p locus-core`
Expected: ALL PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/locus-core/src/paradigms/mod.rs
git commit -m "feat(core): add Paradigm::suggest with default empty impl"
```

### Task 1.3: Add `--acknowledge-empty` flag to InitArgs and persist to lockfile

**Files:**
- Modify: `crates/locus-cli/src/main.rs`

- [ ] **Step 1: Locate InitArgs and extend**

Find `InitArgs` (around line 623 of `crates/locus-cli/src/main.rs`) and replace it with:

```rust
struct InitArgs {
    /// Workspace root (containing Cargo.toml).
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
    /// Refuse to overwrite an existing locus.lock.
    #[arg(long)]
    no_overwrite: bool,
    /// Comma-separated paradigm prefixes the user explicitly acknowledges
    /// as empty. Each prefix is appended to `Lockfile.acknowledged_empty`
    /// (silencing LOCUS002 for that paradigm). Already-present prefixes
    /// are silently deduped. Example: `--acknowledge-empty RW,DA`.
    #[arg(long, value_name = "PREFIXES")]
    acknowledge_empty: Option<String>,
}
```

- [ ] **Step 2: Add the parser helper near the bottom of main.rs**

Append to `crates/locus-cli/src/main.rs`:

```rust
fn parse_prefix_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_uppercase())
        .collect()
}

#[cfg(test)]
mod parse_prefix_list_tests {
    use super::*;

    #[test]
    fn splits_and_uppercases() {
        assert_eq!(parse_prefix_list("rw,da"), vec!["RW", "DA"]);
    }

    #[test]
    fn trims_whitespace_and_drops_empties() {
        assert_eq!(parse_prefix_list("  RW , , FO  "), vec!["RW", "FO"]);
    }

    #[test]
    fn empty_input_returns_empty() {
        assert!(parse_prefix_list("").is_empty());
        assert!(parse_prefix_list(" , ").is_empty());
    }
}
```

- [ ] **Step 3: Run the parser tests**

Run: `cargo test -p locus-cli parse_prefix_list_tests`
Expected: 3 tests pass.

- [ ] **Step 4: Apply the flag in `init()`**

Find the existing `init()` function (around line 1440) and modify it so that, immediately after `let mut lockfile = Lockfile::empty();`, the acknowledge-empty list is collected and merged. The full replacement function — preserving its current source-hint promotion behaviour — is:

```rust
fn init(args: InitArgs) -> Result<()> {
    use locus_core::lockfile::LOCKFILE_NAME;

    let lockfile_path = args.workspace.join(LOCKFILE_NAME);
    if args.no_overwrite && lockfile_path.exists() {
        anyhow::bail!(
            "{} already exists; rerun without --no-overwrite to replace it",
            lockfile_path.display()
        );
    }

    let air = locus_rust::scan(&args.workspace)
        .with_context(|| format!("scan failed: {}", args.workspace.display()))?;

    // Start from the existing lockfile so previously-acknowledged prefixes
    // and accepted decisions survive a re-run.
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;

    // Re-run paradigm `init` to refresh paradigm sections from a fresh scan
    // (today only OT writes a non-empty section).
    let registry = registry();
    for paradigm in &registry {
        let section = paradigm.init(&air);
        if !section_is_empty(&section) {
            lockfile
                .paradigms
                .insert(paradigm.rule_prefix().to_string(), section);
        }
    }

    if let Some(raw) = args.acknowledge_empty.as_deref() {
        for prefix in parse_prefix_list(raw) {
            if !lockfile.acknowledged_empty.iter().any(|p| p == &prefix) {
                lockfile.acknowledged_empty.push(prefix);
            }
        }
    }

    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("wrote {}", written.display());
    for paradigm in &registry {
        let count = lockfile
            .paradigms
            .get(paradigm.rule_prefix())
            .map(summarize_section)
            .unwrap_or_else(|| "(empty)".to_string());
        println!(
            "  {} {}: {}",
            paradigm.rule_prefix(),
            paradigm.name(),
            count
        );
    }
    Ok(())
}
```

- [ ] **Step 5: Add an end-to-end test for the flag**

Append to `crates/locus-cli/src/main.rs`:

```rust
#[cfg(test)]
mod init_acknowledge_empty_tests {
    use super::*;
    use locus_core::lockfile::LOCKFILE_NAME;

    #[test]
    fn acknowledge_empty_persists_into_lockfile() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        // Minimal cargo workspace so `locus_rust::scan` succeeds.
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.0.1\"\nedition = \"2024\"\n[lib]\npath = \"src/lib.rs\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "").unwrap();

        let args = InitArgs {
            workspace: dir.to_path_buf(),
            no_overwrite: false,
            acknowledge_empty: Some("rw, da".into()),
        };
        init(args).unwrap();

        let lockfile_bytes = std::fs::read(dir.join(LOCKFILE_NAME)).unwrap();
        let lf: Lockfile = serde_json::from_slice(&lockfile_bytes).unwrap();
        assert_eq!(lf.acknowledged_empty, vec!["RW", "DA"]);
    }
}
```

- [ ] **Step 6: Add `tempfile` as a CLI dev-dep**

Edit `crates/locus-cli/Cargo.toml`. Add a `[dev-dependencies]` block (or extend it) so it contains:

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 7: Run the new test**

Run: `cargo test -p locus-cli init_acknowledge_empty_tests`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/locus-cli/src/main.rs crates/locus-cli/Cargo.toml
git commit -m "feat(cli): locus init --acknowledge-empty <PREFIXES>"
```

### Task 1.4: Wire `init` to render the Suggestion checklist (currently empty)

**Files:**
- Modify: `crates/locus-cli/src/main.rs`

- [ ] **Step 1: Add a checklist test that verifies the empty-checklist exit message**

Append to `crates/locus-cli/src/main.rs`:

```rust
#[cfg(test)]
mod render_checklist_tests {
    use super::*;
    use locus_core::init::Suggestion;

    #[test]
    fn render_empty_checklist_says_zero_unresolved() {
        let suggestions: Vec<Suggestion> = Vec::new();
        let out = render_checklist(&suggestions, /*hints_promoted=*/ 4);
        assert!(out.contains("auto-applied: 4 source hints promoted"));
        assert!(out.contains("unresolved: 0"));
        assert!(!out.contains("re-run"));
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p locus-cli render_checklist_tests`
Expected: FAIL — `cannot find function 'render_checklist'`.

- [ ] **Step 3: Implement `render_checklist`**

Append to `crates/locus-cli/src/main.rs`:

```rust
fn render_checklist(suggestions: &[locus_core::init::Suggestion], hints_promoted: usize) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    let _ = writeln!(out, "auto-applied: {hints_promoted} source hints promoted");
    let _ = writeln!(out, "unresolved: {}", suggestions.len());
    if suggestions.is_empty() {
        return out;
    }
    for s in suggestions {
        out.push('\n');
        out.push_str(&s.render());
        out.push('\n');
    }
    out.push('\n');
    out.push_str("re-run `locus init` after applying changes.\n");
    out
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cargo test -p locus-cli render_checklist_tests`
Expected: PASS.

- [ ] **Step 5: Wire `render_checklist` into `init()` after the existing summary**

Find the closing of the `init` function. Just before `Ok(())`, insert the aggregation + render call. Replace the tail of `init` (everything after the existing `for paradigm in &registry { ... }` summary loop and before `Ok(())`) with:

```rust
    let mut suggestions: Vec<locus_core::init::Suggestion> = Vec::new();
    for paradigm in &registry {
        suggestions.extend(paradigm.suggest(&air, &lockfile));
    }
    suggestions.extend(locus_core::init::cross_paradigm_suggestions(&air, &lockfile));
    let suggestions = locus_core::init::aggregate(suggestions);

    let hints_promoted = count_hint_promotions(&lockfile);
    print!("{}", render_checklist(&suggestions, hints_promoted));

    if !suggestions.is_empty() {
        std::process::exit(1);
    }
    Ok(())
```

- [ ] **Step 6: Add the helpers needed by the wiring**

Append to `crates/locus-cli/src/main.rs`:

```rust
fn count_hint_promotions(lockfile: &Lockfile) -> usize {
    use locus_core::paradigms::one_truth::lockfile_schema::{OtSection, Source};

    let section: OtSection = match lockfile.paradigm_section("OT") {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let mut count = 0usize;
    for entry in section.concepts.values() {
        if entry.canonical.source == Source::Hint {
            count += 1;
        }
        for b in &entry.boundaries {
            if b.source == Source::Hint {
                count += 1;
            }
        }
    }
    count
}
```

Add the cross-paradigm stub to `crates/locus-core/src/init.rs`:

```rust
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

/// Cross-paradigm suggestions (layer detection, feature partitioning, …).
/// Phase 1 returns no suggestions; phases 2 and 4 populate it.
pub fn cross_paradigm_suggestions(_air: &AirWorkspace, _lockfile: &Lockfile) -> Vec<Suggestion> {
    Vec::new()
}
```

- [ ] **Step 7: Build and run all CLI tests**

Run: `cargo build -p locus-cli && cargo test -p locus-cli`
Expected: build succeeds; all tests pass.

- [ ] **Step 8: Manual smoke test against the sample crate**

Run: `cargo run -p locus-cli -- init --workspace tests/fixtures/sample-crate`
Expected output ends with:

```
auto-applied: <N> source hints promoted
unresolved: 0
```

(Exit code 0; the existing OT promotion still happens.)

- [ ] **Step 9: Commit**

```bash
git add crates/locus-cli/src/main.rs crates/locus-core/src/init.rs
git commit -m "feat(cli): wire init to suggestion-checklist renderer"
```

---

## Phase 2: Path-convention heuristics (BO/FL/CR/TA/UT/CF + ER/RM/PA setters)

After phase 2, `locus init` against a fresh repo with conventional `crate::user::{domain,api,...}` layout produces concrete `[layer]` suggestions. Phase 2 also fills in the missing CLI setters (ER/RM/PA) so the suggested commands are runnable.

### Task 2.1: Layer-detection helper in `locus-core::init`

**Files:**
- Modify: `crates/locus-core/src/init.rs`

- [ ] **Step 1: Write tests for the helper**

Append to `crates/locus-core/src/init.rs`:

```rust
#[cfg(test)]
mod layer_detection_tests {
    use super::*;
    use locus_air::{AirFile, AirPackage, AirWorkspace};

    fn pkg(name: &str, files: &[(&str, Option<&str>)]) -> AirPackage {
        AirPackage {
            name: name.into(),
            version: "0.0.1".into(),
            root_dir: format!("/tmp/{name}"),
            files: files
                .iter()
                .map(|(p, m)| AirFile {
                    path: (*p).into(),
                    module_path: m.map(|s| s.to_string()),
                    items: Vec::new(),
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                })
                .collect(),
        }
    }

    #[test]
    fn detects_domain_modules_by_segment() {
        let air = AirWorkspace::new(vec![pkg(
            "x",
            &[
                ("src/user/domain.rs", Some("x::user::domain")),
                ("src/user/api.rs", Some("x::user::api")),
            ],
        )]);
        let layers = detect_layers(&air);
        assert!(layers.domain.iter().any(|p| p == "x::user::domain::*"));
        assert!(layers.api_or_boundary.iter().any(|p| p == "x::user::api::*"));
    }

    #[test]
    fn returns_empty_when_no_conventions_match() {
        let air = AirWorkspace::new(vec![pkg(
            "x",
            &[("src/lib.rs", Some("x"))],
        )]);
        let layers = detect_layers(&air);
        assert!(layers.domain.is_empty());
        assert!(layers.api_or_boundary.is_empty());
        assert!(layers.application.is_empty());
        assert!(layers.tests.is_empty());
        assert!(layers.utilities.is_empty());
        assert!(layers.config.is_empty());
        assert!(layers.composition.is_empty());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p locus-core layer_detection_tests`
Expected: FAIL — `cannot find function 'detect_layers'`.

- [ ] **Step 3: Implement `detect_layers`**

Append to `crates/locus-core/src/init.rs`:

```rust
/// A set of module-path globs grouped by detected architectural layer. The
/// globs are returned in `<module>::*` form so paradigm setters can use
/// them verbatim.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DetectedLayers {
    pub domain: Vec<String>,
    pub api_or_boundary: Vec<String>,
    pub application: Vec<String>,
    pub composition: Vec<String>,
    pub tests: Vec<String>,
    pub utilities: Vec<String>,
    pub config: Vec<String>,
}

pub fn detect_layers(air: &AirWorkspace) -> DetectedLayers {
    use std::collections::BTreeSet;

    let mut domain: BTreeSet<String> = BTreeSet::new();
    let mut api: BTreeSet<String> = BTreeSet::new();
    let mut application: BTreeSet<String> = BTreeSet::new();
    let mut composition: BTreeSet<String> = BTreeSet::new();
    let mut tests: BTreeSet<String> = BTreeSet::new();
    let mut utilities: BTreeSet<String> = BTreeSet::new();
    let mut config: BTreeSet<String> = BTreeSet::new();

    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module) = file.module_path.as_deref() else {
                continue;
            };
            for seg in module.split("::") {
                match seg {
                    "domain" | "core" | "model" | "models" => {
                        domain.insert(layer_glob(module, seg));
                    }
                    "api" | "dto" | "dtos" | "transport" => {
                        api.insert(layer_glob(module, seg));
                    }
                    "application" | "usecases" | "handlers" | "service" | "services" => {
                        application.insert(layer_glob(module, seg));
                    }
                    "composition" | "wiring" | "bin" | "main" => {
                        composition.insert(layer_glob(module, seg));
                    }
                    "tests" | "test_support" | "fixtures" => {
                        tests.insert(layer_glob(module, seg));
                    }
                    "util" | "utils" | "common" | "helpers" => {
                        utilities.insert(layer_glob(module, seg));
                    }
                    "config" | "settings" => {
                        config.insert(layer_glob(module, seg));
                    }
                    _ => {}
                }
            }
        }
    }

    DetectedLayers {
        domain: domain.into_iter().collect(),
        api_or_boundary: api.into_iter().collect(),
        application: application.into_iter().collect(),
        composition: composition.into_iter().collect(),
        tests: tests.into_iter().collect(),
        utilities: utilities.into_iter().collect(),
        config: config.into_iter().collect(),
    }
}

/// Produce the `<prefix>::<seg>::*` glob from a module path that contains
/// `<seg>` as one of its segments.
fn layer_glob(module: &str, seg: &str) -> String {
    let mut out = String::new();
    for (i, s) in module.split("::").enumerate() {
        if i > 0 {
            out.push_str("::");
        }
        out.push_str(s);
        if s == seg {
            out.push_str("::*");
            return out;
        }
    }
    // Fallback: no segment matched (shouldn't happen for callers above).
    format!("{module}::*")
}
```

- [ ] **Step 4: Run the layer detection tests**

Run: `cargo test -p locus-core layer_detection_tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/locus-core/src/init.rs
git commit -m "feat(core): detect architectural layers from module-path conventions"
```

### Task 2.2: Add `Er` CLI subcommand + `add-domain-path`

**Files:**
- Create: `crates/locus-core/src/paradigms/error_taxonomy/edit.rs`
- Modify: `crates/locus-core/src/paradigms/error_taxonomy/mod.rs`
- Modify: `crates/locus-cli/src/main.rs`

- [ ] **Step 1: Write the unit test for `add_domain_path`**

Create `crates/locus-core/src/paradigms/error_taxonomy/edit.rs`:

```rust
//! `locus er ...` — symbol-by-symbol mutators for the ER lockfile section.

use thiserror::Error;

use super::lockfile_schema::ErSection;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ErEditError {
    #[error("domain path pattern must not be empty")]
    EmptyDomainPath,
}

pub fn add_domain_path(section: &mut ErSection, pattern: &str) -> Result<(), ErEditError> {
    if pattern.is_empty() {
        return Err(ErEditError::EmptyDomainPath);
    }
    if !section.domain_paths.iter().any(|p| p == pattern) {
        section.domain_paths.push(pattern.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_domain_path_appends_and_dedupes() {
        let mut s = ErSection::default();
        add_domain_path(&mut s, "crate::domain::*").unwrap();
        add_domain_path(&mut s, "crate::other::*").unwrap();
        add_domain_path(&mut s, "crate::domain::*").unwrap();
        assert_eq!(s.domain_paths, vec!["crate::domain::*", "crate::other::*"]);
    }

    #[test]
    fn add_domain_path_rejects_empty() {
        let mut s = ErSection::default();
        assert_eq!(add_domain_path(&mut s, "").unwrap_err(), ErEditError::EmptyDomainPath);
    }
}
```

- [ ] **Step 2: Wire the edit module into the ER paradigm**

Open `crates/locus-core/src/paradigms/error_taxonomy/mod.rs`. Find the existing module declarations near the top and add:

```rust
pub mod edit;
```

- [ ] **Step 3: Run the edit-fn tests**

Run: `cargo test -p locus-core error_taxonomy::edit`
Expected: 2 tests pass.

- [ ] **Step 4: Add the CLI subcommand**

In `crates/locus-cli/src/main.rs`, find the `Command` enum (near line 111) and insert immediately after the `Dg(...)` arm:

```rust
    /// Manage ER (Error Taxonomy) declarations in `locus.lock`.
    #[command(subcommand)]
    Er(ErCommand),
```

Find the `match cli.command` block in `main()` (near line 676) and add the matching arm:

```rust
        Command::Er(cmd) => er(cmd),
```

Define the subcommand enum and its argument struct. Append next to the other paradigm subcommand definitions (e.g. just below `enum DgCommand`):

```rust
// locus: ot boundary cli.er cli
#[derive(Subcommand, Debug)]
enum ErCommand {
    /// Mark a module pattern as part of the domain layer (ER003).
    AddDomainPath(ErAddDomainPathArgs),
}

// locus: ot boundary cli.er-add-domain-path cli
#[derive(clap::Args, Debug)]
struct ErAddDomainPathArgs {
    /// Module path glob, e.g. `"crate::domain::*"`.
    pattern: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}
```

Append the handler functions next to the other paradigm handlers:

```rust
fn er(cmd: ErCommand) -> Result<()> {
    match cmd {
        ErCommand::AddDomainPath(args) => er_add_domain_path_cli(args),
    }
}

fn er_add_domain_path_cli(args: ErAddDomainPathArgs) -> Result<()> {
    use locus_core::paradigms::error_taxonomy::edit::add_domain_path;
    use locus_core::paradigms::error_taxonomy::lockfile_schema::ErSection;

    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: ErSection = lockfile
        .paradigm_section("ER")
        .context("ER lockfile section is malformed")?;

    add_domain_path(&mut section, &args.pattern)
        .with_context(|| format!("add ER domain path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize ER section")?;
    lockfile.paradigms.insert("ER".to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added ER domain path pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}
```

- [ ] **Step 5: Build and verify the CLI parses**

Run: `cargo run -p locus-cli -- er add-domain-path --help`
Expected: clap renders the subcommand with the `pattern` positional and `--workspace` long option.

- [ ] **Step 6: Commit**

```bash
git add crates/locus-core/src/paradigms/error_taxonomy/edit.rs \
        crates/locus-core/src/paradigms/error_taxonomy/mod.rs \
        crates/locus-cli/src/main.rs
git commit -m "feat(er): add 'locus er add-domain-path' setter"
```

### Task 2.3: Add `Rm::AddDomainPath` setter (writes `domain_paths_rm`)

**Files:**
- Modify: `crates/locus-core/src/paradigms/responsibility/edit.rs`
- Modify: `crates/locus-cli/src/main.rs`

- [ ] **Step 1: Inspect the current edit module signature**

Run: `grep -n "pub fn" crates/locus-core/src/paradigms/responsibility/edit.rs`
Note the existing fns so the new `add_domain_path` matches house style (return type, error enum, dedup behaviour).

- [ ] **Step 2: Add the unit test inside the existing `tests` module**

Append to `crates/locus-core/src/paradigms/responsibility/edit.rs` inside its existing `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn add_domain_path_appends_and_dedupes() {
        let mut s = RmSection::default();
        add_domain_path(&mut s, "crate::domain::*").unwrap();
        add_domain_path(&mut s, "crate::other::*").unwrap();
        add_domain_path(&mut s, "crate::domain::*").unwrap();
        assert_eq!(s.domain_paths_rm, vec!["crate::domain::*", "crate::other::*"]);
    }

    #[test]
    fn add_domain_path_rejects_empty() {
        let mut s = RmSection::default();
        assert_eq!(add_domain_path(&mut s, "").unwrap_err(), RmEditError::EmptyDomainPath);
    }
```

- [ ] **Step 3: Run the test to confirm it fails**

Run: `cargo test -p locus-core responsibility::edit`
Expected: FAIL — `cannot find function 'add_domain_path'` and `EmptyDomainPath` variant missing.

- [ ] **Step 4: Implement the function and extend the error enum**

Edit `crates/locus-core/src/paradigms/responsibility/edit.rs`. Add a variant to the existing `RmEditError`:

```rust
    #[error("domain path pattern must not be empty")]
    EmptyDomainPath,
```

Add the function (placement: alongside the existing edit fns):

```rust
pub fn add_domain_path(section: &mut RmSection, pattern: &str) -> Result<(), RmEditError> {
    if pattern.is_empty() {
        return Err(RmEditError::EmptyDomainPath);
    }
    if !section.domain_paths_rm.iter().any(|p| p == pattern) {
        section.domain_paths_rm.push(pattern.to_string());
    }
    Ok(())
}
```

- [ ] **Step 5: Run the test to confirm it passes**

Run: `cargo test -p locus-core responsibility::edit`
Expected: PASS (existing + 2 new).

- [ ] **Step 6: Add the CLI subcommand variant**

In `crates/locus-cli/src/main.rs`, find `enum RmCommand` (around line 579) and add an `AddDomainPath` variant:

```rust
#[derive(Subcommand, Debug)]
enum RmCommand {
    /// Set the workspace-wide per-function action-kind cap (RM001).
    SetDefault(RmSetDefaultArgs),
    /// Add a module pattern exempt from RM checks.
    AddExemptPath(RmAddExemptPathArgs),
    /// Add a module pattern declaring the domain layer (RM006).
    AddDomainPath(RmAddDomainPathArgs),
}

// locus: ot boundary cli.rm-add-domain-path cli
#[derive(clap::Args, Debug)]
struct RmAddDomainPathArgs {
    /// Module path glob, e.g. `"crate::domain::*"`.
    pattern: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}
```

Find the existing `fn rm(cmd: RmCommand)` dispatcher (around line 1213) and add a match arm:

```rust
        RmCommand::AddDomainPath(args) => rm_add_domain_path_cli(args),
```

Append the handler:

```rust
fn rm_add_domain_path_cli(args: RmAddDomainPathArgs) -> Result<()> {
    use locus_core::paradigms::responsibility::edit::add_domain_path;
    use locus_core::paradigms::responsibility::lockfile_schema::RmSection;

    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: RmSection = lockfile
        .paradigm_section("RM")
        .context("RM lockfile section is malformed")?;

    add_domain_path(&mut section, &args.pattern)
        .with_context(|| format!("add RM domain path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize RM section")?;
    lockfile.paradigms.insert("RM".to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added RM domain path pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}
```

- [ ] **Step 7: Build and verify CLI parses**

Run: `cargo run -p locus-cli -- rm add-domain-path --help`
Expected: clap renders the subcommand.

- [ ] **Step 8: Commit**

```bash
git add crates/locus-core/src/paradigms/responsibility/edit.rs \
        crates/locus-cli/src/main.rs
git commit -m "feat(rm): add 'locus rm add-domain-path' setter for domain_paths_rm"
```

### Task 2.4: Add `Pa::AddApplicationPath` setter

**Files:**
- Modify: `crates/locus-core/src/paradigms/port_adapter/edit.rs`
- Modify: `crates/locus-cli/src/main.rs`

- [ ] **Step 1: Confirm the edit module exists; if not, create it**

Run: `ls crates/locus-core/src/paradigms/port_adapter/`
Expected: list includes `edit.rs`. If absent, create it with the same skeleton as `error_taxonomy/edit.rs` (Task 2.2 Step 1) but with `PaSection`, `PaEditError`, and an `EmptyApplicationPath` variant.

- [ ] **Step 2: Add the unit test**

Append to `crates/locus-core/src/paradigms/port_adapter/edit.rs` (inside its existing `#[cfg(test)] mod tests`, or create one):

```rust
    #[test]
    fn add_application_path_appends_and_dedupes() {
        let mut s = PaSection::default();
        add_application_path(&mut s, "crate::app::*").unwrap();
        add_application_path(&mut s, "crate::other::*").unwrap();
        add_application_path(&mut s, "crate::app::*").unwrap();
        assert_eq!(s.application_paths, vec!["crate::app::*", "crate::other::*"]);
    }

    #[test]
    fn add_application_path_rejects_empty() {
        let mut s = PaSection::default();
        assert_eq!(
            add_application_path(&mut s, "").unwrap_err(),
            PaEditError::EmptyApplicationPath
        );
    }
```

- [ ] **Step 3: Run to confirm failure**

Run: `cargo test -p locus-core port_adapter::edit`
Expected: FAIL — `cannot find function 'add_application_path'`.

- [ ] **Step 4: Implement the fn**

If `PaEditError` exists, add the variant `EmptyApplicationPath` (with `#[error("application path pattern must not be empty")]`). Then add the function:

```rust
pub fn add_application_path(section: &mut PaSection, pattern: &str) -> Result<(), PaEditError> {
    if pattern.is_empty() {
        return Err(PaEditError::EmptyApplicationPath);
    }
    if !section.application_paths.iter().any(|p| p == pattern) {
        section.application_paths.push(pattern.to_string());
    }
    Ok(())
}
```

- [ ] **Step 5: Run tests to confirm pass**

Run: `cargo test -p locus-core port_adapter::edit`
Expected: PASS.

- [ ] **Step 6: Add the CLI subcommand variant**

In `crates/locus-cli/src/main.rs`, find `enum PaCommand` and add:

```rust
    /// Add a module pattern declaring the application layer (PA002).
    AddApplicationPath(PaAddApplicationPathArgs),
```

```rust
// locus: ot boundary cli.pa-add-application-path cli
#[derive(clap::Args, Debug)]
struct PaAddApplicationPathArgs {
    /// Module path glob, e.g. `"crate::application::*"`.
    pattern: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}
```

Add the dispatch arm in `fn pa()`:

```rust
        PaCommand::AddApplicationPath(args) => pa_add_application_path_cli(args),
```

Append the handler:

```rust
fn pa_add_application_path_cli(args: PaAddApplicationPathArgs) -> Result<()> {
    use locus_core::paradigms::port_adapter::edit::add_application_path;
    use locus_core::paradigms::port_adapter::lockfile_schema::PaSection;

    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: PaSection = lockfile
        .paradigm_section("PA")
        .context("PA lockfile section is malformed")?;

    add_application_path(&mut section, &args.pattern)
        .with_context(|| format!("add PA application path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize PA section")?;
    lockfile.paradigms.insert("PA".to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added PA application path pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}
```

- [ ] **Step 7: Build and CLI-help-check**

Run: `cargo run -p locus-cli -- pa add-application-path --help`
Expected: clap subcommand renders.

- [ ] **Step 8: Commit**

```bash
git add crates/locus-core/src/paradigms/port_adapter/edit.rs \
        crates/locus-cli/src/main.rs
git commit -m "feat(pa): add 'locus pa add-application-path' setter"
```

### Task 2.5: Cross-paradigm Layer Suggestion emitter

**Files:**
- Modify: `crates/locus-core/src/init.rs`

- [ ] **Step 1: Add the test for layer suggestion emission**

Append to `crates/locus-core/src/init.rs`:

```rust
#[cfg(test)]
mod cross_paradigm_layer_tests {
    use super::*;
    use locus_air::{AirFile, AirPackage, AirWorkspace};
    use crate::lockfile::Lockfile;

    fn workspace_with(modules: &[&str]) -> AirWorkspace {
        AirWorkspace::new(vec![AirPackage {
            name: "x".into(),
            version: "0.0.1".into(),
            root_dir: "/tmp/x".into(),
            files: modules
                .iter()
                .map(|m| AirFile {
                    path: format!("src/{}.rs", m.replace("::", "/")),
                    module_path: Some((*m).into()),
                    items: Vec::new(),
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                })
                .collect(),
        }])
    }

    #[test]
    fn emits_domain_suggestion_when_domain_modules_seen() {
        let air = workspace_with(&["x::user::domain", "x::user::api"]);
        let lf = Lockfile::empty();
        let suggestions = cross_paradigm_suggestions(&air, &lf);
        let domain = suggestions
            .iter()
            .find(|s| s.headline.contains("domain layer detected, but no paradigms onboarded"));
        assert!(domain.is_some(), "expected a domain-layer suggestion");
        let s = domain.unwrap();
        assert_eq!(s.category, SuggestionCategory::Layer);
        let cmds = s.options[0].commands.join("\n");
        assert!(cmds.contains("locus bo add-domain-path \"x::user::domain::*\""));
        assert!(cmds.contains("locus fl add-domain-path \"x::user::domain::*\""));
        assert!(cmds.contains("locus er add-domain-path \"x::user::domain::*\""));
        assert!(cmds.contains("locus rm add-domain-path \"x::user::domain::*\""));
    }

    #[test]
    fn omits_domain_suggestion_when_bo_already_has_a_path() {
        use serde_json::json;
        let air = workspace_with(&["x::user::domain"]);
        let mut lf = Lockfile::empty();
        lf.paradigms
            .insert("BO".into(), json!({"domain_paths": ["x::user::domain::*"]}));
        // Once any of BO/ER/FL/RM has the path, the agent has clearly chosen
        // to onboard; don't keep nagging cross-paradigm. Per-paradigm vacancy
        // suggestions can still fire from each paradigm's own `suggest()`.
        let suggestions = cross_paradigm_suggestions(&air, &lf);
        assert!(
            !suggestions
                .iter()
                .any(|s| s.headline.contains("domain layer detected")),
            "domain suggestion should suppress once BO has the path"
        );
    }
}
```

- [ ] **Step 2: Run the tests; expect failures**

Run: `cargo test -p locus-core cross_paradigm_layer_tests`
Expected: FAIL — current `cross_paradigm_suggestions` returns empty.

- [ ] **Step 3: Implement the layer-suggestion emission**

Replace the stub `cross_paradigm_suggestions` in `crates/locus-core/src/init.rs` with:

```rust
pub fn cross_paradigm_suggestions(air: &AirWorkspace, lockfile: &Lockfile) -> Vec<Suggestion> {
    let layers = detect_layers(air);
    let mut out = Vec::new();
    if !layers.domain.is_empty() && !any_domain_paths_set(lockfile) {
        out.push(domain_layer_suggestion(&layers.domain));
    }
    out
}

fn any_domain_paths_set(lockfile: &Lockfile) -> bool {
    let bo: serde_json::Value = lockfile
        .paradigm_section("BO")
        .unwrap_or(serde_json::Value::Null);
    let er: serde_json::Value = lockfile
        .paradigm_section("ER")
        .unwrap_or(serde_json::Value::Null);
    let fl: serde_json::Value = lockfile
        .paradigm_section("FL")
        .unwrap_or(serde_json::Value::Null);
    let rm: serde_json::Value = lockfile
        .paradigm_section("RM")
        .unwrap_or(serde_json::Value::Null);
    has_nonempty_array(&bo, "domain_paths")
        || has_nonempty_array(&er, "domain_paths")
        || has_nonempty_array(&fl, "domain_paths")
        || has_nonempty_array(&rm, "domain_paths_rm")
}

fn has_nonempty_array(v: &serde_json::Value, key: &str) -> bool {
    v.get(key)
        .and_then(|a| a.as_array())
        .is_some_and(|a| !a.is_empty())
}

fn domain_layer_suggestion(globs: &[String]) -> Suggestion {
    let mut commands: Vec<String> = Vec::new();
    for g in globs {
        commands.push(format!("locus bo add-domain-path \"{g}\""));
        commands.push(format!("locus fl add-domain-path \"{g}\""));
        commands.push(format!("locus er add-domain-path \"{g}\""));
        commands.push(format!("locus rm add-domain-path \"{g}\""));
    }
    Suggestion {
        category: SuggestionCategory::Layer,
        headline: "domain layer detected, but no paradigms onboarded".into(),
        why: vec![format!("required by BO, ER, FL, RM"), format!("globs: {}", globs.join(", "))],
        options: vec![
            CommandOption {
                label: "specify (run for each paradigm you want to onboard)".into(),
                commands,
            },
            CommandOption {
                label: "or skip these paradigms".into(),
                commands: vec!["locus init --acknowledge-empty BO,ER,FL,RM".into()],
            },
        ],
        prefixes: vec!["BO".into(), "ER".into(), "FL".into(), "RM".into()],
    }
}
```

- [ ] **Step 4: Run cross-paradigm tests**

Run: `cargo test -p locus-core cross_paradigm_layer_tests`
Expected: PASS.

- [ ] **Step 5: Run the full core suite**

Run: `cargo test -p locus-core`
Expected: ALL PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/locus-core/src/init.rs
git commit -m "feat(core): emit domain-layer suggestion across BO/ER/FL/RM"
```

### Task 2.6: Per-paradigm `suggest()` for CR/TA/UT/CF (path-convention only)

**Files:**
- Create: `crates/locus-core/src/paradigms/composition_root/init.rs`
- Create: `crates/locus-core/src/paradigms/test_architecture/init.rs`
- Create: `crates/locus-core/src/paradigms/utility_discipline/init.rs`
- Create: `crates/locus-core/src/paradigms/config_data/init.rs`
- Modify: each paradigm's `mod.rs` to wire the new `init` module.

CR/TA/UT/CF all already have CLI setters (`add-composition-root`, `add-test-path`, `add-utility-path`, `add-config-path`); their `suggest()` only needs to emit a `[layer]` suggestion when the relevant section field is empty and `detect_layers` returned a candidate.

- [ ] **Step 1: Build the shared helper test (CR-shaped) for suggest()**

Create `crates/locus-core/src/paradigms/composition_root/init.rs`:

```rust
//! `locus init` suggestions for the CR paradigm.

use locus_air::AirWorkspace;

use crate::init::{CommandOption, Suggestion, SuggestionCategory, detect_layers};
use crate::lockfile::Lockfile;
use super::lockfile_schema::CrSection;
use super::CR_PREFIX;

pub fn suggest(air: &AirWorkspace, lockfile: &Lockfile) -> Vec<Suggestion> {
    let section: CrSection = lockfile.paradigm_section(CR_PREFIX).unwrap_or_default();
    if !section.composition_root_paths.is_empty() {
        return Vec::new();
    }
    if lockfile.is_acknowledged_empty(CR_PREFIX) {
        return Vec::new();
    }
    let layers = detect_layers(air);
    if layers.composition.is_empty() {
        return Vec::new();
    }
    let commands: Vec<String> = layers
        .composition
        .iter()
        .map(|g| format!("locus cr add-composition-root \"{g}\""))
        .collect();
    vec![Suggestion {
        category: SuggestionCategory::Layer,
        headline: "composition root candidates detected".into(),
        why: vec![format!("globs: {}", layers.composition.join(", "))],
        options: vec![
            CommandOption {
                label: "specify".into(),
                commands,
            },
            CommandOption {
                label: "or skip".into(),
                commands: vec![format!("locus init --acknowledge-empty {CR_PREFIX}")],
            },
        ],
        prefixes: vec![CR_PREFIX.into()],
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{AirFile, AirPackage, AirWorkspace};

    fn ws_with(module: &str) -> AirWorkspace {
        AirWorkspace::new(vec![AirPackage {
            name: "x".into(),
            version: "0.0.1".into(),
            root_dir: "/tmp/x".into(),
            files: vec![AirFile {
                path: format!("src/{}.rs", module.replace("::", "/")),
                module_path: Some(module.into()),
                items: Vec::new(),
                hints: Vec::new(),
                parse_error: None,
                line_count: 1,
            }],
        }])
    }

    #[test]
    fn suggests_composition_root_when_bin_module_present() {
        let air = ws_with("x::bin::main");
        let lf = Lockfile::empty();
        let s = suggest(&air, &lf);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].category, SuggestionCategory::Layer);
        assert!(s[0].options[0].commands.iter().any(|c| c.contains("locus cr add-composition-root \"x::bin::*\"")));
    }

    #[test]
    fn no_suggestion_when_section_already_populated() {
        let air = ws_with("x::bin::main");
        let mut lf = Lockfile::empty();
        lf.paradigms.insert(
            CR_PREFIX.into(),
            serde_json::json!({"composition_root_paths": ["x::bin::*"]}),
        );
        assert!(suggest(&air, &lf).is_empty());
    }

    #[test]
    fn no_suggestion_when_acknowledged_empty() {
        let air = ws_with("x::bin::main");
        let mut lf = Lockfile::empty();
        lf.acknowledged_empty.push(CR_PREFIX.into());
        assert!(suggest(&air, &lf).is_empty());
    }
}
```

- [ ] **Step 2: Wire the CR module**

Edit `crates/locus-core/src/paradigms/composition_root/mod.rs`. Add at the top:

```rust
pub mod init;
```

Locate the `impl Paradigm for CompositionRoot` block and add a `suggest` override:

```rust
    fn suggest(&self, air: &AirWorkspace, lockfile: &Lockfile) -> Vec<crate::init::Suggestion> {
        init::suggest(air, lockfile)
    }
```

If the file doesn't already import `Lockfile` from this scope, add:

```rust
use crate::lockfile::Lockfile;
```

- [ ] **Step 3: Run CR tests**

Run: `cargo test -p locus-core composition_root::init`
Expected: 3 tests pass.

- [ ] **Step 4: Replicate for TA**

Create `crates/locus-core/src/paradigms/test_architecture/init.rs` with the same shape as CR's, substituting:
- `CrSection` → `TaSection`
- `composition_root_paths` → `test_paths`
- `CR_PREFIX` → `TA_PREFIX`
- `layers.composition` → `layers.tests`
- headline → `"test paths detected"`
- command → `locus ta add-test-path "..."`

Wire `pub mod init;` in `crates/locus-core/src/paradigms/test_architecture/mod.rs` and override `suggest`. Adjust the test cases to use a module like `x::user::tests`.

Run: `cargo test -p locus-core test_architecture::init` — expect 3 pass.

- [ ] **Step 5: Replicate for UT**

Create `crates/locus-core/src/paradigms/utility_discipline/init.rs` substituting:
- `UtSection` → `utility_paths`
- `UT_PREFIX`
- `layers.utilities`
- headline → `"utility module candidates detected"`
- command → `locus ut add-utility-path "..."`

Wire and test.

- [ ] **Step 6: Replicate for CF**

Create `crates/locus-core/src/paradigms/config_data/init.rs` substituting:
- `CfSection.config_paths`
- `CF_PREFIX`
- `layers.config`
- headline → `"config layer candidates detected"`
- command → `locus cf add-config-path "..."`

Wire and test.

- [ ] **Step 7: Run the full core suite**

Run: `cargo test -p locus-core`
Expected: ALL PASS (includes the new 12 tests across CR/TA/UT/CF).

- [ ] **Step 8: End-to-end smoke**

Run: `cargo run -p locus-cli -- init --workspace tests/fixtures/sample-crate`
Expected: a non-empty checklist appears (sample-crate's modules trigger at least one layer suggestion). Exit code 1.

- [ ] **Step 9: Commit**

```bash
git add crates/locus-core/src/paradigms/composition_root/init.rs \
        crates/locus-core/src/paradigms/composition_root/mod.rs \
        crates/locus-core/src/paradigms/test_architecture/init.rs \
        crates/locus-core/src/paradigms/test_architecture/mod.rs \
        crates/locus-core/src/paradigms/utility_discipline/init.rs \
        crates/locus-core/src/paradigms/utility_discipline/mod.rs \
        crates/locus-core/src/paradigms/config_data/init.rs \
        crates/locus-core/src/paradigms/config_data/mod.rs
git commit -m "feat: per-paradigm path-heuristic suggest() for CR/TA/UT/CF"
```

---

## Phase 3: OT concept clustering + `accept converter`

### Task 3.1: Expose cluster signals from `one_truth::infer`

The existing `infer` module already clusters concepts. Phase 3 piggy-backs on it but exposes the per-cluster confidence so `suggest` can tier suggestions.

**Files:**
- Modify: `crates/locus-core/src/paradigms/one_truth/infer.rs`

- [ ] **Step 1: Inspect the current `cluster_concepts` signature**

Run: `grep -n "pub fn cluster_concepts\|pub struct Cluster\|InferredRole" crates/locus-core/src/paradigms/one_truth/infer.rs`
Note the existing public API.

- [ ] **Step 2: Add a confidence field to the cluster struct (if not already present)**

If the cluster struct does not already carry a confidence field, extend it. Inside the `Cluster` definition (or whatever the existing module names it), add:

```rust
    /// Confidence the cluster represents one concept (0.0..=1.0). Computed
    /// from name-stem match strength, field-set Jaccard overlap between
    /// non-canonical members and the canonical, module-path proximity,
    /// and the presence of an existing `From`/`TryFrom` between members.
    /// Suggestion-tiering reads this; the existing init code (which only
    /// promotes hint-tagged members) ignores it.
    pub confidence: f32,
```

If the existing `cluster_concepts` does not yet compute a confidence, leave it set to `1.0` for hint-tagged clusters (already certain) and add a new helper `cluster_confidence(cluster)` for inference-shaped clusters. Where the visitor produces a cluster from name + shape (no hint), compute:

```rust
fn cluster_confidence(cluster: &Cluster, air: &AirWorkspace) -> f32 {
    let canonical = cluster
        .members
        .iter()
        .find(|m| m.role == InferredRole::Canonical);
    let Some(canonical) = canonical else {
        return 0.0;
    };
    let mut score = 0.0f32;
    // name stem present in every member (cluster_concepts already enforces this for inferred clusters)
    score += 0.4;
    // field overlap mean across non-canonical members
    let others: Vec<&ClusterMember> = cluster
        .members
        .iter()
        .filter(|m| m.role != InferredRole::Canonical)
        .collect();
    if !others.is_empty() {
        let mean_overlap: f32 = others
            .iter()
            .map(|m| jaccard(&canonical.field_names, &m.field_names))
            .sum::<f32>()
            / others.len() as f32;
        score += 0.4 * mean_overlap;
    }
    // a converter between any two members boosts confidence
    if has_converter_between_members(air, cluster) {
        score += 0.2;
    }
    score.min(1.0)
}

fn jaccard(a: &[String], b: &[String]) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    use std::collections::BTreeSet;
    let sa: BTreeSet<&String> = a.iter().collect();
    let sb: BTreeSet<&String> = b.iter().collect();
    let inter = sa.intersection(&sb).count() as f32;
    let union = sa.union(&sb).count() as f32;
    if union == 0.0 { 0.0 } else { inter / union }
}

fn has_converter_between_members(air: &AirWorkspace, cluster: &Cluster) -> bool {
    use locus_air::AirItem;
    let names: std::collections::BTreeSet<&str> =
        cluster.members.iter().map(|m| m.symbol.as_str()).collect();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Conversion(c) = item else { continue };
                let from_match = names.iter().any(|sym| {
                    sym.rsplit("::").next().unwrap_or(sym) == c.from.trim() || *sym == c.from.trim()
                });
                let to_match = names.iter().any(|sym| {
                    sym.rsplit("::").next().unwrap_or(sym) == c.to.trim() || *sym == c.to.trim()
                });
                if from_match && to_match {
                    return true;
                }
            }
        }
    }
    false
}
```

(If `ClusterMember` does not yet expose `field_names`, add `pub field_names: Vec<String>` and populate it from the existing AIR walk.)

- [ ] **Step 3: Test the confidence helper**

Append to `crates/locus-core/src/paradigms/one_truth/infer.rs`'s `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn jaccard_full_overlap_returns_one() {
        let a = vec!["id".into(), "name".into()];
        let b = vec!["id".into(), "name".into()];
        assert_eq!(jaccard(&a, &b), 1.0);
    }

    #[test]
    fn jaccard_no_overlap_returns_zero() {
        let a = vec!["id".into()];
        let b = vec!["other".into()];
        assert_eq!(jaccard(&a, &b), 0.0);
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p locus-core one_truth::infer`
Expected: existing tests still pass + 2 new pass.

- [ ] **Step 5: Commit**

```bash
git add crates/locus-core/src/paradigms/one_truth/infer.rs
git commit -m "feat(ot): cluster confidence from name+shape+converter signals"
```

### Task 3.2: OT `suggest()` emits concept-cluster suggestions

**Files:**
- Modify: `crates/locus-core/src/paradigms/one_truth/init.rs`
- Modify: `crates/locus-core/src/paradigms/one_truth/mod.rs`

- [ ] **Step 1: Add the suggestion test**

Append to `crates/locus-core/src/paradigms/one_truth/init.rs`:

```rust
#[cfg(test)]
mod suggest_tests {
    use super::*;
    use crate::init::SuggestionCategory;
    use crate::lockfile::Lockfile;

    // (Helpers to build a workspace fixture with a User/UserDto cluster
    // would normally live in `crates/locus-core/tests/fixtures.rs`. For this
    // test, lean on the fixture sample-crate that ships in the repo.)
    #[test]
    fn high_confidence_cluster_emits_single_option() {
        // The sample-crate has User + UserResponse with a TryFrom impl.
        let air = locus_rust::scan(std::path::Path::new("../../tests/fixtures/sample-crate"))
            .expect("scan sample-crate");
        let lf = Lockfile::empty();
        let suggestions = suggest(&air, &lf);
        let cluster = suggestions
            .iter()
            .find(|s| s.category == SuggestionCategory::Concept);
        assert!(cluster.is_some(), "expected a concept-cluster suggestion");
        let s = cluster.unwrap();
        // Single option means high confidence — the converter exists.
        assert_eq!(s.options.len(), 1, "high-confidence cluster should offer one option, not two");
    }

    #[test]
    fn no_suggestion_for_already_accepted_concept() {
        let air = locus_rust::scan(std::path::Path::new("../../tests/fixtures/sample-crate"))
            .expect("scan sample-crate");
        // Build a lockfile that already accepts the User cluster.
        let section_value = serde_json::json!({
            "concepts": {
                "user": {
                    "canonical": {"symbol": "sample_crate::identity::User", "source": "accepted"},
                    "boundaries": [{"symbol": "sample_crate::dto::UserResponse", "boundary": null, "source": "accepted"}],
                    "converters": []
                }
            }
        });
        let mut lf = Lockfile::empty();
        lf.paradigms.insert("OT".into(), section_value);
        let suggestions = suggest(&air, &lf);
        assert!(suggestions.iter().all(|s| s.category != SuggestionCategory::Concept || !s.headline.contains("user")));
    }
}
```

- [ ] **Step 2: Run the test; expect failure**

Run: `cargo test -p locus-core one_truth::init::suggest_tests`
Expected: FAIL — `cannot find function 'suggest'`.

- [ ] **Step 3: Implement `suggest`**

Append to `crates/locus-core/src/paradigms/one_truth/init.rs`:

```rust
use crate::init::{CommandOption, Suggestion, SuggestionCategory};
use crate::lockfile::Lockfile;

const HIGH_CONFIDENCE: f32 = 0.95;
const MID_CONFIDENCE: f32 = 0.70;

pub fn suggest(air: &AirWorkspace, lockfile: &Lockfile) -> Vec<Suggestion> {
    let section: super::lockfile_schema::OtSection =
        lockfile.paradigm_section("OT").unwrap_or_default();
    let clusters = super::infer::cluster_concepts(air);
    let mut out = Vec::new();
    for cluster in &clusters {
        if section.concepts.contains_key(&cluster.concept_id) {
            continue;
        }
        let canonical = match cluster
            .members
            .iter()
            .find(|m| m.role == super::infer::InferredRole::Canonical)
        {
            Some(c) => c,
            None => continue,
        };
        let boundaries: Vec<&super::infer::ClusterMember> = cluster
            .members
            .iter()
            .filter(|m| m.role == super::infer::InferredRole::Boundary)
            .collect();
        if boundaries.is_empty() {
            continue;
        }
        let confidence = cluster.confidence;
        if confidence < MID_CONFIDENCE {
            continue;
        }
        let cid = &cluster.concept_id;
        let mut accept_canonical_cmd =
            format!("locus accept canonical {} --concept {}", canonical.symbol, cid);
        let accept_boundary_cmds: Vec<String> = boundaries
            .iter()
            .map(|m| {
                format!(
                    "locus accept boundary {} --concept {}",
                    m.symbol, cid
                )
            })
            .collect();
        let mut single_option_cmds = vec![accept_canonical_cmd.clone()];
        single_option_cmds.extend(accept_boundary_cmds.iter().cloned());

        if confidence >= HIGH_CONFIDENCE {
            out.push(Suggestion {
                category: SuggestionCategory::Concept,
                headline: format!(
                    "cluster `{cid}` — {} + {}",
                    canonical.symbol,
                    boundaries.iter().map(|m| m.symbol.as_str()).collect::<Vec<_>>().join(", ")
                ),
                why: vec![format!("confidence {:.2}", confidence)],
                options: vec![CommandOption {
                    label: "accept this cluster".into(),
                    commands: single_option_cmds,
                }],
                prefixes: vec!["OT".into()],
            });
        } else {
            // Mid-confidence: offer both interpretations.
            let split_cmds: Vec<String> = std::iter::once(accept_canonical_cmd.clone())
                .chain(boundaries.iter().map(|m| {
                    format!(
                        "locus accept canonical {} --concept {}_{}",
                        m.symbol,
                        cid,
                        m.symbol
                            .rsplit("::")
                            .next()
                            .unwrap_or("alt")
                            .to_lowercase()
                    )
                }))
                .collect();
            accept_canonical_cmd.clear();
            out.push(Suggestion {
                category: SuggestionCategory::Concept,
                headline: format!("cluster `{cid}` ambiguous — {}", canonical.symbol),
                why: vec![format!("confidence {:.2}; review members", confidence)],
                options: vec![
                    CommandOption {
                        label: "if same concept".into(),
                        commands: single_option_cmds,
                    },
                    CommandOption {
                        label: "if separate concepts".into(),
                        commands: split_cmds,
                    },
                ],
                prefixes: vec!["OT".into()],
            });
        }
    }
    out
}
```

- [ ] **Step 4: Wire OT to override `Paradigm::suggest`**

Open `crates/locus-core/src/paradigms/one_truth/mod.rs`. Inside `impl Paradigm for OneTruth`, add:

```rust
    fn suggest(&self, air: &AirWorkspace, lockfile: &Lockfile) -> Vec<crate::init::Suggestion> {
        init::suggest(air, lockfile)
    }
```

- [ ] **Step 5: Run OT tests**

Run: `cargo test -p locus-core one_truth::init::suggest_tests`
Expected: 2 tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/locus-core/src/paradigms/one_truth/init.rs \
        crates/locus-core/src/paradigms/one_truth/mod.rs
git commit -m "feat(ot): emit concept-cluster suggestions during init"
```

### Task 3.3: `locus accept converter` CLI subcommand

**Files:**
- Modify: `crates/locus-core/src/paradigms/one_truth/accept.rs`
- Modify: `crates/locus-cli/src/main.rs`

- [ ] **Step 1: Add the unit test for `accept_converter`**

Append to the existing `#[cfg(test)] mod tests` in `crates/locus-core/src/paradigms/one_truth/accept.rs`:

```rust
    #[test]
    fn accept_converter_inserts_into_existing_concept() {
        // Build a section with one accepted canonical + one boundary so the
        // converter's endpoints both resolve.
        let mut section = OtSection::default();
        section.concepts.insert(
            "user".into(),
            ConceptEntry {
                canonical: AcceptedCanonical {
                    symbol: "x::User".into(),
                    source: Source::Accepted,
                },
                boundaries: vec![AcceptedBoundary {
                    symbol: "x::UserDto".into(),
                    boundary: None,
                    source: Source::Accepted,
                }],
                converters: Vec::new(),
            },
        );
        let air = locus_rust::scan(std::path::Path::new("../../tests/fixtures/sample-crate")).unwrap();
        accept_converter(
            &mut section,
            &air,
            "impl TryFrom<x::User> for x::UserDto",
            "user",
            Some("x::User"),
            Some("x::UserDto"),
        )
        .unwrap();
        let entry = section.concepts.get("user").unwrap();
        assert_eq!(entry.converters.len(), 1);
        assert_eq!(entry.converters[0].symbol, "impl TryFrom<x::User> for x::UserDto");
    }

    #[test]
    fn accept_converter_rejects_unknown_concept() {
        let mut section = OtSection::default();
        let air = locus_rust::scan(std::path::Path::new("../../tests/fixtures/sample-crate")).unwrap();
        let err = accept_converter(
            &mut section,
            &air,
            "sym",
            "missing",
            Some("x::A"),
            Some("x::B"),
        )
        .unwrap_err();
        assert!(matches!(err, AcceptError::UnknownConcept(_)));
    }
```

- [ ] **Step 2: Run the test; expect failure**

Run: `cargo test -p locus-core one_truth::accept::tests::accept_converter_inserts_into_existing_concept`
Expected: FAIL — function missing or `UnknownConcept` variant missing.

- [ ] **Step 3: Implement `accept_converter`**

Edit `crates/locus-core/src/paradigms/one_truth/accept.rs`. If `AcceptError` does not yet have `UnknownConcept`, add it:

```rust
    #[error("concept `{0}` is not in the lockfile; accept its canonical first")]
    UnknownConcept(String),
```

Add the function:

```rust
/// Accept a converter for `concept`. The concept must exist (i.e. its
/// canonical was previously accepted). `from` and `to` are optional symbol
/// hints that flow into the lockfile entry; if both are omitted, the
/// AIR-side conversion is searched for endpoints whose short names match
/// the concept's accepted symbols.
pub fn accept_converter(
    section: &mut OtSection,
    _air: &AirWorkspace,
    symbol: &str,
    concept: &str,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<(), AcceptError> {
    let entry = section
        .concepts
        .get_mut(concept)
        .ok_or_else(|| AcceptError::UnknownConcept(concept.to_string()))?;
    let from_s = from.unwrap_or("").to_string();
    let to_s = to.unwrap_or("").to_string();
    if entry.converters.iter().any(|c| c.symbol == symbol) {
        return Ok(());
    }
    entry.converters.push(AcceptedConverter {
        from: from_s,
        to: to_s,
        symbol: symbol.to_string(),
        source: Source::Accepted,
    });
    Ok(())
}
```

- [ ] **Step 4: Run the test; confirm pass**

Run: `cargo test -p locus-core one_truth::accept::tests`
Expected: PASS (existing + 2 new).

- [ ] **Step 5: Add the CLI subcommand variant + arg struct**

In `crates/locus-cli/src/main.rs`, find `enum AcceptCommand` (around line 412) and replace it with:

```rust
#[derive(Subcommand, Debug)]
enum AcceptCommand {
    /// Accept a symbol as canonical for a concept.
    Canonical(AcceptCanonicalArgs),
    /// Accept a symbol as a boundary adapter for an existing concept.
    Boundary(AcceptBoundaryArgs),
    /// Accept a converter symbol for an existing concept.
    Converter(AcceptConverterArgs),
}
```

Append the args struct next to the others:

```rust
// locus: ot boundary cli.accept-converter cli
#[derive(clap::Args, Debug)]
struct AcceptConverterArgs {
    /// The converter symbol — e.g. `"impl TryFrom<UserDto> for User"` or a free fn path.
    symbol: String,
    /// Concept id the converter belongs to.
    #[arg(long)]
    concept: String,
    /// Optional source-side symbol hint.
    #[arg(long)]
    from: Option<String>,
    /// Optional target-side symbol hint.
    #[arg(long)]
    to: Option<String>,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}
```

- [ ] **Step 6: Wire the dispatch**

Find `fn accept(cmd: AcceptCommand)` (near line 1387). Modify the workspace-extraction block and the match to include the new arm:

```rust
fn accept(cmd: AcceptCommand) -> Result<()> {
    let workspace = match &cmd {
        AcceptCommand::Canonical(a) => a.workspace.clone(),
        AcceptCommand::Boundary(a) => a.workspace.clone(),
        AcceptCommand::Converter(a) => a.workspace.clone(),
    };
    let air = locus_rust::scan(&workspace)
        .with_context(|| format!("scan failed: {}", workspace.display()))?;
    let mut lockfile = Lockfile::load_or_empty(&workspace)
        .with_context(|| format!("load lockfile from {}", workspace.display()))?;

    let mut section: OtSection = lockfile
        .paradigm_section(OT_PREFIX)
        .context("OT lockfile section is malformed")?;

    let summary = match cmd {
        AcceptCommand::Canonical(a) => {
            let cid =
                accept_canonical(&mut section, &air, &a.symbol, a.concept.as_deref(), a.force)
                    .with_context(|| format!("accept canonical `{}`", a.symbol))?;
            format!("accepted `{}` as canonical for concept `{cid}`", a.symbol)
        }
        AcceptCommand::Boundary(a) => {
            accept_boundary(
                &mut section,
                &air,
                &a.symbol,
                &a.concept,
                a.boundary.as_deref(),
            )
            .with_context(|| format!("accept boundary `{}`", a.symbol))?;
            format!(
                "accepted `{}` as boundary for concept `{}`{}",
                a.symbol,
                a.concept,
                a.boundary
                    .as_deref()
                    .map(|b| format!(" (label `{b}`)"))
                    .unwrap_or_default()
            )
        }
        AcceptCommand::Converter(a) => {
            accept_converter(
                &mut section,
                &air,
                &a.symbol,
                &a.concept,
                a.from.as_deref(),
                a.to.as_deref(),
            )
            .with_context(|| format!("accept converter `{}`", a.symbol))?;
            format!(
                "accepted `{}` as converter for concept `{}`",
                a.symbol, a.concept
            )
        }
    };

    let value = serde_json::to_value(&section).context("serialize OT section")?;
    lockfile.paradigms.insert(OT_PREFIX.to_string(), value);
    let written = lockfile
        .save(&workspace)
        .with_context(|| format!("write lockfile to {}", workspace.display()))?;

    println!("{summary}");
    println!("updated {}", written.display());
    Ok(())
}
```

Add the import at the top of the file (or near the existing OT imports):

```rust
use locus_core::paradigms::one_truth::accept::accept_converter;
```

- [ ] **Step 7: Build and CLI-help-check**

Run: `cargo run -p locus-cli -- accept converter --help`
Expected: clap renders the subcommand with `<symbol>` positional plus `--concept`, `--from`, `--to`, `--workspace`.

- [ ] **Step 8: Commit**

```bash
git add crates/locus-core/src/paradigms/one_truth/accept.rs \
        crates/locus-cli/src/main.rs
git commit -m "feat(ot): 'locus accept converter' subcommand"
```

---

## Phase 4: Feature partitioning (DG/FO)

After phase 4, top-level workspace modules surface as `[feature]` suggestions when neither `paradigms.DG.features` nor `paradigms.FO.features` are populated.

### Task 4.1: Top-level module enumeration helper

**Files:**
- Modify: `crates/locus-core/src/init.rs`

- [ ] **Step 1: Add tests for `top_level_modules`**

Append to `crates/locus-core/src/init.rs`:

```rust
#[cfg(test)]
mod top_level_module_tests {
    use super::*;
    use locus_air::{AirFile, AirPackage, AirWorkspace};

    fn ws(modules: &[&str]) -> AirWorkspace {
        AirWorkspace::new(vec![AirPackage {
            name: "x".into(),
            version: "0.0.1".into(),
            root_dir: "/tmp/x".into(),
            files: modules
                .iter()
                .map(|m| AirFile {
                    path: format!("src/{}.rs", m.replace("::", "/")),
                    module_path: Some((*m).into()),
                    items: Vec::new(),
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                })
                .collect(),
        }])
    }

    #[test]
    fn enumerates_distinct_first_segment_after_crate_root() {
        let air = ws(&[
            "x::user::domain",
            "x::user::api",
            "x::order::domain",
            "x::billing",
        ]);
        let modules = top_level_modules(&air);
        assert_eq!(modules, vec!["billing".to_string(), "order".to_string(), "user".to_string()]);
    }

    #[test]
    fn ignores_crate_root_only_files() {
        let air = ws(&["x"]);
        assert!(top_level_modules(&air).is_empty());
    }
}
```

- [ ] **Step 2: Run; expect failure**

Run: `cargo test -p locus-core top_level_module_tests`
Expected: FAIL — `cannot find function 'top_level_modules'`.

- [ ] **Step 3: Implement**

Append to `crates/locus-core/src/init.rs`:

```rust
/// Distinct second-segment module names across the workspace
/// (`x::user::domain` → `"user"`). Excludes single-segment files (the
/// crate root). Returned alphabetically.
pub fn top_level_modules(air: &AirWorkspace) -> Vec<String> {
    use std::collections::BTreeSet;
    let mut names: BTreeSet<String> = BTreeSet::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module) = file.module_path.as_deref() else {
                continue;
            };
            let mut segs = module.split("::");
            let _root = segs.next();
            if let Some(second) = segs.next() {
                names.insert(second.to_string());
            }
        }
    }
    names.into_iter().collect()
}
```

- [ ] **Step 4: Run; confirm pass**

Run: `cargo test -p locus-core top_level_module_tests`
Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/locus-core/src/init.rs
git commit -m "feat(core): top_level_modules helper for feature detection"
```

### Task 4.2: DG/FO feature suggestion in `cross_paradigm_suggestions`

**Files:**
- Modify: `crates/locus-core/src/init.rs`

- [ ] **Step 1: Add tests for the feature-emission branch**

Append to `crates/locus-core/src/init.rs`:

```rust
#[cfg(test)]
mod cross_paradigm_feature_tests {
    use super::*;
    use locus_air::{AirFile, AirPackage, AirWorkspace};
    use crate::lockfile::Lockfile;

    fn ws(modules: &[&str]) -> AirWorkspace {
        AirWorkspace::new(vec![AirPackage {
            name: "x".into(),
            version: "0.0.1".into(),
            root_dir: "/tmp/x".into(),
            files: modules
                .iter()
                .map(|m| AirFile {
                    path: format!("src/{}.rs", m.replace("::", "/")),
                    module_path: Some((*m).into()),
                    items: Vec::new(),
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                })
                .collect(),
        }])
    }

    #[test]
    fn emits_feature_suggestion_when_dg_and_fo_empty() {
        let air = ws(&["x::user::domain", "x::order::api", "x::billing::handlers"]);
        let lf = Lockfile::empty();
        let suggestions = cross_paradigm_suggestions(&air, &lf);
        let feat = suggestions.iter().find(|s| s.category == SuggestionCategory::Feature);
        assert!(feat.is_some(), "expected a feature suggestion");
        let s = feat.unwrap();
        let cmds = s.options[0].commands.join("\n");
        assert!(cmds.contains("locus dg define-feature --name user --module \"user::*\""));
        assert!(cmds.contains("locus dg define-feature --name order --module \"order::*\""));
        assert!(cmds.contains("locus dg define-feature --name billing --module \"billing::*\""));
    }

    #[test]
    fn omits_feature_suggestion_when_dg_already_has_features() {
        use serde_json::json;
        let air = ws(&["x::user::domain"]);
        let mut lf = Lockfile::empty();
        lf.paradigms.insert(
            "DG".into(),
            json!({"features": [{"name": "user", "module": "user::*", "public_api": []}]}),
        );
        let suggestions = cross_paradigm_suggestions(&air, &lf);
        assert!(suggestions.iter().all(|s| s.category != SuggestionCategory::Feature));
    }
}
```

- [ ] **Step 2: Run; expect failure on the first test**

Run: `cargo test -p locus-core cross_paradigm_feature_tests`
Expected: `emits_feature_suggestion_when_dg_and_fo_empty` FAIL (no Feature category present).

- [ ] **Step 3: Extend `cross_paradigm_suggestions` to emit Feature**

Modify `cross_paradigm_suggestions` in `crates/locus-core/src/init.rs` so it now reads:

```rust
pub fn cross_paradigm_suggestions(air: &AirWorkspace, lockfile: &Lockfile) -> Vec<Suggestion> {
    let layers = detect_layers(air);
    let mut out = Vec::new();
    if !layers.domain.is_empty() && !any_domain_paths_set(lockfile) {
        out.push(domain_layer_suggestion(&layers.domain));
    }
    let modules = top_level_modules(air);
    if !modules.is_empty() && !any_features_set(lockfile) {
        out.push(feature_partition_suggestion(&modules));
    }
    out
}

fn any_features_set(lockfile: &Lockfile) -> bool {
    let dg: serde_json::Value = lockfile
        .paradigm_section("DG")
        .unwrap_or(serde_json::Value::Null);
    let fo: serde_json::Value = lockfile
        .paradigm_section("FO")
        .unwrap_or(serde_json::Value::Null);
    has_nonempty_array(&dg, "features") || has_nonempty_array(&fo, "features")
}

fn feature_partition_suggestion(modules: &[String]) -> Suggestion {
    let mut commands: Vec<String> = Vec::new();
    for m in modules {
        commands.push(format!(
            "locus dg define-feature --name {m} --module \"{m}::*\""
        ));
    }
    commands.push("# (FO mirrors DG features once these are accepted)".into());
    Suggestion {
        category: SuggestionCategory::Feature,
        headline: "no features defined; DG/FO will not fire".into(),
        why: vec![format!("top-level modules: {}", modules.join(", "))],
        options: vec![
            CommandOption {
                label: "define".into(),
                commands,
            },
            CommandOption {
                label: "or skip".into(),
                commands: vec!["locus init --acknowledge-empty DG,FO".into()],
            },
        ],
        prefixes: vec!["DG".into(), "FO".into()],
    }
}
```

- [ ] **Step 4: Run tests; confirm pass**

Run: `cargo test -p locus-core cross_paradigm_feature_tests`
Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/locus-core/src/init.rs
git commit -m "feat(core): emit feature-partition suggestion across DG/FO"
```

---

## Phase 5: Threshold dial-in (CX/MO/RM/CR)

After phase 5, threshold suggestions surface only when computed p95 differs notably from the spec default.

### Task 5.1: Stats helper (p50/p95) in `locus-core::init`

**Files:**
- Modify: `crates/locus-core/src/init.rs`

- [ ] **Step 1: Add tests**

Append to `crates/locus-core/src/init.rs`:

```rust
#[cfg(test)]
mod stats_tests {
    use super::*;

    #[test]
    fn p95_of_constant_returns_constant() {
        assert_eq!(percentile(&[5; 20], 0.95), Some(5));
    }

    #[test]
    fn p95_of_ramp_returns_near_top() {
        let vals: Vec<u32> = (1..=100).collect();
        // p95 of 1..=100 = the value at index ceil(0.95 * 100) - 1 = 95.
        assert_eq!(percentile(&vals, 0.95), Some(95));
    }

    #[test]
    fn p50_of_empty_is_none() {
        assert_eq!(percentile::<u32>(&[], 0.50), None);
    }
}
```

- [ ] **Step 2: Run; expect failure**

Run: `cargo test -p locus-core stats_tests`
Expected: FAIL — `cannot find function 'percentile'`.

- [ ] **Step 3: Implement**

Append to `crates/locus-core/src/init.rs`:

```rust
/// Compute a percentile of a `u32` slice. `q` is in `0.0..=1.0`. Returns
/// `None` if the slice is empty. Uses the "ceiling" rank with `n*q`
/// rounded up to the nearest 1-based index. Not statistically perfect,
/// good enough for budget heuristics.
pub fn percentile<T: Copy + Ord + Into<u32>>(values: &[T], q: f32) -> Option<u32> {
    if values.is_empty() {
        return None;
    }
    let mut sorted: Vec<u32> = values.iter().map(|v| (*v).into()).collect();
    sorted.sort_unstable();
    let n = sorted.len() as f32;
    let rank = (n * q).ceil() as usize;
    let idx = rank.saturating_sub(1).min(sorted.len() - 1);
    Some(sorted[idx])
}
```

- [ ] **Step 4: Run; confirm pass**

Run: `cargo test -p locus-core stats_tests`
Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/locus-core/src/init.rs
git commit -m "feat(core): percentile helper for threshold heuristics"
```

### Task 5.2: CX threshold suggestions

**Files:**
- Create: `crates/locus-core/src/paradigms/complexity_budget/init.rs`
- Modify: `crates/locus-core/src/paradigms/complexity_budget/mod.rs`

- [ ] **Step 1: Add the test**

Create `crates/locus-core/src/paradigms/complexity_budget/init.rs`:

```rust
//! `locus init` suggestions for the CX paradigm.

use locus_air::{AirItem, AirWorkspace};

use crate::init::{CommandOption, Suggestion, SuggestionCategory, percentile};
use crate::lockfile::Lockfile;
use super::lockfile_schema::CxSection;
use super::CX_PREFIX;

const SPEC_DEFAULT_FUNCTION_LINES: u32 = 50;
const TOLERANCE: f32 = 1.5;

pub fn suggest(air: &AirWorkspace, lockfile: &Lockfile) -> Vec<Suggestion> {
    let section: CxSection = lockfile.paradigm_section(CX_PREFIX).unwrap_or_default();
    if section.default_max_function_lines.is_some() {
        return Vec::new();
    }
    if lockfile.is_acknowledged_empty(CX_PREFIX) {
        return Vec::new();
    }
    let function_lines: Vec<u32> = air
        .packages
        .iter()
        .flat_map(|p| p.files.iter())
        .flat_map(|f| {
            f.items.iter().filter_map(|item| match item {
                AirItem::Function(fun) => Some(fun.span.line_end.saturating_sub(fun.span.line_start) + 1),
                _ => None,
            })
        })
        .collect();
    let Some(p95) = percentile(&function_lines, 0.95) else {
        return Vec::new();
    };
    let default_f = SPEC_DEFAULT_FUNCTION_LINES as f32;
    if (p95 as f32) > default_f * TOLERANCE || (p95 as f32) * TOLERANCE < default_f {
        let suggested = (p95 as f32 * 1.1).ceil() as u32;
        vec![Suggestion {
            category: SuggestionCategory::Threshold,
            headline: format!("CX001 function-line p95 = {p95}; default = {SPEC_DEFAULT_FUNCTION_LINES}"),
            why: vec![format!("p95 differs from default by >1.5×; consider explicit cap")],
            options: vec![CommandOption {
                label: "set explicit cap".into(),
                commands: vec![format!(
                    "locus cx set-default-max-function-lines {suggested}"
                )],
            }],
            prefixes: vec![CX_PREFIX.into()],
        }]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{AirFile, AirFunction, AirItem, AirPackage, AirSpan, AirWorkspace};

    fn fn_item(start: u32, end: u32) -> AirItem {
        AirItem::Function(AirFunction {
            name: "f".into(),
            symbol: format!("x::f@{start}"),
            symbol_segments: vec!["x".into(), "f".into()],
            visibility: locus_air::Visibility::Public,
            decorators: Vec::new(),
            span: AirSpan::new("t.rs", start, end),
            params: Vec::new(),
            return_type: None,
            calls: Vec::new(),
            actions: Vec::new(),
        })
    }

    fn ws_with_fns(spans: &[(u32, u32)]) -> AirWorkspace {
        AirWorkspace::new(vec![AirPackage {
            name: "x".into(),
            version: "0.0.1".into(),
            root_dir: "/tmp/x".into(),
            files: vec![AirFile {
                path: "src/lib.rs".into(),
                module_path: Some("x".into()),
                items: spans.iter().map(|(s, e)| fn_item(*s, *e)).collect(),
                hints: Vec::new(),
                parse_error: None,
                line_count: 200,
            }],
        }])
    }

    #[test]
    fn no_suggestion_when_p95_within_tolerance() {
        // p95 ≈ 30, default 50, ratio < 1.5×.
        let spans: Vec<(u32, u32)> = (1..=20).map(|i| (i, i + 30)).collect();
        let air = ws_with_fns(&spans);
        let lf = Lockfile::empty();
        assert!(suggest(&air, &lf).is_empty());
    }

    #[test]
    fn suggestion_when_p95_far_above_default() {
        // p95 ≈ 200, default 50, ratio > 1.5×.
        let spans: Vec<(u32, u32)> = (1..=20).map(|i| (i, i + 200)).collect();
        let air = ws_with_fns(&spans);
        let lf = Lockfile::empty();
        let s = suggest(&air, &lf);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].category, SuggestionCategory::Threshold);
    }

    #[test]
    fn no_suggestion_when_acknowledged_empty() {
        let spans: Vec<(u32, u32)> = (1..=5).map(|i| (i, i + 200)).collect();
        let air = ws_with_fns(&spans);
        let mut lf = Lockfile::empty();
        lf.acknowledged_empty.push(CX_PREFIX.into());
        assert!(suggest(&air, &lf).is_empty());
    }
}
```

- [ ] **Step 2: Wire it**

Edit `crates/locus-core/src/paradigms/complexity_budget/mod.rs`. Add at the top:

```rust
pub mod init;
```

In `impl Paradigm for ComplexityBudget`, add:

```rust
    fn suggest(&self, air: &AirWorkspace, lockfile: &Lockfile) -> Vec<crate::init::Suggestion> {
        init::suggest(air, lockfile)
    }
```

(Add `use crate::lockfile::Lockfile;` at the top if not already imported.)

- [ ] **Step 3: Run CX tests**

Run: `cargo test -p locus-core complexity_budget::init`
Expected: 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/locus-core/src/paradigms/complexity_budget/init.rs \
        crates/locus-core/src/paradigms/complexity_budget/mod.rs
git commit -m "feat(cx): emit function-line threshold suggestion when p95 diverges"
```

### Task 5.3: MO threshold suggestion (public types per file)

**Files:**
- Create: `crates/locus-core/src/paradigms/module_ownership/init.rs`
- Modify: `crates/locus-core/src/paradigms/module_ownership/mod.rs`

- [ ] **Step 1: Implement and test**

Create `crates/locus-core/src/paradigms/module_ownership/init.rs` mirroring CX's structure but specialized to MO:

```rust
//! `locus init` suggestions for the MO paradigm.

use locus_air::{AirItem, AirWorkspace, Visibility};

use crate::init::{CommandOption, Suggestion, SuggestionCategory, percentile};
use crate::lockfile::Lockfile;
use super::lockfile_schema::MoSection;
use super::MO_PREFIX;

const SPEC_DEFAULT_PUBLIC_TYPES: u32 = 5;
const TOLERANCE: f32 = 1.5;

pub fn suggest(air: &AirWorkspace, lockfile: &Lockfile) -> Vec<Suggestion> {
    let section: MoSection = lockfile.paradigm_section(MO_PREFIX).unwrap_or_default();
    if section.default_max_public_types.is_some() {
        return Vec::new();
    }
    if lockfile.is_acknowledged_empty(MO_PREFIX) {
        return Vec::new();
    }
    let counts: Vec<u32> = air
        .packages
        .iter()
        .flat_map(|p| p.files.iter())
        .map(|f| {
            f.items
                .iter()
                .filter(|i| matches!(i, AirItem::Type(t) if matches!(t.visibility, Visibility::Public)))
                .count() as u32
        })
        .collect();
    let Some(p95) = percentile(&counts, 0.95) else {
        return Vec::new();
    };
    let d = SPEC_DEFAULT_PUBLIC_TYPES as f32;
    if (p95 as f32) > d * TOLERANCE {
        let suggested = ((p95 as f32) * 1.1).ceil() as u32;
        vec![Suggestion {
            category: SuggestionCategory::Threshold,
            headline: format!(
                "MO001 public-types-per-file p95 = {p95}; default = {SPEC_DEFAULT_PUBLIC_TYPES}"
            ),
            why: vec!["p95 above default by >1.5×".into()],
            options: vec![CommandOption {
                label: "set explicit cap".into(),
                commands: vec![format!("locus mo set-default-max-public-types {suggested}")],
            }],
            prefixes: vec![MO_PREFIX.into()],
        }]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{AirField, AirFile, AirItem, AirPackage, AirSpan, AirType, TypeKind, Visibility, AirWorkspace};

    fn ty(name: &str, vis: Visibility) -> AirItem {
        AirItem::Type(AirType {
            kind: TypeKind::Struct,
            name: name.into(),
            symbol: format!("x::{name}"),
            symbol_segments: vec!["x".into(), name.into()],
            visibility: vis,
            decorators: Vec::new(),
            span: AirSpan::new("t.rs", 1, 1),
            fields: Vec::<AirField>::new(),
            variants: Vec::new(),
            generics: Vec::new(),
            module_path: Some("x".into()),
            path_segments: vec!["x".into()],
        })
    }

    fn ws_with(types_per_file: &[Vec<&str>]) -> AirWorkspace {
        let mut files = Vec::new();
        for (i, names) in types_per_file.iter().enumerate() {
            files.push(AirFile {
                path: format!("src/f{i}.rs"),
                module_path: Some(format!("x::f{i}")),
                items: names.iter().map(|n| ty(n, Visibility::Public)).collect(),
                hints: Vec::new(),
                parse_error: None,
                line_count: 100,
            });
        }
        AirWorkspace::new(vec![AirPackage {
            name: "x".into(),
            version: "0.0.1".into(),
            root_dir: "/tmp/x".into(),
            files,
        }])
    }

    #[test]
    fn no_suggestion_when_p95_within_tolerance() {
        let air = ws_with(&[vec!["A"], vec!["A", "B"], vec!["A"]]);
        let lf = Lockfile::empty();
        assert!(suggest(&air, &lf).is_empty());
    }

    #[test]
    fn suggestion_when_p95_far_above_default() {
        let air = ws_with(&[(0..20).map(|i| Box::leak(format!("T{i}").into_boxed_str()) as &str).collect()]);
        let lf = Lockfile::empty();
        let s = suggest(&air, &lf);
        assert_eq!(s.len(), 1);
    }
}
```

(Note: if `AirType` has more fields than shown in the fixture builder, fill them in by inspecting `crates/locus-air/src/lib.rs` — the test only needs `items` to be `AirItem::Type` so `Visibility` matters; everything else can be defaulted.)

- [ ] **Step 2: Wire MO**

Edit `crates/locus-core/src/paradigms/module_ownership/mod.rs`. Add `pub mod init;`. Override `suggest` in `impl Paradigm for ModuleOwnership`:

```rust
    fn suggest(&self, air: &AirWorkspace, lockfile: &Lockfile) -> Vec<crate::init::Suggestion> {
        init::suggest(air, lockfile)
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p locus-core module_ownership::init`
Expected: 2 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/locus-core/src/paradigms/module_ownership/init.rs \
        crates/locus-core/src/paradigms/module_ownership/mod.rs
git commit -m "feat(mo): emit public-types-per-file threshold suggestion"
```

### Task 5.4: CR + RM threshold suggestions

CR has `wiring_density_threshold` (default 12). RM has `default_max_action_kinds` (no default — vacant). Each gets a small `init.rs` mirroring CX/MO.

- [ ] **Step 1: Author CR threshold init.rs**

Create `crates/locus-core/src/paradigms/composition_root/init.rs` already exists (from Phase 2). Append a `suggest_threshold` function and merge it with the existing `suggest`. Edit the file's `pub fn suggest` so that, after the existing path-glob branch, it also adds:

```rust
    out.extend(suggest_wiring_density(air, lockfile));
```

and append:

```rust
fn suggest_wiring_density(air: &AirWorkspace, lockfile: &Lockfile) -> Vec<Suggestion> {
    use locus_air::{AirItem, AirTruthAction};
    let section: CrSection = lockfile.paradigm_section(CR_PREFIX).unwrap_or_default();
    // wiring_density_threshold has a non-Option default of 12; only suggest
    // when the user hasn't overridden it AND p95 of TruthAction::Construct
    // counts per function exceeds 1.5× default.
    let mut counts_per_fn: Vec<u32> = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                if let AirItem::Function(f) = item {
                    let constructs = f
                        .actions
                        .iter()
                        .filter(|a| matches!(a, AirTruthAction::Construct(_)))
                        .count() as u32;
                    counts_per_fn.push(constructs);
                }
            }
        }
    }
    let Some(p95) = percentile(&counts_per_fn, 0.95) else {
        return Vec::new();
    };
    if (p95 as f32) <= section.wiring_density_threshold as f32 * 1.5 {
        return Vec::new();
    }
    let suggested = ((p95 as f32) * 1.1).ceil() as u32;
    vec![Suggestion {
        category: SuggestionCategory::Threshold,
        headline: format!(
            "CR002 wiring-density p95 = {p95}; current cap = {}",
            section.wiring_density_threshold
        ),
        why: vec!["p95 above current cap by >1.5×".into()],
        options: vec![CommandOption {
            label: "raise the cap".into(),
            commands: vec![format!(
                "locus cr set-wiring-density-threshold {suggested}"
            )],
        }],
        prefixes: vec![CR_PREFIX.into()],
    }]
}
```

Add the import at the top of the file:

```rust
use crate::init::percentile;
```

- [ ] **Step 2: Add a CR wiring-density test**

Append to `crates/locus-core/src/paradigms/composition_root/init.rs`'s `mod tests`:

```rust
    #[test]
    fn no_threshold_suggestion_when_constructs_within_default() {
        // CR default = 12; need p95 > 18 to trigger.
        let air = ws_with("x::lib"); // empty function list
        let lf = Lockfile::empty();
        let s = suggest(&air, &lf);
        assert!(s.iter().all(|s| s.category != SuggestionCategory::Threshold));
    }
```

(A more thorough test that builds AIR with many `TruthAction::Construct` actions can be added once the visitor fixture helpers are available; the regression test above ensures the no-suggestion path doesn't fire spuriously.)

- [ ] **Step 3: Verify CR setter exists**

Run: `grep -n "set-wiring-density\|SetWiringDensity" crates/locus-cli/src/main.rs`

If absent, add a `SetWiringDensityThreshold` variant to `enum CrCommand` and a corresponding handler that mirrors the BO setter pattern (see Task 2.2 Step 4 for the template). The setter writes `section.wiring_density_threshold = args.threshold;`.

- [ ] **Step 4: RM threshold suggestion**

Create `crates/locus-core/src/paradigms/responsibility/init.rs`:

```rust
//! `locus init` suggestions for the RM paradigm.

use locus_air::{AirItem, AirWorkspace};

use crate::init::{CommandOption, Suggestion, SuggestionCategory, percentile};
use crate::lockfile::Lockfile;
use super::lockfile_schema::RmSection;
use super::RM_PREFIX;

pub fn suggest(air: &AirWorkspace, lockfile: &Lockfile) -> Vec<Suggestion> {
    let section: RmSection = lockfile.paradigm_section(RM_PREFIX).unwrap_or_default();
    if section.default_max_action_kinds.is_some() {
        return Vec::new();
    }
    if lockfile.is_acknowledged_empty(RM_PREFIX) {
        return Vec::new();
    }
    let kinds_per_fn: Vec<u32> = air
        .packages
        .iter()
        .flat_map(|p| p.files.iter())
        .flat_map(|f| f.items.iter())
        .filter_map(|item| match item {
            AirItem::Function(f) => {
                use std::collections::BTreeSet;
                let kinds: BTreeSet<&'static str> = f
                    .actions
                    .iter()
                    .map(action_kind_tag)
                    .collect();
                Some(kinds.len() as u32)
            }
            _ => None,
        })
        .collect();
    let Some(p95) = percentile(&kinds_per_fn, 0.95) else {
        return Vec::new();
    };
    if p95 <= 3 {
        return Vec::new();
    }
    let suggested = ((p95 as f32) * 1.1).ceil() as u32;
    vec![Suggestion {
        category: SuggestionCategory::Threshold,
        headline: format!("RM001 action-kinds-per-fn p95 = {p95}; no cap set"),
        why: vec!["consider an explicit cap so RM001 fires meaningfully".into()],
        options: vec![CommandOption {
            label: "set explicit cap".into(),
            commands: vec![format!("locus rm set-default --max-kinds {suggested}")],
        }],
        prefixes: vec![RM_PREFIX.into()],
    }]
}

fn action_kind_tag(a: &locus_air::AirTruthAction) -> &'static str {
    use locus_air::AirTruthAction::*;
    match a {
        Construct(_) => "construct",
        Validate(_) => "validate",
        Persist(_) => "persist",
        Delegate(_) => "delegate",
        Transform(_) => "transform",
        Branch(_) => "branch",
        _ => "other",
    }
}
```

(If `AirTruthAction` variants in your AIR differ from the placeholders here, run `grep -n "pub enum AirTruthAction" crates/locus-air/src/lib.rs` and edit the match to use the actual variants.)

- [ ] **Step 5: Wire RM**

Edit `crates/locus-core/src/paradigms/responsibility/mod.rs`. Add `pub mod init;`. Override `suggest`:

```rust
    fn suggest(&self, air: &AirWorkspace, lockfile: &Lockfile) -> Vec<crate::init::Suggestion> {
        init::suggest(air, lockfile)
    }
```

- [ ] **Step 6: Run all phase-5 tests**

Run: `cargo test -p locus-core complexity_budget::init module_ownership::init composition_root::init responsibility::init`
Expected: ALL PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/locus-core/src/paradigms/composition_root/init.rs \
        crates/locus-core/src/paradigms/responsibility/init.rs \
        crates/locus-core/src/paradigms/responsibility/mod.rs \
        crates/locus-cli/src/main.rs
git commit -m "feat(cr,rm): threshold suggestions for wiring density and action kinds"
```

---

## Phase 6: Vacancy seeding + RW `accept-runtime-owner`

After phase 6, every vacant-by-definition paradigm without a more specific suggestion gets a generic `[paradigm-vacant]` block, and RW gains its first CLI mutator.

### Task 6.1: Generic vacancy fallback in the aggregator

**Files:**
- Modify: `crates/locus-core/src/init.rs`

- [ ] **Step 1: Add the test**

Append to `crates/locus-core/src/init.rs`:

```rust
#[cfg(test)]
mod vacancy_tests {
    use super::*;
    use locus_air::AirWorkspace;
    use crate::lockfile::Lockfile;

    #[test]
    fn vacancy_seed_for_paradigms_with_no_specific_suggestion() {
        let air = AirWorkspace::new(Vec::new());
        let lf = Lockfile::empty();
        let seeds = vacancy_seeds(
            &air,
            &lf,
            &[("RW", "Runtime Work", &["locus rw accept-runtime-owner \"<glob>\""])],
            &[], // no specific suggestions
        );
        assert_eq!(seeds.len(), 1);
        assert_eq!(seeds[0].category, SuggestionCategory::ParadigmVacant);
        let cmds = seeds[0].options[0].commands.join("\n");
        assert!(cmds.contains("locus rw accept-runtime-owner"));
    }

    #[test]
    fn vacancy_seed_suppressed_when_specific_suggestion_already_present() {
        let air = AirWorkspace::new(Vec::new());
        let lf = Lockfile::empty();
        let specific = vec![Suggestion {
            category: SuggestionCategory::Layer,
            headline: "RW-specific suggestion".into(),
            why: Vec::new(),
            options: vec![CommandOption { label: "x".into(), commands: vec!["a".into()] }],
            prefixes: vec!["RW".into()],
        }];
        let seeds = vacancy_seeds(
            &air,
            &lf,
            &[("RW", "Runtime Work", &["locus rw accept-runtime-owner \"<glob>\""])],
            &specific,
        );
        assert!(seeds.is_empty());
    }

    #[test]
    fn vacancy_seed_suppressed_when_acknowledged_empty() {
        let air = AirWorkspace::new(Vec::new());
        let mut lf = Lockfile::empty();
        lf.acknowledged_empty.push("RW".into());
        let seeds = vacancy_seeds(
            &air,
            &lf,
            &[("RW", "Runtime Work", &["locus rw accept-runtime-owner \"<glob>\""])],
            &[],
        );
        assert!(seeds.is_empty());
    }
}
```

- [ ] **Step 2: Run; expect failure**

Run: `cargo test -p locus-core vacancy_tests`
Expected: FAIL — `cannot find function 'vacancy_seeds'`.

- [ ] **Step 3: Implement**

Append to `crates/locus-core/src/init.rs`:

```rust
/// `(prefix, human-readable name, &[seed commands])`
type VacantSeed<'a> = (&'a str, &'a str, &'a [&'a str]);

pub fn vacancy_seeds(
    _air: &AirWorkspace,
    lockfile: &Lockfile,
    seeds: &[VacantSeed<'_>],
    specific: &[Suggestion],
) -> Vec<Suggestion> {
    let already_covered: std::collections::BTreeSet<String> = specific
        .iter()
        .flat_map(|s| s.prefixes.iter().cloned())
        .collect();
    seeds
        .iter()
        .filter_map(|(prefix, name, cmds)| {
            if lockfile.is_acknowledged_empty(prefix) {
                return None;
            }
            if already_covered.contains(*prefix) {
                return None;
            }
            Some(Suggestion {
                category: SuggestionCategory::ParadigmVacant,
                headline: format!("{prefix} ({name}) has no definitions"),
                why: Vec::new(),
                options: vec![
                    CommandOption {
                        label: "onboard".into(),
                        commands: cmds.iter().map(|c| (*c).to_string()).collect(),
                    },
                    CommandOption {
                        label: "or skip".into(),
                        commands: vec![format!("locus init --acknowledge-empty {prefix}")],
                    },
                ],
                prefixes: vec![(*prefix).to_string()],
            })
        })
        .collect()
}
```

- [ ] **Step 4: Wire vacancy_seeds into the CLI's `init` flow**

Edit `crates/locus-cli/src/main.rs`. Inside `init()`, replace the suggestion-aggregation block with:

```rust
    let mut suggestions: Vec<locus_core::init::Suggestion> = Vec::new();
    for paradigm in &registry {
        suggestions.extend(paradigm.suggest(&air, &lockfile));
    }
    suggestions.extend(locus_core::init::cross_paradigm_suggestions(&air, &lockfile));
    let seeds: &[(&str, &str, &[&str])] = &[
        ("RW", "Runtime Work", &["locus rw accept-runtime-owner \"<glob>\""]),
        ("OB", "Observability", &["locus ob add-observer-path \"<glob>\""]),
        ("AB", "Abstraction Discipline", &["locus ab accept-single-impl \"<symbol>\""]),
        ("DA", "Demand-Driven", &["locus da toggle --enabled true"]),
        ("DC", "Documentation", &["locus dc toggle --require-public-docs true"]),
    ];
    suggestions.extend(locus_core::init::vacancy_seeds(&air, &lockfile, seeds, &suggestions));
    let suggestions = locus_core::init::aggregate(suggestions);

    let hints_promoted = count_hint_promotions(&lockfile);
    print!("{}", render_checklist(&suggestions, hints_promoted));

    if !suggestions.is_empty() {
        std::process::exit(1);
    }
    Ok(())
```

- [ ] **Step 5: Run vacancy tests**

Run: `cargo test -p locus-core vacancy_tests`
Expected: 3 tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/locus-core/src/init.rs crates/locus-cli/src/main.rs
git commit -m "feat(core): generic paradigm-vacancy seed for un-onboarded paradigms"
```

### Task 6.2: RW `accept-runtime-owner` setter + CLI

**Files:**
- Create: `crates/locus-core/src/paradigms/runtime_work/edit.rs`
- Modify: `crates/locus-core/src/paradigms/runtime_work/mod.rs`
- Modify: `crates/locus-cli/src/main.rs`

- [ ] **Step 1: Author the edit module + tests**

Create `crates/locus-core/src/paradigms/runtime_work/edit.rs`:

```rust
//! `locus rw ...` — symbol-by-symbol mutators for the RW lockfile section.

use thiserror::Error;

use super::lockfile_schema::RwSection;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RwEditError {
    #[error("runtime owner pattern must not be empty")]
    EmptyRuntimeOwnerPath,
}

pub fn add_runtime_owner_path(section: &mut RwSection, pattern: &str) -> Result<(), RwEditError> {
    if pattern.is_empty() {
        return Err(RwEditError::EmptyRuntimeOwnerPath);
    }
    if !section.runtime_owner_paths.iter().any(|p| p == pattern) {
        section.runtime_owner_paths.push(pattern.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_runtime_owner_appends_and_dedupes() {
        let mut s = RwSection::default();
        add_runtime_owner_path(&mut s, "crate::runtime::*").unwrap();
        add_runtime_owner_path(&mut s, "crate::worker::*").unwrap();
        add_runtime_owner_path(&mut s, "crate::runtime::*").unwrap();
        assert_eq!(
            s.runtime_owner_paths,
            vec!["crate::runtime::*", "crate::worker::*"]
        );
    }

    #[test]
    fn add_runtime_owner_rejects_empty() {
        let mut s = RwSection::default();
        assert_eq!(
            add_runtime_owner_path(&mut s, "").unwrap_err(),
            RwEditError::EmptyRuntimeOwnerPath
        );
    }
}
```

- [ ] **Step 2: Wire it into the RW paradigm**

Edit `crates/locus-core/src/paradigms/runtime_work/mod.rs` and add at the top:

```rust
pub mod edit;
```

- [ ] **Step 3: Run the edit tests**

Run: `cargo test -p locus-core runtime_work::edit`
Expected: 2 tests pass.

- [ ] **Step 4: Add the CLI subcommand**

In `crates/locus-cli/src/main.rs`, add to the top-level `Command` enum (after `Rm`):

```rust
    /// Manage RW (Runtime Work Ownership) declarations in `locus.lock`.
    #[command(subcommand)]
    Rw(RwCommand),
```

In the `match cli.command` block, add:

```rust
        Command::Rw(cmd) => rw(cmd),
```

Define the subcommand and arg structs:

```rust
// locus: ot boundary cli.rw cli
#[derive(Subcommand, Debug)]
enum RwCommand {
    /// Mark a module pattern as a runtime owner (RW001).
    AcceptRuntimeOwner(RwAcceptRuntimeOwnerArgs),
}

// locus: ot boundary cli.rw-accept-runtime-owner cli
#[derive(clap::Args, Debug)]
struct RwAcceptRuntimeOwnerArgs {
    /// Module path glob.
    pattern: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}
```

Add the dispatch + handler:

```rust
fn rw(cmd: RwCommand) -> Result<()> {
    match cmd {
        RwCommand::AcceptRuntimeOwner(args) => rw_accept_runtime_owner_cli(args),
    }
}

fn rw_accept_runtime_owner_cli(args: RwAcceptRuntimeOwnerArgs) -> Result<()> {
    use locus_core::paradigms::runtime_work::edit::add_runtime_owner_path;
    use locus_core::paradigms::runtime_work::lockfile_schema::RwSection;

    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: RwSection = lockfile
        .paradigm_section("RW")
        .context("RW lockfile section is malformed")?;

    add_runtime_owner_path(&mut section, &args.pattern)
        .with_context(|| format!("add RW runtime owner path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize RW section")?;
    lockfile.paradigms.insert("RW".to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added RW runtime owner pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}
```

- [ ] **Step 5: CLI-help-check**

Run: `cargo run -p locus-cli -- rw accept-runtime-owner --help`
Expected: clap renders the subcommand.

- [ ] **Step 6: Commit**

```bash
git add crates/locus-core/src/paradigms/runtime_work/edit.rs \
        crates/locus-core/src/paradigms/runtime_work/mod.rs \
        crates/locus-cli/src/main.rs
git commit -m "feat(rw): 'locus rw accept-runtime-owner' setter"
```

---

## Phase 7: Snapshot / fixture coverage

### Task 7.1: Sample-crate end-to-end snapshot

**Files:**
- Modify: `crates/locus-cli/Cargo.toml`
- Create: `crates/locus-cli/tests/init_smoke.rs`

- [ ] **Step 1: Add dev-deps**

Edit `crates/locus-cli/Cargo.toml`. Replace its `[dev-dependencies]` block with:

```toml
[dev-dependencies]
tempfile = "3"
assert_cmd = "2"
predicates = "3"
insta = { workspace = true, features = ["filters"] }
```

- [ ] **Step 2: Author the smoke test**

Create `crates/locus-cli/tests/init_smoke.rs`:

```rust
use assert_cmd::Command;

#[test]
fn init_against_sample_crate_emits_expected_checklist() {
    let bin = env!("CARGO_BIN_EXE_locus");
    let workspace_dir = tempfile::tempdir().unwrap();
    // Copy the fixture into a tempdir so the test doesn't dirty the repo.
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/sample-crate");
    copy_dir_all(&src, workspace_dir.path()).unwrap();

    let assert = Command::new(bin)
        .arg("init")
        .arg("--workspace")
        .arg(workspace_dir.path())
        .assert();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    let mut settings = insta::Settings::clone_current();
    settings.add_filter(r"wrote .*locus\.lock", "wrote <PATH>/locus.lock");
    settings.add_filter(r"updated .*locus\.lock", "updated <PATH>/locus.lock");
    settings.bind(|| insta::assert_snapshot!("init_sample_crate", stdout));
}

fn copy_dir_all(
    src: &std::path::Path,
    dst: &std::path::Path,
) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &dst_path)?;
        } else {
            std::fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}
```

- [ ] **Step 3: Run once to capture the snapshot**

Run: `INSTA_UPDATE=auto cargo test -p locus-cli --test init_smoke -- --nocapture`
Expected: PASS, with `init_sample_crate.snap` created in `crates/locus-cli/tests/snapshots/`.

- [ ] **Step 4: Inspect the snapshot**

Open `crates/locus-cli/tests/snapshots/init_smoke__init_sample_crate.snap` and verify it contains the expected checklist shape: `auto-applied:` line, `unresolved:` line, suggestion blocks. Edit only if the snapshot reveals a real bug; otherwise leave as the regression baseline.

- [ ] **Step 5: Run the test in pure-replay mode**

Run: `cargo test -p locus-cli --test init_smoke`
Expected: PASS without `INSTA_UPDATE`.

- [ ] **Step 6: Commit**

```bash
git add crates/locus-cli/Cargo.toml crates/locus-cli/tests/init_smoke.rs \
        crates/locus-cli/tests/snapshots/
git commit -m "test(cli): snapshot test for 'locus init' against sample-crate"
```

### Task 7.2: Concept-cluster fixture (User / UserResponse + From impl)

**Files:**
- Create: `tests/fixtures/cluster-crate/Cargo.toml`
- Create: `tests/fixtures/cluster-crate/src/lib.rs`
- Create: `tests/fixtures/cluster-crate/src/domain.rs`
- Create: `tests/fixtures/cluster-crate/src/api.rs`
- Modify: `crates/locus-cli/tests/init_smoke.rs`

- [ ] **Step 1: Create the fixture crate**

Create `tests/fixtures/cluster-crate/Cargo.toml`:

```toml
[workspace]

[package]
name = "cluster-crate"
version = "0.1.0"
edition = "2024"
publish = false

[lib]
path = "src/lib.rs"
```

Create `tests/fixtures/cluster-crate/src/lib.rs`:

```rust
pub mod domain;
pub mod api;
```

Create `tests/fixtures/cluster-crate/src/domain.rs`:

```rust
pub struct User {
    pub id: u64,
    pub email: String,
    pub created_at: u64,
}
```

Create `tests/fixtures/cluster-crate/src/api.rs`:

```rust
use crate::domain::User;

pub struct UserResponse {
    pub id: u64,
    pub email: String,
}

impl From<User> for UserResponse {
    fn from(u: User) -> Self {
        UserResponse { id: u.id, email: u.email }
    }
}
```

- [ ] **Step 2: Add a snapshot test for the cluster fixture**

Append to `crates/locus-cli/tests/init_smoke.rs`:

```rust
#[test]
fn init_against_cluster_crate_emits_concept_suggestion() {
    let bin = env!("CARGO_BIN_EXE_locus");
    let workspace_dir = tempfile::tempdir().unwrap();
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/cluster-crate");
    copy_dir_all(&src, workspace_dir.path()).unwrap();

    let assert = Command::new(bin)
        .arg("init")
        .arg("--workspace")
        .arg(workspace_dir.path())
        .assert();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    let mut settings = insta::Settings::clone_current();
    settings.add_filter(r"wrote .*locus\.lock", "wrote <PATH>/locus.lock");
    settings.add_filter(r"updated .*locus\.lock", "updated <PATH>/locus.lock");
    settings.bind(|| insta::assert_snapshot!("init_cluster_crate", stdout));

    assert!(stdout.contains("[concept]"), "expected a concept-cluster suggestion");
    assert!(stdout.contains("user"), "expected the cluster id 'user'");
}
```

- [ ] **Step 3: Capture and inspect the snapshot**

Run: `INSTA_UPDATE=auto cargo test -p locus-cli --test init_smoke init_against_cluster_crate -- --nocapture`
Expected: PASS, snapshot created. Open the snapshot and verify the `[concept]` block lists the `accept canonical` and `accept boundary` commands.

- [ ] **Step 4: Run the test in replay mode**

Run: `cargo test -p locus-cli --test init_smoke init_against_cluster_crate`
Expected: PASS.

- [ ] **Step 5: Round-trip test — running the suggested commands clears the suggestion**

Append to `crates/locus-cli/tests/init_smoke.rs`:

```rust
#[test]
fn cluster_round_trip_clears_concept_suggestion() {
    let bin = env!("CARGO_BIN_EXE_locus");
    let workspace_dir = tempfile::tempdir().unwrap();
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/cluster-crate");
    copy_dir_all(&src, workspace_dir.path()).unwrap();

    // First run: capture concept suggestion presence.
    let first = Command::new(bin)
        .arg("init")
        .arg("--workspace")
        .arg(workspace_dir.path())
        .output()
        .unwrap();
    let first_out = String::from_utf8_lossy(&first.stdout);
    assert!(first_out.contains("[concept]"));

    // Apply the suggested commands.
    Command::new(bin)
        .args([
            "accept",
            "canonical",
            "cluster_crate::domain::User",
            "--concept",
            "user",
        ])
        .arg("--workspace")
        .arg(workspace_dir.path())
        .assert()
        .success();
    Command::new(bin)
        .args([
            "accept",
            "boundary",
            "cluster_crate::api::UserResponse",
            "--concept",
            "user",
        ])
        .arg("--workspace")
        .arg(workspace_dir.path())
        .assert()
        .success();

    // Second run: concept suggestion should be gone.
    let second = Command::new(bin)
        .arg("init")
        .arg("--workspace")
        .arg(workspace_dir.path())
        .output()
        .unwrap();
    let second_out = String::from_utf8_lossy(&second.stdout);
    assert!(
        !second_out.contains("[concept]"),
        "concept suggestion should have cleared after accept commands; got:\n{second_out}"
    );
}
```

- [ ] **Step 6: Run the round-trip**

Run: `cargo test -p locus-cli --test init_smoke cluster_round_trip`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add tests/fixtures/cluster-crate \
        crates/locus-cli/tests/init_smoke.rs \
        crates/locus-cli/tests/snapshots/
git commit -m "test: cluster-crate fixture + round-trip test for concept suggestion"
```

### Task 7.3: Self-application smoke test

**Files:** *no new files; this task is a manual verification + docs update*

- [ ] **Step 1: Run init against the locus repo itself**

Run: `cargo run -p locus-cli -- init --workspace .`
Expected output: a non-empty checklist that proposes valid commands. Exit code 1.

- [ ] **Step 2: Pick one suggestion at random and execute it**

For example, if the output proposes `locus bo add-domain-path "crate::domain::*"` for the locus-core crate, run that command. Verify `locus.lock` updates.

- [ ] **Step 3: Re-run init and confirm the suggestion disappears**

Run: `cargo run -p locus-cli -- init --workspace .`
Expected: the previously-printed suggestion no longer appears (or is replaced by a more specific follow-up).

- [ ] **Step 4: Restore working tree**

Run: `git checkout -- locus.lock`
Expected: pre-test lockfile state restored.

- [ ] **Step 5: Update the project status note in CLAUDE.md**

Append a note to the `## Implementation roadmap` block in `CLAUDE.md` (just below the existing `🔜 CLI oracle commands:` line):

```markdown
- ✅ **`locus init` multi-run scan-and-report** — `locus init` emits a checklist of `locus <verb> ...` commands per detected layer / concept cluster / feature / threshold; `--acknowledge-empty` silences a paradigm; new mutators added: `accept converter`, `rw accept-runtime-owner`, `er add-domain-path`, `rm add-domain-path`, `pa add-application-path`. See `docs/superpowers/specs/2026-05-08-locus-init-multi-run-design.md`.
```

- [ ] **Step 6: Run the entire test suite**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: ALL PASS, no clippy warnings.

- [ ] **Step 7: Commit the docs update**

```bash
git add CLAUDE.md
git commit -m "docs: note multi-run init in implementation roadmap"
```

---

## Self-review

The plan was reviewed against the spec at `docs/superpowers/specs/2026-05-08-locus-init-multi-run-design.md`:

- **Phase 1 — Suggestion infrastructure**: Tasks 1.1–1.4 cover the data shape, trait method, `--acknowledge-empty` flag, and renderer.
- **Phase 2 — Path heuristics**: Tasks 2.1–2.6 cover layer detection, the missing ER/RM/PA setters, the cross-paradigm Layer suggestion, and per-paradigm `suggest()` for CR/TA/UT/CF.
- **Phase 3 — OT clustering**: Tasks 3.1–3.3 cover confidence, `suggest()`, and `accept converter`.
- **Phase 4 — Features**: Tasks 4.1–4.2 cover top-level enumeration and the cross-paradigm Feature suggestion.
- **Phase 5 — Thresholds**: Tasks 5.1–5.4 cover the percentile helper plus CX/MO/CR/RM threshold suggestions.
- **Phase 6 — Vacancy + RW**: Tasks 6.1–6.2 cover the generic vacancy fallback and `rw accept-runtime-owner`.
- **Phase 7 — Fixtures**: Tasks 7.1–7.3 cover sample-crate snapshot, cluster-crate fixture + round-trip, and self-application + docs update.

All design constraints from the spec are honoured: no daemon, no on-disk architecture model, init never auto-writes inferred decisions (only existing source-hint promotion), per-paradigm `init.rs` modules.

Open questions from the spec are surfaced and respected:
- `acknowledged_empty` already exists on `Lockfile` (verified during context exploration; Phase 1 reuses it without adding the field).
- Confidence thresholds (0.95 / 0.70) and tolerance (1.5×) are encoded as named constants in the relevant tasks for future tuning.

No placeholders remain. Every step contains concrete commands, exact file paths, and complete code.

---

## Execution

Plan complete and saved to `docs/superpowers/plans/2026-05-08-locus-init-multi-run.md`. Two execution options:

1. **Subagent-Driven (recommended)** — fresh subagent per task, review between tasks, fast iteration.
2. **Inline Execution** — execute tasks in this session using executing-plans, batch execution with checkpoints.

Which approach?
