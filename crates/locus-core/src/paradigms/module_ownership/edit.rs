//! `locus mo ...` — symbol-by-symbol mutators for the MO lockfile section.
//!
//! Mirror of CX's `edit` module: per-module overrides are identity-keyed by
//! `module` and refuse silent overwrites unless `force` is set.

use thiserror::Error;

use super::lockfile_schema::{MoOverride, MoSection};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MoEditError {
    #[error("module pattern must not be empty")]
    EmptyModule,
    #[error("override for module `{0}` already exists; pass --force to update its budget")]
    DuplicateOverride(String),
}

/// Set the workspace-wide default budget for `pub` top-level types per file.
/// Overwrites any prior value.
pub fn set_default_max_public_types(section: &mut MoSection, max: u32) {
    section.default_max_public_types = Some(max);
}

/// Add (or overwrite, with `force`) a per-module override. Identity is the
/// `module` pattern; refuses duplicates without `force`, replaces the budget
/// when `force` is set.
pub fn add_override(
    section: &mut MoSection,
    module: &str,
    max: u32,
    force: bool,
) -> Result<(), MoEditError> {
    if module.is_empty() {
        return Err(MoEditError::EmptyModule);
    }
    if let Some(existing) = section.overrides.iter_mut().find(|o| o.module == module) {
        if !force {
            return Err(MoEditError::DuplicateOverride(module.to_string()));
        }
        existing.max_public_types = max;
        return Ok(());
    }
    section.overrides.push(MoOverride {
        module: module.to_string(),
        max_public_types: max,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_default_max_public_types_writes_through() {
        let mut section = MoSection::default();
        set_default_max_public_types(&mut section, 7);
        assert_eq!(section.default_max_public_types, Some(7));
        set_default_max_public_types(&mut section, 3);
        assert_eq!(section.default_max_public_types, Some(3));
    }

    #[test]
    fn add_override_appends_a_new_entry() {
        let mut section = MoSection::default();
        add_override(&mut section, "lore::api::*", 20, false).unwrap();
        assert_eq!(section.overrides.len(), 1);
        assert_eq!(section.overrides[0].module, "lore::api::*");
        assert_eq!(section.overrides[0].max_public_types, 20);
    }

    #[test]
    fn add_override_rejects_duplicate_without_force() {
        let mut section = MoSection::default();
        add_override(&mut section, "lore::api::*", 20, false).unwrap();
        let err = add_override(&mut section, "lore::api::*", 10, false).unwrap_err();
        assert!(matches!(err, MoEditError::DuplicateOverride(_)));
        assert_eq!(section.overrides[0].max_public_types, 20);
    }

    #[test]
    fn add_override_with_force_updates_budget() {
        let mut section = MoSection::default();
        add_override(&mut section, "lore::api::*", 20, false).unwrap();
        add_override(&mut section, "lore::api::*", 10, true).unwrap();
        assert_eq!(section.overrides.len(), 1);
        assert_eq!(section.overrides[0].max_public_types, 10);
    }

    #[test]
    fn add_override_rejects_empty_module() {
        let mut section = MoSection::default();
        assert_eq!(
            add_override(&mut section, "", 5, false).unwrap_err(),
            MoEditError::EmptyModule
        );
        assert!(section.overrides.is_empty());
    }
}
