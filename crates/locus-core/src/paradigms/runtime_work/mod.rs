//! RW — Runtime Work Ownership.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 14: Runtime Work Ownership".
//!
//! Phase scope:
//! - RW001: spawn-shaped action (tokio/std::thread/rayon/etc.) outside any
//!   declared runtime owner module.
//! - RW002: blocking call (filesystem read, `thread::sleep`, blocking
//!   socket op, …) outside any declared runtime owner module.
//! - RW003: `Mutex` / `RwLock` (or similar runtime-state-shaped) field on a
//!   type outside any declared runtime-owner module.
//! - RW004: `OnceCell` / `Lazy` / named-singleton type outside any declared
//!   runtime-owner module.
//! - RW005: blocking call inside a function the user marked `// locus: fact
//!   hot_path` — blocking ops in a hot loop / frame budget.
//! - RW006: spawn inside a function the user marked `// locus: fact hot_path`
//!   — uncontrolled per-iteration task pressure.
//!
//! `init` returns an empty section: runtime-owner locations are a user
//! declaration, not an inference. RW001–RW004 stay silent until the user
//! populates `runtime_owner_paths`. RW005 / RW006 are gated by the user's
//! `// locus: fact hot_path` annotations (no lockfile entry needed).

// locus: ot canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
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
    fn check(
        &self,
        _air: &AirWorkspace,
        _lockfile: &Lockfile,
        _mode: CheckMode,
    ) -> Vec<Diagnostic> {
        // All RW rules migrated to RuleDefinition (#71 P4).
        // Detection runs through the governance pipeline; this legacy
        // path is now a no-op.
        Vec::new()
    }
}
