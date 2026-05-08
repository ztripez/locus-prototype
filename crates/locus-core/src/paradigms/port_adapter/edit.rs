//! `locus pa ...` — symbol-by-symbol mutators for the PA lockfile section.
//!
//! Mirror of DG/UT's `edit` module.

use thiserror::Error;

use super::lockfile_schema::PaSection;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PaEditError {
    #[error("accepted co-located trait pattern must not be empty")]
    EmptyPattern,
    #[error("application path pattern must not be empty")]
    EmptyApplicationPath,
}

/// Append an `accepted_colocated_traits` pattern. Duplicate patterns are
/// silently deduped — no value in erroring; the user already declared this once.
pub fn add_accepted_colocated(section: &mut PaSection, pattern: &str) -> Result<(), PaEditError> {
    if pattern.is_empty() {
        return Err(PaEditError::EmptyPattern);
    }
    if !section
        .accepted_colocated_traits
        .iter()
        .any(|p| p == pattern)
    {
        section.accepted_colocated_traits.push(pattern.to_string());
    }
    Ok(())
}

/// Append an `application_paths` pattern declaring a module as part of the
/// application layer (consumed by PA002). Duplicates are silently deduped.
pub fn add_application_path(section: &mut PaSection, pattern: &str) -> Result<(), PaEditError> {
    if pattern.is_empty() {
        return Err(PaEditError::EmptyApplicationPath);
    }
    if !section.application_paths.iter().any(|p| p == pattern) {
        section.application_paths.push(pattern.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_accepted_colocated_appends_and_dedupes() {
        let mut section = PaSection::default();
        add_accepted_colocated(&mut section, "crate::utils::*").unwrap();
        add_accepted_colocated(&mut section, "Helper").unwrap();
        add_accepted_colocated(&mut section, "crate::utils::*").unwrap(); // duplicate
        assert_eq!(
            section.accepted_colocated_traits,
            vec!["crate::utils::*", "Helper"]
        );
    }

    #[test]
    fn add_accepted_colocated_rejects_empty() {
        let mut section = PaSection::default();
        assert_eq!(
            add_accepted_colocated(&mut section, "").unwrap_err(),
            PaEditError::EmptyPattern
        );
        assert!(section.accepted_colocated_traits.is_empty());
    }

    #[test]
    fn add_application_path_appends_and_dedupes() {
        let mut s = PaSection::default();
        add_application_path(&mut s, "crate::app::*").unwrap();
        add_application_path(&mut s, "crate::other::*").unwrap();
        add_application_path(&mut s, "crate::app::*").unwrap();
        assert_eq!(
            s.application_paths,
            vec!["crate::app::*", "crate::other::*"]
        );
    }

    #[test]
    fn add_application_path_rejects_empty() {
        let mut s = PaSection::default();
        assert_eq!(
            add_application_path(&mut s, "").unwrap_err(),
            PaEditError::EmptyApplicationPath
        );
    }
}
