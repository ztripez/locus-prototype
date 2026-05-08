//! RW — Runtime Work Ownership.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 14: Runtime Work Ownership".
//!
//! Phase scope:
//! - RW001: spawn-shaped action (tokio/std::thread/rayon/etc.) outside any
//!   declared runtime owner module.
//! - RW003: `Mutex` / `RwLock` (or similar runtime-state-shaped) field on a
//!   type outside any declared runtime-owner module.
//! - RW004: `OnceCell` / `Lazy` / named-singleton type outside any declared
//!   runtime-owner module.
//!
//! `init` returns an empty section: runtime-owner locations are a user
//! declaration, not an inference. The rules stay silent until the user
//! populates `runtime_owner_paths`.

// ot: canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod lockfile_schema;
pub mod rules;

pub const RW_PREFIX: &str = "RW";

pub struct RuntimeWork;

impl Paradigm for RuntimeWork {
    fn name(&self) -> &'static str {
        "Runtime Work Ownership"
    }
    fn rule_prefix(&self) -> &'static str {
        RW_PREFIX
    }
    fn init(&self, _air: &AirWorkspace) -> serde_json::Value {
        serde_json::Value::Null
    }
    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic> {
        let section: lockfile_schema::RwSection =
            lockfile.paradigm_section(RW_PREFIX).unwrap_or_default();
        let mut diags = rules::rw001(air, &section, mode);
        diags.extend(rules::rw003(air, &section, mode));
        diags.extend(rules::rw004(air, &section, mode));
        diags
    }
}
