//! `locus accept` — symbol-by-symbol promotion into the OT lockfile.
//!
//! Used when a codebase isn't yet annotated with `// ot:` hints and the user
//! wants to record canonicals/boundaries by name. All operations validate
//! against AIR (the symbol must exist) and the existing OT section (no silent
//! collisions).

use locus_air::{AirItem, AirWorkspace};
use thiserror::Error;

use super::infer::stem_concept_id;
use super::lockfile_schema::{
    AcceptedBoundary, AcceptedCanonical, ConceptEntry, OtSection, Source,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AcceptError {
    #[error("symbol `{0}` was not found in the workspace AIR")]
    SymbolNotFound(String),
    #[error("concept `{0}` does not exist; accept its canonical first")]
    UnknownConcept(String),
    #[error("concept `{concept}` already has canonical `{existing}`; pass --force to replace")]
    CanonicalAlreadySet { concept: String, existing: String },
    #[error("symbol `{0}` is already accepted as canonical for some concept")]
    SymbolAlreadyCanonical(String),
    #[error("symbol `{0}` is already accepted as a boundary in some concept")]
    SymbolAlreadyBoundary(String),
}

/// Accept `symbol` as the canonical type for `concept`. If `concept` is `None`,
/// derive it from the symbol's name stem (same algorithm as `locus init`).
pub fn accept_canonical(
    section: &mut OtSection,
    air: &AirWorkspace,
    symbol: &str,
    concept: Option<&str>,
    force: bool,
) -> Result<String, AcceptError> {
    let ty_name = type_name_for_symbol(air, symbol)
        .ok_or_else(|| AcceptError::SymbolNotFound(symbol.to_string()))?;

    let concept_id = concept
        .map(str::to_string)
        .unwrap_or_else(|| stem_concept_id(&ty_name));

    if let Some((_, existing_concept)) = section.role_of(symbol)
        && existing_concept != concept_id
    {
        return Err(AcceptError::SymbolAlreadyCanonical(symbol.to_string()));
    }

    let entry = section
        .concepts
        .entry(concept_id.clone())
        .or_insert_with(|| ConceptEntry {
            canonical: AcceptedCanonical {
                symbol: symbol.to_string(),
                source: Source::Accepted,
            },
            boundaries: Vec::new(),
            converters: Vec::new(),
        });

    if entry.canonical.symbol != symbol {
        if !force {
            return Err(AcceptError::CanonicalAlreadySet {
                concept: concept_id,
                existing: entry.canonical.symbol.clone(),
            });
        }
        entry.canonical = AcceptedCanonical {
            symbol: symbol.to_string(),
            source: Source::Accepted,
        };
    } else if entry.canonical.source != Source::Accepted {
        // Promote to Accepted to mark that a human (re-)confirmed it.
        entry.canonical.source = Source::Accepted;
    }

    Ok(concept_id)
}

/// Accept `symbol` as a boundary adapter for `concept`. The concept must
/// already exist (its canonical must be accepted first). `boundary` is the
/// optional boundary label (e.g. `"api.v1"`).
pub fn accept_boundary(
    section: &mut OtSection,
    air: &AirWorkspace,
    symbol: &str,
    concept: &str,
    boundary: Option<&str>,
) -> Result<(), AcceptError> {
    if type_name_for_symbol(air, symbol).is_none() {
        return Err(AcceptError::SymbolNotFound(symbol.to_string()));
    }

    if let Some((_, existing_concept)) = section.role_of(symbol)
        && existing_concept != concept
    {
        return Err(AcceptError::SymbolAlreadyBoundary(symbol.to_string()));
    }

    let entry = section
        .concepts
        .get_mut(concept)
        .ok_or_else(|| AcceptError::UnknownConcept(concept.to_string()))?;

    if entry.canonical.symbol == symbol {
        return Err(AcceptError::SymbolAlreadyCanonical(symbol.to_string()));
    }

    if let Some(existing) = entry.boundaries.iter_mut().find(|b| b.symbol == symbol) {
        // Already accepted — just refresh the boundary label / source if
        // the user passed a new one.
        if let Some(b) = boundary {
            existing.boundary = Some(b.to_string());
        }
        existing.source = Source::Accepted;
    } else {
        entry.boundaries.push(AcceptedBoundary {
            symbol: symbol.to_string(),
            boundary: boundary.map(str::to_string),
            source: Source::Accepted,
        });
    }
    Ok(())
}

fn type_name_for_symbol(air: &AirWorkspace, symbol: &str) -> Option<String> {
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                if let AirItem::Type(ty) = item
                    && ty.symbol == symbol
                {
                    return Some(ty.name.clone());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{
        AIR_SCHEMA_VERSION, AirFile, AirItem, AirPackage, AirSpan, AirType, AirWorkspace, TypeKind,
        Visibility,
    };

    fn ty(symbol: &str, name: &str) -> AirItem {
        AirItem::Type(AirType {
            kind: TypeKind::Struct,
            name: name.into(),
            symbol: symbol.into(),
            visibility: Visibility::Public,
            fields: Vec::new(),
            variants: Vec::new(),
            derives: Vec::new(),
            attrs: Vec::new(),
            span: AirSpan::new("t.rs", 1, 1),
            doc: None,
        })
    }

    fn air_with(types: Vec<AirItem>) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0.1.0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "t.rs".into(),
                    module_path: Some("crate".into()),
                    items: types,
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                }],
            }],
            facts: Vec::new(),
        }
    }

    #[test]
    fn accept_canonical_derives_concept_from_stem() {
        let air = air_with(vec![ty("crate::User", "User")]);
        let mut section = OtSection::default();
        let cid = accept_canonical(&mut section, &air, "crate::User", None, false).unwrap();
        assert_eq!(cid, "user");
        let entry = section.concepts.get("user").unwrap();
        assert_eq!(entry.canonical.symbol, "crate::User");
        assert_eq!(entry.canonical.source, Source::Accepted);
    }

    #[test]
    fn accept_canonical_rejects_unknown_symbol() {
        let air = air_with(vec![ty("crate::User", "User")]);
        let mut section = OtSection::default();
        let err = accept_canonical(&mut section, &air, "crate::Nope", None, false).unwrap_err();
        assert_eq!(err, AcceptError::SymbolNotFound("crate::Nope".into()));
    }

    #[test]
    fn accept_canonical_rejects_replacement_without_force() {
        let air = air_with(vec![ty("crate::User", "User"), ty("crate::User2", "User2")]);
        let mut section = OtSection::default();
        accept_canonical(&mut section, &air, "crate::User", Some("user"), false).unwrap();
        let err =
            accept_canonical(&mut section, &air, "crate::User2", Some("user"), false).unwrap_err();
        assert!(matches!(err, AcceptError::CanonicalAlreadySet { .. }));
    }

    #[test]
    fn accept_canonical_replaces_with_force() {
        let air = air_with(vec![ty("crate::User", "User"), ty("crate::User2", "User2")]);
        let mut section = OtSection::default();
        accept_canonical(&mut section, &air, "crate::User", Some("user"), false).unwrap();
        accept_canonical(&mut section, &air, "crate::User2", Some("user"), true).unwrap();
        assert_eq!(
            section.concepts.get("user").unwrap().canonical.symbol,
            "crate::User2"
        );
    }

    #[test]
    fn accept_boundary_requires_existing_concept() {
        let air = air_with(vec![ty("crate::UserDto", "UserDto")]);
        let mut section = OtSection::default();
        let err = accept_boundary(&mut section, &air, "crate::UserDto", "user", Some("api.v1"))
            .unwrap_err();
        assert_eq!(err, AcceptError::UnknownConcept("user".into()));
    }

    #[test]
    fn accept_boundary_records_label() {
        let air = air_with(vec![
            ty("crate::User", "User"),
            ty("crate::UserDto", "UserDto"),
        ]);
        let mut section = OtSection::default();
        accept_canonical(&mut section, &air, "crate::User", None, false).unwrap();
        accept_boundary(&mut section, &air, "crate::UserDto", "user", Some("api.v1")).unwrap();
        let entry = section.concepts.get("user").unwrap();
        assert_eq!(entry.boundaries.len(), 1);
        assert_eq!(entry.boundaries[0].symbol, "crate::UserDto");
        assert_eq!(entry.boundaries[0].boundary.as_deref(), Some("api.v1"));
        assert_eq!(entry.boundaries[0].source, Source::Accepted);
    }

    #[test]
    fn accept_boundary_rejects_symbol_already_canonical_in_other_concept() {
        let air = air_with(vec![ty("crate::User", "User"), ty("crate::Team", "Team")]);
        let mut section = OtSection::default();
        accept_canonical(&mut section, &air, "crate::User", Some("user"), false).unwrap();
        accept_canonical(&mut section, &air, "crate::Team", Some("team"), false).unwrap();
        let err = accept_boundary(&mut section, &air, "crate::Team", "user", None).unwrap_err();
        assert_eq!(
            err,
            AcceptError::SymbolAlreadyBoundary("crate::Team".into())
        );
    }
}
