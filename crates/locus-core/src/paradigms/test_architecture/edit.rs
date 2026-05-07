//! `locus ta ...` — symbol-by-symbol mutators for the TA lockfile section.
//!
//! Mirror of DG/UT's `edit` module.

use thiserror::Error;

use super::lockfile_schema::TaSection;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TaEditError {
    #[error("test path pattern must not be empty")]
    EmptyTestPath,
}

/// Append a `test_paths` pattern. Duplicate patterns are silently deduped.
pub fn add_test_path(section: &mut TaSection, pattern: &str) -> Result<(), TaEditError> {
    if pattern.is_empty() {
        return Err(TaEditError::EmptyTestPath);
    }
    if !section.test_paths.iter().any(|p| p == pattern) {
        section.test_paths.push(pattern.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_test_path_appends_and_dedupes() {
        let mut section = TaSection::default();
        add_test_path(&mut section, "*::tests::*").unwrap();
        add_test_path(&mut section, "tests::*").unwrap();
        add_test_path(&mut section, "*::tests::*").unwrap(); // duplicate
        assert_eq!(section.test_paths, vec!["*::tests::*", "tests::*"]);
    }

    #[test]
    fn add_test_path_rejects_empty() {
        let mut section = TaSection::default();
        assert_eq!(
            add_test_path(&mut section, "").unwrap_err(),
            TaEditError::EmptyTestPath
        );
        assert!(section.test_paths.is_empty());
    }
}
