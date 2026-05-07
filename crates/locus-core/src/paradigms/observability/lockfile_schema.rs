//! Lockfile section shape for OB (Observability Ownership). Stub.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ObSection {
    // TODO: add fields when rules land.
}
