//! `locus fl ...` — symbol-by-symbol mutators for the FL lockfile section.
//!
//! Mirror of DG/UT's `edit` module.

use thiserror::Error;

use super::lockfile_schema::FlSection;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FlEditError {
    #[error("domain path pattern must not be empty")]
    EmptyDomainPath,
    #[error("boundary error pattern must not be empty")]
    EmptyBoundaryErrorPattern,
}

/// Append a `domain_paths` pattern. Duplicate patterns are silently deduped.
pub fn add_domain_path(section: &mut FlSection, pattern: &str) -> Result<(), FlEditError> {
    if pattern.is_empty() {
        return Err(FlEditError::EmptyDomainPath);
    }
    if !section.domain_paths.iter().any(|p| p == pattern) {
        section.domain_paths.push(pattern.to_string());
    }
    Ok(())
}

/// Append a `boundary_error_patterns` pattern. Duplicate patterns are silently deduped.
pub fn add_boundary_error_pattern(
    section: &mut FlSection,
    pattern: &str,
) -> Result<(), FlEditError> {
    if pattern.is_empty() {
        return Err(FlEditError::EmptyBoundaryErrorPattern);
    }
    if !section.boundary_error_patterns.iter().any(|p| p == pattern) {
        section.boundary_error_patterns.push(pattern.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_domain_path_appends_and_dedupes() {
        let mut section = FlSection::default();
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
        let mut section = FlSection::default();
        assert_eq!(
            add_domain_path(&mut section, "").unwrap_err(),
            FlEditError::EmptyDomainPath
        );
        assert!(section.domain_paths.is_empty());
    }

    #[test]
    fn add_boundary_error_pattern_appends_and_dedupes() {
        let mut section = FlSection::default();
        add_boundary_error_pattern(&mut section, "reqwest::Error").unwrap();
        add_boundary_error_pattern(&mut section, "sqlx::*").unwrap();
        add_boundary_error_pattern(&mut section, "reqwest::Error").unwrap(); // duplicate
        assert_eq!(
            section.boundary_error_patterns,
            vec!["reqwest::Error", "sqlx::*"]
        );
    }

    #[test]
    fn add_boundary_error_pattern_rejects_empty() {
        let mut section = FlSection::default();
        assert_eq!(
            add_boundary_error_pattern(&mut section, "").unwrap_err(),
            FlEditError::EmptyBoundaryErrorPattern
        );
        assert!(section.boundary_error_patterns.is_empty());
    }
}
