//! `locus rw ...` — symbol-by-symbol mutators for the RW lockfile section.

use thiserror::Error;

use super::lockfile_schema::RwSection;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RwEditError {
    #[error("runtime owner pattern must not be empty")]
    EmptyRuntimeOwnerPath,
}

pub fn add_runtime_owner_path(section: &mut RwSection, pattern: &str) -> Result<(), RwEditError> {
    if pattern.is_empty() {
        return Err(RwEditError::EmptyRuntimeOwnerPath);
    }
    if !section.runtime_owner_paths.iter().any(|p| p == pattern) {
        section.runtime_owner_paths.push(pattern.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_runtime_owner_appends_and_dedupes() {
        let mut s = RwSection::default();
        add_runtime_owner_path(&mut s, "crate::runtime::*").unwrap();
        add_runtime_owner_path(&mut s, "crate::worker::*").unwrap();
        add_runtime_owner_path(&mut s, "crate::runtime::*").unwrap();
        assert_eq!(
            s.runtime_owner_paths,
            vec!["crate::runtime::*", "crate::worker::*"]
        );
    }

    #[test]
    fn add_runtime_owner_rejects_empty() {
        let mut s = RwSection::default();
        assert_eq!(
            add_runtime_owner_path(&mut s, "").unwrap_err(),
            RwEditError::EmptyRuntimeOwnerPath
        );
    }
}
