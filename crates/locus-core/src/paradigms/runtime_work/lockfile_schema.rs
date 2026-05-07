//! Lockfile section shape for RW (Runtime Work Ownership). Stub.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct RwSection {
    // TODO: add fields when rules land.
}
