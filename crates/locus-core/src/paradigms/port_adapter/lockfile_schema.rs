//! Lockfile section shape for PA (Port/Adapter Ownership). Stub.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct PaSection {
    // TODO: add fields when rules land.
}
