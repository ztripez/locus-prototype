//! `locus ob ...` — symbol-by-symbol mutators for the OB lockfile section.
//!
//! Mirror of DG/UT's `edit` module.

use thiserror::Error;

use super::lockfile_schema::ObSection;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ObEditError {
    #[error("observer path pattern must not be empty")]
    EmptyObserverPath,
    #[error("forbidden log target pattern must not be empty")]
    EmptyForbiddenLogTarget,
}

/// Append an `observer_paths` pattern. Duplicate patterns are silently deduped.
pub fn add_observer_path(section: &mut ObSection, pattern: &str) -> Result<(), ObEditError> {
    if pattern.is_empty() {
        return Err(ObEditError::EmptyObserverPath);
    }
    if !section.observer_paths.iter().any(|p| p == pattern) {
        section.observer_paths.push(pattern.to_string());
    }
    Ok(())
}

/// Append a `forbidden_log_targets` pattern. Duplicate patterns are silently deduped.
pub fn add_forbidden_log_target(section: &mut ObSection, pattern: &str) -> Result<(), ObEditError> {
    if pattern.is_empty() {
        return Err(ObEditError::EmptyForbiddenLogTarget);
    }
    if !section.forbidden_log_targets.iter().any(|p| p == pattern) {
        section.forbidden_log_targets.push(pattern.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_observer_path_appends_and_dedupes() {
        let mut section = ObSection::default();
        add_observer_path(&mut section, "*::tests::*").unwrap();
        add_observer_path(&mut section, "*::cli::*").unwrap();
        add_observer_path(&mut section, "*::tests::*").unwrap(); // duplicate
        assert_eq!(section.observer_paths, vec!["*::tests::*", "*::cli::*"]);
    }

    #[test]
    fn add_observer_path_rejects_empty() {
        let mut section = ObSection::default();
        assert_eq!(
            add_observer_path(&mut section, "").unwrap_err(),
            ObEditError::EmptyObserverPath
        );
        assert!(section.observer_paths.is_empty());
    }

    #[test]
    fn add_forbidden_log_target_appends_and_dedupes() {
        // Start from an empty list of forbidden targets so we can assert the
        // exact resulting vec without depending on the default baseline.
        let mut section = ObSection {
            observer_paths: Vec::new(),
            forbidden_log_targets: Vec::new(),
            ..ObSection::default()
        };
        add_forbidden_log_target(&mut section, "println").unwrap();
        add_forbidden_log_target(&mut section, "tracing::info").unwrap();
        add_forbidden_log_target(&mut section, "println").unwrap(); // duplicate
        assert_eq!(
            section.forbidden_log_targets,
            vec!["println", "tracing::info"]
        );
    }

    #[test]
    fn add_forbidden_log_target_rejects_empty() {
        let mut section = ObSection::default();
        let before = section.forbidden_log_targets.clone();
        assert_eq!(
            add_forbidden_log_target(&mut section, "").unwrap_err(),
            ObEditError::EmptyForbiddenLogTarget
        );
        assert_eq!(section.forbidden_log_targets, before);
    }
}
