//! Lockfile section shape for FL (Failure Lineage Ownership). Stub.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct FlSection {
    // TODO: add fields when rules land.
}
