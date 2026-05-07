//! Lockfile section shape for CR (Composition Root Ownership). Stub.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct CrSection {
    // TODO: add fields when rules land.
}
