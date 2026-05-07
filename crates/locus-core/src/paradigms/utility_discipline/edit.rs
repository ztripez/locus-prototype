//! `locus ut ...` — symbol-by-symbol mutators for the UT lockfile section.
//!
//! Mirror of DG's `edit` module. Only the UT001 surface is wired today;
//! more mutators land alongside UT002+.

use thiserror::Error;

use super::lockfile_schema::UtSection;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum UtEditError {
    #[error("utility path pattern must not be empty")]
    EmptyUtilityPath,
}

/// Append a utility-paths pattern. Duplicate patterns are silently deduped —
/// no value in erroring; the user already declared this once.
pub fn add_utility_path(section: &mut UtSection, pattern: &str) -> Result<(), UtEditError> {
    if pattern.is_empty() {
        return Err(UtEditError::EmptyUtilityPath);
    }
    if !section.utility_paths.iter().any(|p| p == pattern) {
        section.utility_paths.push(pattern.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_utility_path_appends_and_dedupes() {
        let mut section = UtSection::default();
        add_utility_path(&mut section, "x::utils::*").unwrap();
        add_utility_path(&mut section, "x::helpers::*").unwrap();
        add_utility_path(&mut section, "x::utils::*").unwrap(); // duplicate, silently deduped
        assert_eq!(section.utility_paths, vec!["x::utils::*", "x::helpers::*"]);
    }

    #[test]
    fn add_utility_path_rejects_empty() {
        let mut section = UtSection::default();
        assert_eq!(
            add_utility_path(&mut section, "").unwrap_err(),
            UtEditError::EmptyUtilityPath
        );
        assert!(section.utility_paths.is_empty());
    }
}
