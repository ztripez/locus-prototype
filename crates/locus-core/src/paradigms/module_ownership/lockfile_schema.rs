//! Lockfile section shape for MO (Module / File Ownership). Stub.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct MoSection {
    // TODO: add fields when rules land.
}
