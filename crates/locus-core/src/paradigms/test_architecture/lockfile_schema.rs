//! Lockfile section shape for TA (Test Architecture Ownership). Stub.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TaSection {
    // TODO: add fields when rules land.
}
