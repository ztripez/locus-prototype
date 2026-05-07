//! Lockfile section shape for AB (Abstraction Discipline). Stub.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct AbSection {
    // TODO: add fields when rules land.
}
