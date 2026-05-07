//! `locus dc ...` — symbol-by-symbol mutators for the DC lockfile section.
//!
//! Mirror of DG/UT's `edit` module.

use thiserror::Error;

use super::lockfile_schema::DcSection;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DcEditError {
    #[error("exempt path pattern must not be empty")]
    EmptyExemptPath,
}

/// Set the `require_public_docs` switch. Re-setting overwrites — there's only
/// one and the natural intent of re-setting is to flip it.
pub fn set_require_public_docs(section: &mut DcSection, value: bool) {
    section.require_public_docs = value;
}

/// Append an `exempt_paths` pattern. Duplicate patterns are silently deduped.
pub fn add_exempt_path(section: &mut DcSection, pattern: &str) -> Result<(), DcEditError> {
    if pattern.is_empty() {
        return Err(DcEditError::EmptyExemptPath);
    }
    if !section.exempt_paths.iter().any(|p| p == pattern) {
        section.exempt_paths.push(pattern.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_require_public_docs_toggles() {
        let mut section = DcSection::default();
        assert!(!section.require_public_docs);
        set_require_public_docs(&mut section, true);
        assert!(section.require_public_docs);
        set_require_public_docs(&mut section, false);
        assert!(!section.require_public_docs);
    }

    #[test]
    fn add_exempt_path_appends_and_dedupes() {
        let mut section = DcSection::default();
        add_exempt_path(&mut section, "*::tests::*").unwrap();
        add_exempt_path(&mut section, "*::generated::*").unwrap();
        add_exempt_path(&mut section, "*::tests::*").unwrap(); // duplicate
        assert_eq!(section.exempt_paths, vec!["*::tests::*", "*::generated::*"]);
    }

    #[test]
    fn add_exempt_path_rejects_empty() {
        let mut section = DcSection::default();
        assert_eq!(
            add_exempt_path(&mut section, "").unwrap_err(),
            DcEditError::EmptyExemptPath
        );
        assert!(section.exempt_paths.is_empty());
    }
}
