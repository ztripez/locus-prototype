//! `locus bo ...` — symbol-by-symbol mutators for the BO lockfile section.
//!
//! Mirror of DG/UT's `edit` module.

use thiserror::Error;

use super::lockfile_schema::BoSection;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum BoEditError {
    #[error("domain path pattern must not be empty")]
    EmptyDomainPath,
    #[error("forbidden import pattern must not be empty")]
    EmptyForbiddenImport,
}

/// Append a `domain_paths` pattern. Duplicate patterns are silently deduped.
pub fn add_domain_path(section: &mut BoSection, pattern: &str) -> Result<(), BoEditError> {
    if pattern.is_empty() {
        return Err(BoEditError::EmptyDomainPath);
    }
    if !section.domain_paths.iter().any(|p| p == pattern) {
        section.domain_paths.push(pattern.to_string());
    }
    Ok(())
}

/// Append a `forbidden_in_domain` pattern. Duplicate patterns are silently deduped.
pub fn add_forbidden_import(section: &mut BoSection, pattern: &str) -> Result<(), BoEditError> {
    if pattern.is_empty() {
        return Err(BoEditError::EmptyForbiddenImport);
    }
    if !section.forbidden_in_domain.iter().any(|p| p == pattern) {
        section.forbidden_in_domain.push(pattern.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_domain_path_appends_and_dedupes() {
        let mut section = BoSection::default();
        add_domain_path(&mut section, "crate::domain::*").unwrap();
        add_domain_path(&mut section, "crate::application::*").unwrap();
        add_domain_path(&mut section, "crate::domain::*").unwrap(); // duplicate
        assert_eq!(
            section.domain_paths,
            vec!["crate::domain::*", "crate::application::*"]
        );
    }

    #[test]
    fn add_domain_path_rejects_empty() {
        let mut section = BoSection::default();
        assert_eq!(
            add_domain_path(&mut section, "").unwrap_err(),
            BoEditError::EmptyDomainPath
        );
        assert!(section.domain_paths.is_empty());
    }

    #[test]
    fn add_forbidden_import_appends_and_dedupes() {
        let mut section = BoSection::default();
        add_forbidden_import(&mut section, "serde::*").unwrap();
        add_forbidden_import(&mut section, "sqlx::*").unwrap();
        add_forbidden_import(&mut section, "serde::*").unwrap(); // duplicate
        assert_eq!(section.forbidden_in_domain, vec!["serde::*", "sqlx::*"]);
    }

    #[test]
    fn add_forbidden_import_rejects_empty() {
        let mut section = BoSection::default();
        assert_eq!(
            add_forbidden_import(&mut section, "").unwrap_err(),
            BoEditError::EmptyForbiddenImport
        );
        assert!(section.forbidden_in_domain.is_empty());
    }
}
