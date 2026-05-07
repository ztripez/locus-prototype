//! Lockfile section shape for CF (Config/Data Ownership). Stub.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct CfSection {
    // TODO: add fields when rules land.
}
