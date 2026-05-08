//! `locus rm ...` — symbol-by-symbol mutators for the RM lockfile section.
//!
//! Mirror of DG/UT's `edit` module.

use thiserror::Error;

use super::lockfile_schema::RmSection;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RmEditError {
    #[error("exempt path pattern must not be empty")]
    EmptyExemptPath,
    #[error("domain path pattern must not be empty")]
    EmptyDomainPath,
}

/// Set the workspace-wide default per-function action-kind cap. Overwrites
/// any prior value.
pub fn set_default_max_action_kinds(section: &mut RmSection, max: u32) {
    section.default_max_action_kinds = Some(max);
}

/// Append an `exempt_paths` pattern. Duplicate patterns are silently deduped.
pub fn add_exempt_path(section: &mut RmSection, pattern: &str) -> Result<(), RmEditError> {
    if pattern.is_empty() {
        return Err(RmEditError::EmptyExemptPath);
    }
    if !section.exempt_paths.iter().any(|p| p == pattern) {
        section.exempt_paths.push(pattern.to_string());
    }
    Ok(())
}

/// Append a `domain_paths_rm` pattern declaring a module as part of the
/// domain layer (consumed by RM006). Duplicates are silently deduped.
pub fn add_domain_path(section: &mut RmSection, pattern: &str) -> Result<(), RmEditError> {
    if pattern.is_empty() {
        return Err(RmEditError::EmptyDomainPath);
    }
    if !section.domain_paths_rm.iter().any(|p| p == pattern) {
        section.domain_paths_rm.push(pattern.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_default_max_action_kinds_writes_through() {
        let mut section = RmSection::default();
        set_default_max_action_kinds(&mut section, 3);
        assert_eq!(section.default_max_action_kinds, Some(3));
        set_default_max_action_kinds(&mut section, 2);
        assert_eq!(section.default_max_action_kinds, Some(2));
    }

    #[test]
    fn add_exempt_path_appends_and_dedupes() {
        let mut section = RmSection::default();
        add_exempt_path(&mut section, "*::tests::*").unwrap();
        add_exempt_path(&mut section, "*::main").unwrap();
        add_exempt_path(&mut section, "*::tests::*").unwrap(); // duplicate
        assert_eq!(section.exempt_paths, vec!["*::tests::*", "*::main"]);
    }

    #[test]
    fn add_exempt_path_rejects_empty() {
        let mut section = RmSection::default();
        assert_eq!(
            add_exempt_path(&mut section, "").unwrap_err(),
            RmEditError::EmptyExemptPath
        );
        assert!(section.exempt_paths.is_empty());
    }

    #[test]
    fn add_domain_path_appends_and_dedupes() {
        let mut s = RmSection::default();
        add_domain_path(&mut s, "crate::domain::*").unwrap();
        add_domain_path(&mut s, "crate::other::*").unwrap();
        add_domain_path(&mut s, "crate::domain::*").unwrap();
        assert_eq!(
            s.domain_paths_rm,
            vec!["crate::domain::*", "crate::other::*"]
        );
    }

    #[test]
    fn add_domain_path_rejects_empty() {
        let mut s = RmSection::default();
        assert_eq!(
            add_domain_path(&mut s, "").unwrap_err(),
            RmEditError::EmptyDomainPath
        );
    }
}
