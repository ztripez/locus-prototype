//! Lockfile section shape for ER (Error Taxonomy Ownership).
//!
//! ER001 is heuristic and lockfile-free — the rule fires purely on the
//! "≥2 public Error types in one file" pattern. Later rules (ER002+) will
//! likely need to record which error symbols the user has explicitly accepted
//! as canonical for a layer, in shape roughly like:
//!
//! ```text
//! TODO (ER002+): accepted_errors: Vec<{ symbol: String, layer: String }>
//! ```
//!
//! For now keep the section empty so ER's lockfile entry remains deserializable
//! and the paradigm dispatch in `mod.rs` can call `paradigm_section::<ErSection>`
//! without erroring out.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ErSection {
    // TODO (ER002+): add fields when later rules land. Likely shape:
    //   `accepted_errors: Vec<AcceptedError>` where `AcceptedError`
    //   carries `{ symbol, layer, source }` — the canonical error type
    //   per layer plus a Source tag mirroring OT's Hint/Init split.
}
