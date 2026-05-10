//! OT011 — shadow newtype / value object.
//!
//! Fires for each single-field struct (a "newtype") whose **name** matches
//! an accepted canonical (by short name) but whose symbol isn't accepted.
//! Common shape: `pub struct UserId(pub String);` defined in two places.
//!
//! Confidence 0.80 — name-match is a strong signal; the field-type check
//! keeps us off generic `Wrapper<T>`-style structs.

use std::collections::BTreeMap;

use locus_air::{AirItem, AirWorkspace, TypeKind};

use super::super::lockfile_schema::OtSection;
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

pub fn ot011(air: &AirWorkspace, section: &OtSection, mode: CheckMode) -> Vec<Diagnostic> {
    let mut canonical_short: BTreeMap<String, (String, String)> = BTreeMap::new(); // short → (concept, full)
    for (concept_id, entry) in &section.concepts {
        let symbol = &entry.canonical.symbol;
        if let Some(short) = symbol.rsplit("::").next() {
            canonical_short.insert(short.to_string(), (concept_id.clone(), symbol.clone()));
        }
    }
    if canonical_short.is_empty() {
        return Vec::new();
    }
    let confidence = 0.80;
    let Some(severity) = Severity::from_confidence(confidence, mode) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Type(ty) = item else {
                    continue;
                };
                if ty.kind != TypeKind::Struct || ty.fields.len() != 1 {
                    continue;
                }
                if section.role_of(&ty.symbol).is_some() {
                    continue;
                }
                let Some((concept_id, canonical_symbol)) = canonical_short.get(ty.name.as_str())
                else {
                    continue;
                };
                if &ty.symbol == canonical_symbol {
                    continue; // canonical itself, just not accepted under that concept yet
                }
                out.push(Diagnostic {
                    rule_id: "OT011".to_string(),
                    severity,
                    span: ty.span.clone(),
                    concept: Some(concept_id.clone()),
                    message: format!(
                        "newtype `{}` shadows accepted canonical `{canonical_symbol}` \
                         (concept `{concept_id}`)",
                        ty.symbol
                    ),
                    why: vec![
                        format!("single-field struct named `{}`", ty.name),
                        format!("canonical for `{concept_id}`: `{canonical_symbol}`"),
                        format!("inference confidence: {confidence:.2}"),
                    ],
                    suggested_fix: Some(format!(
                        "remove `{}` and import `{canonical_symbol}` instead; if this \
                         really is a parallel boundary representation, accept it via \
                         `// locus: ot boundary {concept_id} <name>` then rerun `locus init`",
                        ty.symbol
                    )),
                });
            }
        }
    }
    out
}
