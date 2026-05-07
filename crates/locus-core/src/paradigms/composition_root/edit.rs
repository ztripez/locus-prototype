//! `locus cr ...` — symbol-by-symbol mutators for the CR lockfile section.
//!
//! Mirror of DG/UT's `edit` module.

use thiserror::Error;

use super::lockfile_schema::CrSection;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CrEditError {
    #[error("composition root pattern must not be empty")]
    EmptyCompositionRoot,
}

/// Append a `composition_root_paths` pattern. Duplicate patterns are silently deduped.
pub fn add_composition_root(section: &mut CrSection, pattern: &str) -> Result<(), CrEditError> {
    if pattern.is_empty() {
        return Err(CrEditError::EmptyCompositionRoot);
    }
    if !section.composition_root_paths.iter().any(|p| p == pattern) {
        section.composition_root_paths.push(pattern.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_composition_root_appends_and_dedupes() {
        let mut section = CrSection::default();
        add_composition_root(&mut section, "bin::*").unwrap();
        add_composition_root(&mut section, "crate::wire").unwrap();
        add_composition_root(&mut section, "bin::*").unwrap(); // duplicate
        assert_eq!(
            section.composition_root_paths,
            vec!["bin::*", "crate::wire"]
        );
    }

    #[test]
    fn add_composition_root_rejects_empty() {
        let mut section = CrSection::default();
        assert_eq!(
            add_composition_root(&mut section, "").unwrap_err(),
            CrEditError::EmptyCompositionRoot
        );
        assert!(section.composition_root_paths.is_empty());
    }
}
