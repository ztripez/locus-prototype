//! MO005 — explicit kind declarations for crate `lib.rs` files.
//!
//! `lib.rs` covers four distinct architectural shapes (thin re-export /
//! canonical-data / composition-root / accidental god module). MO005's
//! default classification is a built-in heuristic; this module ships the
//! `lib_rs_kinds` lockfile schema that lets a project pin a crate's
//! `lib.rs` to a specific kind when the heuristic is wrong (e.g. an
//! intentional canonical-data crate the heuristic would otherwise read
//! as a god module once `pub use` re-exports are added).
//!
//! Lives next to `lockfile_schema.rs` rather than inside it so each
//! schema concept owns its own module — keeps `lockfile_schema.rs`
//! focused on MO001/MO002 per-module budgets and avoids stuffing
//! permanent rule concepts into a growing god-module of its own.

// locus: ot canonical

use serde::{Deserialize, Serialize};

use super::lockfile_schema::matches_pattern;

/// MO005 — explicit kind declaration for a crate's `lib.rs`. Overrides
/// the heuristic that classifies lib.rs files into one of three shapes.
///
/// The `module` field follows the same segment-aligned wildcard syntax
/// as `MoOverride::module` (typically the crate's lib module path:
/// `locus_air`, `my_pkg`, or `my_pkg::*`). The first entry whose pattern
/// matches the `lib.rs`'s module path wins.
///
/// Debt metadata mirrors `MoOverride` for consistency with the rest of
/// the MO section — Policy Guard's PG002/PG006 will visibility-flag new
/// entries the same way.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LibRsKindEntry {
    pub module: String,
    pub kind: LibRsKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced_by: Option<String>,
}

/// MO005 — the three canonical shapes a `lib.rs` can take. Used by
/// [`LibRsKindEntry::kind`] to pin the enforcement mode explicitly when
/// the heuristic is wrong.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LibRsKind {
    /// Thin public re-export surface — `pub use` / `pub mod` only, no
    /// substantial declarations. Same MO005 scoping as `main.rs`: any
    /// type/impl/converter/non-glue function fires.
    #[default]
    ThinReexport,
    /// Canonical-data crate surface — the entire `lib.rs` is intentional
    /// declaration (e.g. `locus-air`: 40+ `pub struct`/`pub enum` types
    /// that ARE the crate's data contract). MO005 is skipped entirely
    /// for the file; MO001 still applies via its normal per-module budget.
    CanonicalData,
    /// Composition root — declarations + setup + glue (e.g. a workspace-
    /// level integration crate that wires several modules together at
    /// the crate root). MO005 is skipped for the file; rely on MO001/MO002
    /// to flag growth into a god module.
    CompositionRoot,
}

/// Find the first `lib_rs_kinds` entry whose `module` pattern matches
/// `module_path`. Returns `None` when no entry matches.
///
/// Lives here (not as a method on `MoSection`) so the schema concept
/// owns its own lookup helper; `MoSection` re-exposes it via
/// `MoSection::lib_rs_kind_for` for ergonomics.
pub fn lookup<'a>(entries: &'a [LibRsKindEntry], module_path: &str) -> Option<&'a LibRsKindEntry> {
    entries
        .iter()
        .find(|e| matches_pattern(&e.module, module_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_returns_first_match() {
        let entries = vec![
            LibRsKindEntry {
                module: "locus_air".into(),
                kind: LibRsKind::CanonicalData,
                ..Default::default()
            },
            LibRsKindEntry {
                module: "locus_core".into(),
                kind: LibRsKind::ThinReexport,
                ..Default::default()
            },
        ];
        let hit = lookup(&entries, "locus_air").expect("expected match");
        assert_eq!(hit.kind, LibRsKind::CanonicalData);
        assert!(lookup(&entries, "other_pkg").is_none());
    }

    #[test]
    fn lib_rs_kind_default_is_thin_reexport() {
        // `LibRsKind` participates in the section's serde default. When
        // a `lib_rs_kinds` entry omits `kind` (deserialised from JSON
        // without that key), it must round-trip to `ThinReexport`.
        let raw = serde_json::json!({
            "module": "some_pkg",
            "kind": "thin-reexport",
        });
        let entry: LibRsKindEntry = serde_json::from_value(raw).unwrap();
        assert_eq!(entry.kind, LibRsKind::ThinReexport);
    }

    #[test]
    fn lib_rs_kind_serialises_kebab_case() {
        let entry = LibRsKindEntry {
            module: "x".into(),
            kind: LibRsKind::CanonicalData,
            ..Default::default()
        };
        let j = serde_json::to_value(&entry).unwrap();
        assert_eq!(j["kind"], "canonical-data");
    }

    #[test]
    fn entry_round_trips_through_serde_with_full_metadata() {
        let entry = LibRsKindEntry {
            module: "locus_air".into(),
            kind: LibRsKind::CanonicalData,
            reason: Some("ADR PR #39".into()),
            expires: Some("2026-12-31".into()),
            owner: Some("@locus-core".into()),
            debt_id: Some("DEBT-123".into()),
            introduced_by: Some("PR #104".into()),
        };
        let j = serde_json::to_value(&entry).unwrap();
        let back: LibRsKindEntry = serde_json::from_value(j).unwrap();
        assert_eq!(entry, back);
    }
}
