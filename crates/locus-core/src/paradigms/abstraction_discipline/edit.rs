//! `locus ab ...` — symbol-by-symbol mutators for the AB lockfile section.
//!
//! Mirror of DG/UT's `edit` module. Only the AB001 surface is wired today;
//! more mutators land alongside AB002+.

use thiserror::Error;

use super::lockfile_schema::AbSection;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AbEditError {
    #[error("accepted single-impl trait pattern must not be empty")]
    EmptyPattern,
}

/// Append an `accepted_single_impl_traits` pattern. Duplicate patterns are
/// silently deduped — no value in erroring; the user already declared this once.
pub fn add_accepted_single_impl(section: &mut AbSection, pattern: &str) -> Result<(), AbEditError> {
    if pattern.is_empty() {
        return Err(AbEditError::EmptyPattern);
    }
    if !section
        .accepted_single_impl_traits
        .iter()
        .any(|p| p == pattern)
    {
        section
            .accepted_single_impl_traits
            .push(pattern.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_accepted_single_impl_appends_and_dedupes() {
        let mut section = AbSection::default();
        add_accepted_single_impl(&mut section, "crate::ports::*").unwrap();
        add_accepted_single_impl(&mut section, "Clock").unwrap();
        add_accepted_single_impl(&mut section, "crate::ports::*").unwrap(); // duplicate
        assert_eq!(
            section.accepted_single_impl_traits,
            vec!["crate::ports::*", "Clock"]
        );
    }

    #[test]
    fn add_accepted_single_impl_rejects_empty() {
        let mut section = AbSection::default();
        assert_eq!(
            add_accepted_single_impl(&mut section, "").unwrap_err(),
            AbEditError::EmptyPattern
        );
        assert!(section.accepted_single_impl_traits.is_empty());
    }
}
