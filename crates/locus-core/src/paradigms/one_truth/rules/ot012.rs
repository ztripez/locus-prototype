//! OT012 — primitive obsession around a known canonical.
//!
//! Fires for each struct field whose:
//! - name (snake_case) maps to an accepted canonical (PascalCase) by short name,
//! - type-text is a primitive (`String`, `&str`, integer, bool, …), and
//! - enclosing struct is not lockfile-accepted (i.e. not a boundary adapter).
//!
//! Boundary adapters are the legitimate place for primitive-typed fields
//! because they mirror the wire shape. Application/domain types should
//! carry the canonical value object instead.
//!
//! Confidence 0.70. Per the spec's agent-strict severity table this is
//! fatal under `--agent-strict` and warning otherwise.

use std::collections::BTreeMap;

use locus_air::{AirItem, AirWorkspace, TypeKind};

use super::super::lockfile_schema::OtSection;
use super::helpers::{is_primitive_type_text, snake_to_pascal};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

pub fn ot012(air: &AirWorkspace, section: &OtSection, mode: CheckMode) -> Vec<Diagnostic> {
    let mut canonical_short: BTreeMap<String, String> = BTreeMap::new();
    for (concept_id, entry) in &section.concepts {
        if let Some(short) = entry.canonical.symbol.rsplit("::").next() {
            canonical_short.insert(short.to_string(), concept_id.clone());
        }
    }
    if canonical_short.is_empty() {
        return Vec::new();
    }
    let confidence = 0.70;
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
                if ty.kind != TypeKind::Struct {
                    continue;
                }
                if section.role_of(&ty.symbol).is_some() {
                    continue; // accepted boundary or canonical — primitives OK here
                }
                for field in &ty.fields {
                    let Some(canonical_short_name) = snake_to_pascal(&field.name) else {
                        continue;
                    };
                    let Some(concept_id) = canonical_short.get(&canonical_short_name) else {
                        continue;
                    };
                    if !is_primitive_type_text(&field.type_text) {
                        continue;
                    }
                    out.push(Diagnostic {
                        rule_id: "OT012".to_string(),
                        severity,
                        span: ty.span.clone(),
                        concept: Some(concept_id.clone()),
                        message: format!(
                            "field `{}::{}: {}` is a primitive substitute for canonical \
                             `{canonical_short_name}` (concept `{concept_id}`)",
                            ty.symbol, field.name, field.type_text
                        ),
                        why: vec![
                            format!(
                                "field name `{}` maps to canonical `{canonical_short_name}`",
                                field.name
                            ),
                            format!("type `{}` is a primitive", field.type_text),
                            format!("enclosing type `{}` is not an accepted boundary", ty.symbol),
                            format!("inference confidence: {confidence:.2}"),
                        ],
                        suggested_fix: Some(format!(
                            "use `{canonical_short_name}` instead of `{}` for `{}`, or \
                             accept `{}` as a boundary via `// locus: ot boundary {concept_id} \
                             <name>` if it's a wire-shape adapter",
                            field.type_text, field.name, ty.symbol
                        )),
                    });
                }
            }
        }
    }
    out
}
