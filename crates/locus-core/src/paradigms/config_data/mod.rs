//! CF — Config/Data Ownership.
//!
//! Spec: `docs/PARADIGMS.md` §"Paradigm 2: Config/Data Ownership". Reads the
//! declared config layer from `paradigms.CF` in `locus.lock` and flags
//! environment-variable reads emitted by the AIR visitor as
//! `ActionKind::EnvRead` from any file outside that layer.
//!
//! Phase scope so far:
//! - CF001: environment-variable read outside the config layer.
//! - CF002: filesystem-walk rule, reserved for a future filesystem-aware
//!   loader. Lockfile fields ship today (`config_file_patterns`,
//!   `accepted_config_files`) so users can pre-populate; the rule body
//!   is a no-op stub until the loader lands.

// ot: canonical

use super::Paradigm;
use crate::diagnostics::{CheckMode, Diagnostic};
use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;

pub mod edit;
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
    fn check(&self, air: &AirWorkspace, lockfile: &Lockfile, mode: CheckMode) -> Vec<Diagnostic> {
        let section: lockfile_schema::CfSection =
            lockfile.paradigm_section(CF_PREFIX).unwrap_or_default();
        rules::cf001(air, &section, mode)
        // TODO(cf002): wire when filesystem-aware loaders land
    }
}
