//! Lockfile section shape for BO (Boundary Ownership). Stub.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct BoSection {
    // TODO: add fields when rules land.
}
