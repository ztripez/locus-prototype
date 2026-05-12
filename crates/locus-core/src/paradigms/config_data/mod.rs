//! CF — Config/Data Ownership.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 2: Config/Data Ownership". Reads the
//! declared config layer from `paradigms.CF` in `locus.lock` and flags
//! decision-data ownership leaks: env reads, magic decision constants,
//! and hardcoded provider/model/topic IDs outside that layer.
//!
//! Phase scope so far:
//! - CF001: environment-variable read outside the config layer.
//! - CF002: magic decision constant (str/int/float scrutinee literal in a
//!   match arm or `==`/`!=` comparison) outside the config layer.
//!   Configurable via `forbidden_literal_kinds`.
//! - CF003: hardcoded provider/model/topic ID — string scrutinee literal
//!   matching a user-declared `forbidden_id_patterns` entry — outside the
//!   config layer.
//!
//! Future direction: the historical filesystem-walk concept (stray
//! `.yaml`/`.toml` files outside accepted locations) stays parked behind
//! the `config_file_patterns` / `accepted_config_files` lockfile fields
//! pending a filesystem-aware loader.

// locus: ot canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
pub mod init;
pub mod lockfile_schema;
pub mod rules;

pub const CF_PREFIX: &str = "CF";

pub struct ConfigData;

impl Paradigm for ConfigData {
    fn name(&self) -> &'static str {
        "Config/Data Ownership"
    }
    fn rule_prefix(&self) -> &'static str {
        CF_PREFIX
    }
    fn init(&self, _air: &AirWorkspace) -> serde_json::Value {
        // The config-layer split is a user assertion, not an inference.
        // `init` returns an empty section; the user populates `config_paths`
        // directly (or via a future `locus cf` mutator).
        serde_json::Value::Null
    }
    fn check(
        &self,
        _air: &AirWorkspace,
        _lockfile: &Lockfile,
        _mode: CheckMode,
    ) -> Vec<Diagnostic> {
        // All CF rules migrated to RuleDefinition (#71 P4).
        // Detection runs through the governance pipeline; this legacy
        // path is now a no-op.
        Vec::new()
    }
    fn suggest(&self, air: &AirWorkspace, lockfile: &Lockfile) -> Vec<crate::init::Suggestion> {
        init::suggest(air, lockfile)
    }
}
