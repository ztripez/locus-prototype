//! `locus pa ...` — symbol-by-symbol mutators for the PA lockfile section.
//!
//! Mirror of DG/UT's `edit` module.

use thiserror::Error;

use super::lockfile_schema::PaSection;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PaEditError {
    #[error("accepted co-located trait pattern must not be empty")]
    EmptyPattern,
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
}
