//! Lockfile section shape for DA (Demand-Driven Architecture). Stub.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct DaSection {
    // TODO: add fields when rules land.
}
