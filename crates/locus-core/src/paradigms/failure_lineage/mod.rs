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
//! - FL011: bare `_ => <silent>` match arm whose body is `Empty`,
//!   `Literal`, or `Call`, outside `invariant_owner_paths` — unknown
//!   enum variants silently routed to a default sink.
//! - FL013: a function returning `Result<_, String>` / `Result<_, &str>`
//!   that contains a stringifying call site (`to_string` / `format!` /
//!   `format` / `display`) — lossy error stringification at the source.

// ot: canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic};
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
    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic> {
        let section: lockfile_schema::FlSection =
            lockfile.paradigm_section(FL_PREFIX).unwrap_or_default();
        let mut out = rules::fl001(air, &section, mode);
        out.extend(rules::fl002(air, &section, mode));
        out.extend(rules::fl003(air, &section, mode));
        out.extend(rules::fl004(air, &section, mode));
        out.extend(rules::fl005(air, &section, mode));
        out.extend(rules::fl006(air, &section, mode));
        out.extend(rules::fl007(air, &section, mode));
        out.extend(rules::fl011(air, &section, mode));
        out.extend(rules::fl013(air, &section, mode));
        out
    }
}
