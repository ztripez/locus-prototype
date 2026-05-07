//! `locus cf ...` — symbol-by-symbol mutators for the CF lockfile section.
//!
//! Mirror of DG/UT's `edit` module.

use thiserror::Error;

use super::lockfile_schema::CfSection;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CfEditError {
    #[error("config path pattern must not be empty")]
    EmptyConfigPath,
}

/// Append a `config_paths` pattern. Duplicate patterns are silently deduped.
pub fn add_config_path(section: &mut CfSection, pattern: &str) -> Result<(), CfEditError> {
    if pattern.is_empty() {
        return Err(CfEditError::EmptyConfigPath);
    }
    if !section.config_paths.iter().any(|p| p == pattern) {
        section.config_paths.push(pattern.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_config_path_appends_and_dedupes() {
        let mut section = CfSection::default();
        add_config_path(&mut section, "crate::config::*").unwrap();
        add_config_path(&mut section, "crate::settings::*").unwrap();
        add_config_path(&mut section, "crate::config::*").unwrap(); // duplicate
        assert_eq!(
            section.config_paths,
            vec!["crate::config::*", "crate::settings::*"]
        );
    }

    #[test]
    fn add_config_path_rejects_empty() {
        let mut section = CfSection::default();
        assert_eq!(
            add_config_path(&mut section, "").unwrap_err(),
            CfEditError::EmptyConfigPath
        );
        assert!(section.config_paths.is_empty());
    }
}
