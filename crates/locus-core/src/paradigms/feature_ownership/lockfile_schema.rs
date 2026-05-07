//! Lockfile section shape for FO (Feature Ownership). Stub.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct FoSection {
    // TODO: add fields when rules land.
}
