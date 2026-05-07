//! `locus da ...` — symbol-by-symbol mutators for the DA lockfile section.
//!
//! Mirror of DG/UT's `edit` module.

use thiserror::Error;

use super::lockfile_schema::DaSection;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DaEditError {
    #[error("accepted single-impl pattern must not be empty")]
    EmptyPattern,
}

/// Set the master `enabled` switch. There's only one — re-setting overwrites.
pub fn set_enabled(section: &mut DaSection, enabled: bool) {
    section.enabled = enabled;
}

/// Append an `accepted_single_impl` pattern. Duplicate patterns are silently deduped.
pub fn add_accepted_single_impl(section: &mut DaSection, pattern: &str) -> Result<(), DaEditError> {
    if pattern.is_empty() {
        return Err(DaEditError::EmptyPattern);
    }
    if !section.accepted_single_impl.iter().any(|p| p == pattern) {
        section.accepted_single_impl.push(pattern.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_enabled_toggles() {
        let mut section = DaSection::default();
        assert!(!section.enabled);
        set_enabled(&mut section, true);
        assert!(section.enabled);
        set_enabled(&mut section, false);
        assert!(!section.enabled);
    }

    #[test]
    fn add_accepted_single_impl_appends_and_dedupes() {
        let mut section = DaSection::default();
        add_accepted_single_impl(&mut section, "Clock").unwrap();
        add_accepted_single_impl(&mut section, "my_crate::ports::*").unwrap();
        add_accepted_single_impl(&mut section, "Clock").unwrap(); // duplicate
        assert_eq!(
            section.accepted_single_impl,
            vec!["Clock", "my_crate::ports::*"]
        );
    }

    #[test]
    fn add_accepted_single_impl_rejects_empty() {
        let mut section = DaSection::default();
        assert_eq!(
            add_accepted_single_impl(&mut section, "").unwrap_err(),
            DaEditError::EmptyPattern
        );
        assert!(section.accepted_single_impl.is_empty());
    }
}
