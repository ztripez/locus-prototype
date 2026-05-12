//! DC — Documentation / Comment Ownership.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 17: Documentation / Comment Ownership".
//!
//! Phase scope so far:
//! - DC001: public type or function has no doc comment. Opt-in via
//!   `paradigms.DC.require_public_docs`; silent by default.
//! - DC002: public type or function carries a doc comment containing a
//!   forbidden phrase (LLM-transcript residue, stale planning markers).
//!   Active by default thanks to a seeded `forbidden_doc_phrases` list;
//!   clearing the list opts out.
//! - DC004: public type or function carries a `TODO`/`FIXME`/`HACK`/`XXX`
//!   marker without a parenthesised owner reference (`TODO(alice):` /
//!   `FIXME(#123):`). Active by default thanks to a seeded
//!   `unowned_marker_patterns` list; clearing it opts out.

// locus: ot canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
pub mod lockfile_schema;
pub mod rules;

pub const DC_PREFIX: &str = "DC";

pub struct Documentation;

impl Paradigm for Documentation {
    fn name(&self) -> &'static str {
        "Documentation / Comment Ownership"
    }
    fn rule_prefix(&self) -> &'static str {
        DC_PREFIX
    }
    fn init(&self, _air: &AirWorkspace) -> serde_json::Value {
        // No automatic inference: `require_public_docs` is a project policy
        // choice, and `exempt_paths` is hand-curated. `init` returns an
        // empty section; users opt in via the lockfile directly (or via a
        // future `locus dc` CLI mutator).
        serde_json::Value::Null
    }
    fn check(
        &self,
        _air: &AirWorkspace,
        _lockfile: &Lockfile,
        _mode: CheckMode,
    ) -> Vec<Diagnostic> {
        // All DC rules migrated to RuleDefinition (#71 P4).
        // Detection runs through the governance pipeline; this legacy
        // path is now a no-op.
        Vec::new()
    }
}
