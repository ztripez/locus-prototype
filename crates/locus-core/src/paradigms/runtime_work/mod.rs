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
use crate::diagnostics::{CheckMode, Diagnostic, vacant_paradigm_diagnostic};
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
    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic> {
        let section: lockfile_schema::RwSection =
            lockfile.paradigm_section(RW_PREFIX).unwrap_or_default();
        // RW005/006 are marker-driven (no lockfile entry needed) — keep
        // them on regardless of vacancy.
        let mut diags = rules::rw005(air, mode);
        diags.extend(rules::rw006(air, mode));
        if section.is_vacant() && !lockfile.is_acknowledged_empty(RW_PREFIX) {
            diags.push(vacant_paradigm_diagnostic(
                RW_PREFIX,
                "Runtime Work Ownership",
                &[(
                    "runtime_owner_paths",
                    "module patterns identifying runtime owners (job queues, supervisors, runtime entry points)",
                )],
            ));
            return diags;
        }
        diags.extend(rules::rw001(air, &section, mode));
        diags.extend(rules::rw002(air, &section, mode));
        diags.extend(rules::rw003(air, &section, mode));
        diags.extend(rules::rw004(air, &section, mode));
        diags
    }
}
