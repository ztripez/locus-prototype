//! MO — Module / File Ownership.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 9: Module / File Ownership".
//!
//! Phase scope so far:
//! - MO001: too many public top-level types in a single file.
//! - MO002: responsibility entropy in a single file (canonical/boundary/
//!   converter hints, handler-named functions, persistence imports, io
//!   call sites — too many distinct architectural roles co-existing).
//! - MO003: canonical type co-located with a boundary type in the same file.
//! - MO004: canonical type co-located with a handler-named function in the
//!   same file.
//! - MO005: entrypoint modules (`main.rs`, `mod.rs`) must be composition
//!   surfaces, not ownership sites — they may not declare types, impl blocks,
//!   converters, or substantial non-glue functions. `lib.rs` is out of scope
//!   in this first pass (see follow-up issue for lib.rs entrypoint handling).
//!
//! `init` returns `Null`: there's no automatic inference for "this module
//! is allowed to be wide" — the user has to declare the override (or the
//! default) deliberately, same as DG. Without an MO section, MO001/MO002
//! stay silent so un-onboarded code isn't bombarded with file-shape
//! warnings. MO003/MO004 are pure structural checks driven by hints, so
//! they fire as soon as the source carries the relevant `// locus:` comments.

// locus: ot canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
pub mod init;
pub mod lockfile_schema;
pub mod rules;

pub const MO_PREFIX: &str = "MO";

// locus: allow MO005 — paradigm host struct intentionally lives in mod.rs by convention
pub struct ModuleOwnership;

// locus: allow MO005 — paradigm Paradigm impl intentionally lives in mod.rs by convention
impl Paradigm for ModuleOwnership {
    fn name(&self) -> &'static str {
        "Module / File Ownership"
    }
    fn rule_prefix(&self) -> &'static str {
        MO_PREFIX
    }
    fn init(&self, _air: &AirWorkspace) -> serde_json::Value {
        // No automatic inference — module budgets come from the user.
        serde_json::Value::Null
    }
    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic> {
        let section: lockfile_schema::MoSection =
            lockfile.paradigm_section(MO_PREFIX).unwrap_or_default();
        let mut diags = rules::mo001(air, &section, mode);
        diags.extend(rules::mo002(air, &section, mode));
        diags.extend(rules::mo003(air, mode));
        diags.extend(rules::mo004(air, &section, mode));
        diags.extend(rules::mo005(air, mode));
        diags
    }
    fn suggest(&self, air: &AirWorkspace, lockfile: &Lockfile) -> Vec<crate::init::Suggestion> {
        init::suggest(air, lockfile)
    }
}
