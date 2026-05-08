//! `locus er ...` — symbol-by-symbol mutators for the ER lockfile section.

use thiserror::Error;

use super::lockfile_schema::ErSection;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ErEditError {
    #[error("domain path pattern must not be empty")]
    EmptyDomainPath,
}

pub fn add_domain_path(section: &mut ErSection, pattern: &str) -> Result<(), ErEditError> {
    if pattern.is_empty() {
        return Err(ErEditError::EmptyDomainPath);
    }
    if !section.domain_paths.iter().any(|p| p == pattern) {
        section.domain_paths.push(pattern.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_domain_path_appends_and_dedupes() {
        let mut s = ErSection::default();
        add_domain_path(&mut s, "crate::domain::*").unwrap();
        add_domain_path(&mut s, "crate::other::*").unwrap();
        add_domain_path(&mut s, "crate::domain::*").unwrap();
        assert_eq!(s.domain_paths, vec!["crate::domain::*", "crate::other::*"]);
    }

    #[test]
    fn add_domain_path_rejects_empty() {
        let mut s = ErSection::default();
        assert_eq!(
            add_domain_path(&mut s, "").unwrap_err(),
            ErEditError::EmptyDomainPath
        );
    }
}
