//! Lockfile section shape for RM (Responsibility Mixing). Stub.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct RmSection {
    // TODO: add fields when rules land.
}
