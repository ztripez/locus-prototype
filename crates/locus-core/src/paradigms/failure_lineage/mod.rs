//! FL — Failure Lineage Ownership.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 12: Failure Lineage Ownership". Reads
//! declared domain module patterns and boundary error patterns from
//! `paradigms.FL` in `locus.lock` and flags functions in domain modules whose
//! `Result<_, E>` return type leaks a boundary error — the layer edge that
//! should have wrapped the error in a domain error type didn't, breaking the
//! failure-lineage invariant.
//!
//! Phase scope so far:
//! - FL001: domain function returns `Result<_, E>` where E is a declared
//!   boundary error type.
//! - FL002: panic-shaped callee (`unwrap` / `expect` / `unwrap_or_default` /
//!   `panic` / `todo` / `unimplemented`) fires from a module that isn't in
//!   `invariant_owner_paths`.
//! - FL003: silent-discard method call (`.ok()` / `.err()` /
//!   `.unwrap_or_else()`) fires from a module that isn't in
//!   `invariant_owner_paths` — the inverse of FL002.
//! - FL004: `let _ = expr;` discarded binding (where expr is a call) in
//!   a module that isn't in `invariant_owner_paths` and whose callee
//!   isn't on the `silent_discard_allowed_callees` allowlist.
//! - FL005: `if let Ok/Err(...) = expr { ... }` with no `else` branch in
//!   a module that isn't in `invariant_owner_paths`.
//! - FL006: `.map_err(|_| ...)` call whose closure discards its argument,
//!   outside `invariant_owner_paths` — the original error is dropped
//!   before being wrapped, breaking failure lineage at the conversion site.
//! - FL007: catch-all `Err(_) => <silent>` match arm whose body is
//!   `Empty`, `Literal`, or `Call`, outside `invariant_owner_paths` —
//!   every `Err` variant routed to a silent default.
//! - FL010: a `FallbackCall` (`.unwrap_or(...)` / `.or(...)`) whose
//!   default-arg shape is `Literal` or `Call`, outside
//!   `invariant_owner_paths` — invalid input silently converted into a
//!   valid default state. The no-arg `unwrap_or_default()` form is
//!   FL002's territory; FL010 covers the explicit-default form.
//! - FL011: bare `_ => <silent>` match arm whose body is `Empty`,
//!   `Literal`, or `Call`, outside `invariant_owner_paths` — unknown
//!   enum variants silently routed to a default sink.
//! - FL012: a `RetryLoop` (`loop` / `for` / `while`) whose body
//!   contains both `?`-propagation and `break`, outside
//!   `retry_policy_owner_paths` — retry-shaped control flow with no
//!   declared retry policy.
//! - FL013: a function returning `Result<_, String>` / `Result<_, &str>`
//!   that contains a stringifying call site (`to_string` / `format!` /
//!   `format` / `display`) — lossy error stringification at the source.

// locus: ot canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic, vacant_paradigm_diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
pub mod lockfile_schema;
pub mod rules;

pub const FL_PREFIX: &str = "FL";

pub struct FailureLineage;

impl Paradigm for FailureLineage {
    fn name(&self) -> &'static str {
        "Failure Lineage Ownership"
    }
    fn rule_prefix(&self) -> &'static str {
        FL_PREFIX
    }
    fn init(&self, _air: &AirWorkspace) -> serde_json::Value {
        // Domain status and boundary error sets are user assertions, not
        // inferences. `init` returns an empty section; the user adds patterns
        // via future `locus fl ...` commands.
        serde_json::Value::Null
    }
    fn check(&self, _air: &AirWorkspace, lockfile: &Lockfile, _mode: CheckMode) -> Vec<Diagnostic> {
        // All FL rules migrated to RuleDefinition (#71 P4); only the LOCUS002
        // vacancy nudge remains here so vacant-by-definition paradigms keep
        // surfacing onboarding guidance.
        let section: lockfile_schema::FlSection =
            lockfile.paradigm_section(FL_PREFIX).unwrap_or_default();
        if section.is_vacant() && !lockfile.is_acknowledged_empty(FL_PREFIX) {
            return vec![vacant_paradigm_diagnostic(
                FL_PREFIX,
                "Failure Lineage Ownership",
                &[
                    (
                        "invariant_owner_paths",
                        "module patterns where panic-shaped/silent-discard callees are legitimate (typically `*::tests::*`)",
                    ),
                    (
                        "domain_paths",
                        "module patterns identifying domain code (FL001)",
                    ),
                    (
                        "boundary_error_patterns",
                        "patterns matching boundary error types (FL001)",
                    ),
                    (
                        "retry_policy_owner_paths",
                        "module patterns for declared retry-policy modules (FL012)",
                    ),
                ],
            )];
        }
        Vec::new()
    }
}
