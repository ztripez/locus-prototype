//! `locus cx ...` — symbol-by-symbol mutators for the CX lockfile section.
//!
//! Mirror of DG's `edit` module: per-module overrides are identity-keyed by
//! `module` and refuse silent overwrites unless `force` is set.

use thiserror::Error;

use super::lockfile_schema::{CxOverride, CxSection};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CxEditError {
    #[error("module pattern must not be empty")]
    EmptyModule,
    #[error("override for module `{0}` already exists; pass --force to update its budget")]
    DuplicateOverride(String),
}

/// Set the workspace-wide default budget. Overwrites any prior value — there's
/// only ever one default and updating it is the natural intent.
pub fn set_default_max_lines(section: &mut CxSection, max: u32) {
    section.default_max_function_lines = Some(max);
}

/// Add (or overwrite, with `force`) a per-module override. Identity is the
/// `module` pattern; refuses duplicates without `force`, replaces the budget
/// when `force` is set.
pub fn add_override(
    section: &mut CxSection,
    module: &str,
    max: u32,
    force: bool,
) -> Result<(), CxEditError> {
    if module.is_empty() {
        return Err(CxEditError::EmptyModule);
    }
    if let Some(existing) = section.overrides.iter_mut().find(|o| o.module == module) {
        if !force {
            return Err(CxEditError::DuplicateOverride(module.to_string()));
        }
        existing.max_function_lines = max;
        return Ok(());
    }
    section.overrides.push(CxOverride {
        module: module.to_string(),
        max_function_lines: max,
        reason: None,
        expires: None,
        owner: None,
        debt_id: None,
        introduced_by: None,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_default_max_lines_writes_through() {
        let mut section = CxSection::default();
        set_default_max_lines(&mut section, 75);
        assert_eq!(section.default_max_function_lines, Some(75));
        // Re-setting overwrites.
        set_default_max_lines(&mut section, 30);
        assert_eq!(section.default_max_function_lines, Some(30));
    }

    #[test]
    fn add_override_appends_a_new_entry() {
        let mut section = CxSection::default();
        add_override(&mut section, "lore::parser::*", 200, false).unwrap();
        assert_eq!(section.overrides.len(), 1);
        assert_eq!(section.overrides[0].module, "lore::parser::*");
        assert_eq!(section.overrides[0].max_function_lines, 200);
    }

    #[test]
    fn add_override_rejects_duplicate_without_force() {
        let mut section = CxSection::default();
        add_override(&mut section, "lore::parser::*", 200, false).unwrap();
        let err = add_override(&mut section, "lore::parser::*", 100, false).unwrap_err();
        assert!(matches!(err, CxEditError::DuplicateOverride(_)));
        // Original value retained.
        assert_eq!(section.overrides[0].max_function_lines, 200);
    }

    #[test]
    fn add_override_with_force_updates_budget() {
        let mut section = CxSection::default();
        add_override(&mut section, "lore::parser::*", 200, false).unwrap();
        add_override(&mut section, "lore::parser::*", 100, true).unwrap();
        assert_eq!(section.overrides.len(), 1);
        assert_eq!(section.overrides[0].max_function_lines, 100);
    }

    #[test]
    fn add_override_rejects_empty_module() {
        let mut section = CxSection::default();
        assert_eq!(
            add_override(&mut section, "", 50, false).unwrap_err(),
            CxEditError::EmptyModule
        );
        assert!(section.overrides.is_empty());
    }
}
